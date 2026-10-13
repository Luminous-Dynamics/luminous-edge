//! Linux descriptor-relative inventory.
//!
//! Child lookups are relative to an already-open parent directory descriptor.
//! A no-follow metadata query plus a no-follow open prevents child symlinks
//! from being intentionally traversed, including during rename races.

use super::to_hex;
use crate::model::{
    detect_format, encode_component, encode_relative_path, same_file_version, Coverage,
    EntryKind, EntryStatus, InventoryEntry, InventoryIssue, InventoryReport, Limits, Reason,
    Summary,
};
use rustix::fs::{openat, statat, AtFlags, Dir, FileType, Mode, OFlags, Stat};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::{self, File, Metadata},
    io::{self, Read},
    os::unix::ffi::OsStringExt,
    path::{Component, Path, PathBuf},
};

const HARD_MAX_FILES: usize = 250_000;
const HARD_MAX_PATHS: usize = 250_000;
const HARD_MAX_PATH_BYTES: usize = 4_096;
const HARD_MAX_TOTAL_PATH_BYTES: usize = 64 * 1024 * 1024;
const HARD_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const HARD_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const HARD_MAX_DEPTH: usize = 256;
const HARD_MAX_ENTRIES_PER_DIRECTORY: usize = 100_000;
const READ_BUFFER_BYTES: usize = 64 * 1024;

pub fn inventory(root: &Path, limits: Limits) -> io::Result<InventoryReport> {
    if limits.max_files > HARD_MAX_FILES
        || limits.max_paths > HARD_MAX_PATHS
        || limits.max_path_bytes > HARD_MAX_PATH_BYTES
        || limits.max_total_path_bytes > HARD_MAX_TOTAL_PATH_BYTES
        || limits.max_file_bytes > HARD_MAX_FILE_BYTES
        || limits.max_total_bytes > HARD_MAX_TOTAL_BYTES
        || limits.max_depth > HARD_MAX_DEPTH
        || limits.max_entries_per_directory > HARD_MAX_ENTRIES_PER_DIRECTORY
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "requested limits exceed hard safety bounds",
        ));
    }

    // Resolve the user's chosen root, check its type before opening, then open
    // each canonical component with O_DIRECTORY|O_NOFOLLOW. This avoids blocking
    // on a FIFO and fails closed if a path component is swapped for a symlink.
    let root_path = fs::canonicalize(root)?;
    let expected_root_metadata = fs::metadata(&root_path)?;
    if !expected_root_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "inventory root must be a directory",
        ));
    }
    let root_dir = open_root_no_follow(&root_path)?;
    if !same_file_version(&expected_root_metadata, &root_dir.metadata()?) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "inventory root changed during acquisition",
        ));
    }

    let mut walker = Walker {
        limits: limits.clone(),
        entries: Vec::new(),
        issues: Vec::new(),
        bytes_read: 0,
        files_attempted: 0,
        paths_examined: 0,
        total_path_bytes: 0,
        stopped: false,
    };
    walker.visit_dir(&root_dir, Path::new(""), 0);

    walker.entries.sort_by(|a, b| a.path.cmp(&b.path));
    walker.issues.sort_by(|a, b| {
        a.path.cmp(&b.path).then_with(|| a.reason.cmp(&b.reason))
    });

    let files_hashed = walker.entries.iter()
        .filter(|entry| entry.kind == EntryKind::File && entry.status == EntryStatus::Hashed)
        .count();
    let bytes_hashed = walker.entries.iter()
        .filter(|entry| entry.kind == EntryKind::File && entry.status == EntryStatus::Hashed)
        .filter_map(|entry| entry.size_bytes)
        .fold(0_u64, u64::saturating_add);
    let skipped_paths = walker.entries.iter()
        .filter(|entry| entry.status == EntryStatus::Skipped).count();
    let error_paths = walker.entries.iter()
        .filter(|entry| entry.status == EntryStatus::Error).count();
    let partial = !walker.issues.is_empty()
        || walker.entries.iter().any(|entry| entry.status != EntryStatus::Hashed);

    Ok(InventoryReport {
        schema_version: "rea-rs-inventory/v0.1",
        coverage: if partial { Coverage::Partial } else { Coverage::Complete },
        policy: limits,
        summary: Summary {
            paths_reported: walker.entries.len(),
            path_bytes_accounted: walker.total_path_bytes,
            files_hashed,
            bytes_hashed,
            bytes_read: walker.bytes_read,
            skipped_paths,
            error_paths,
            traversal_issues: walker.issues.len(),
        },
        entries: walker.entries,
        issues: walker.issues,
    })
}

fn open_root_no_follow(root_path: &Path) -> io::Result<File> {
    let mut current = File::open("/")?;
    for component in root_path.components() {
        match component {
            Component::RootDir => {},
            Component::Normal(name) => {
                let fd = openat(
                    &current,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                    Mode::empty(),
                )?;
                current = File::from(fd);
            },
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "root must be canonical and absolute")),
        }
    }
    if !current.metadata()?.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "inventory root must be a directory"));
    }
    Ok(current)
}

struct Walker {
    limits: Limits,
    entries: Vec<InventoryEntry>,
    issues: Vec<InventoryIssue>,
    bytes_read: u64,
    files_attempted: usize,
    paths_examined: usize,
    total_path_bytes: usize,
    stopped: bool,
}

impl Walker {
    fn visit_dir(&mut self, directory: &File, relative_dir: &Path, depth: usize) {
        if self.stopped {
            return;
        }
        if depth > self.limits.max_depth {
            self.issue(relative_dir, Reason::DepthLimit);
            return;
        }

        let metadata_before = match directory.metadata() {
            Ok(metadata) if metadata.is_dir() => metadata,
            _ => {
                self.issue(relative_dir, Reason::Unreadable);
                return;
            }
        };

        // A fresh open of "." gives the directory stream its own file offset.
        // It remains anchored to the already-open directory descriptor.
        let stream_fd = match openat(
            directory,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => {
                self.issue(relative_dir, Reason::Unreadable);
                return;
            }
        };
        let mut stream = match Dir::new(stream_fd) {
            Ok(stream) => stream,
            Err(_) => {
                self.issue(relative_dir, Reason::Unreadable);
                return;
            }
        };

        let mut children = Vec::new();
        let mut too_many = false;
        let mut read_failed = false;
        for result in stream.by_ref() {
            let entry = match result {
                Ok(entry) => entry,
                Err(_) => {
                    read_failed = true;
                    break;
                }
            };
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = std::ffi::OsString::from_vec(bytes.to_vec());
            if self.paths_examined >= self.limits.max_paths {
                self.issues.push(InventoryIssue {
                    path: encode_relative_path(relative_dir),
                    reason: Reason::PathCountLimit,
                });
                self.stopped = true;
                break;
            }
            self.paths_examined += 1;
            children.push(name);
            if children.len() > self.limits.max_entries_per_directory {
                too_many = true;
                break;
            }
        }
        drop(stream);

        if read_failed {
            self.issue(relative_dir, Reason::Unreadable);
            return;
        }
        // A path limit hit while collecting names must stop this directory
        // before recursing; count names at discovery time to bound retained
        // child vectors across every recursive frame.
        if self.stopped {
            return;
        }
        if too_many {
            self.issue(relative_dir, Reason::DirectoryEntryLimit);
            return;
        }

        let metadata_after_listing = match directory.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                self.issue(relative_dir, Reason::UnstableDirectory);
                return;
            }
        };
        if !same_file_version(&metadata_before, &metadata_after_listing) {
            self.issue(relative_dir, Reason::UnstableDirectory);
            return;
        }

        // Sorting only names bounds memory by the per-directory entry cap and is
        // deterministic regardless of the filesystem's readdir order.
        children.sort_by_key(|name| encode_component(name));

        for name in children {
            if self.stopped {
                break;
            }
            let child_relative = if relative_dir.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                relative_dir.join(&name)
            };
            let report_path = encode_relative_path(&child_relative);
            if report_path.len() > self.limits.max_path_bytes {
                self.issues.push(InventoryIssue {
                    path: encode_relative_path(relative_dir),
                    reason: Reason::PathLengthLimit,
                });
                self.stopped = true;
                break;
            }
            let next_path_bytes = self.total_path_bytes.saturating_add(report_path.len());
            if next_path_bytes > self.limits.max_total_path_bytes {
                self.issues.push(InventoryIssue {
                    path: encode_relative_path(relative_dir),
                    reason: Reason::PathBytesLimit,
                });
                self.stopped = true;
                break;
            }
            self.total_path_bytes = next_path_bytes;

            let stat = match statat(directory, name.as_os_str(), AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => stat,
                Err(_) => {
                    self.issues.push(InventoryIssue {
                        path: report_path,
                        reason: Reason::Unreadable,
                    });
                    continue;
                }
            };
            let file_type = FileType::from_raw_mode(stat.st_mode);
            let size = match u64::try_from(stat.st_size) {
                Ok(size) => size,
                Err(_) => {
                    self.entries.push(InventoryEntry {
                        path: report_path,
                        kind: EntryKind::Other,
                        status: EntryStatus::Error,
                        size_bytes: None,
                        sha256: None,
                        format_hint: None,
                        format_basis: None,
                        reason: Some(Reason::Unreadable),
                    });
                    continue;
                }
            };

            if file_type.is_symlink() {
                self.entries.push(InventoryEntry {
                    path: report_path,
                    kind: EntryKind::Symlink,
                    status: EntryStatus::Skipped,
                    size_bytes: None,
                    sha256: None,
                    format_hint: None,
                    format_basis: None,
                    reason: Some(Reason::SymlinkNotFollowed),
                });
                continue;
            }

            if file_type.is_dir() {
                let fd = match openat(
                    directory,
                    name.as_os_str(),
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                    Mode::empty(),
                ) {
                    Ok(fd) => fd,
                    Err(_) => {
                        self.issues.push(InventoryIssue {
                            path: report_path,
                            reason: Reason::UnstableDirectory,
                        });
                        continue;
                    }
                };
                let child_dir = File::from(fd);
                let opened_metadata = match child_dir.metadata() {
                    Ok(metadata) if metadata.is_dir() => metadata,
                    _ => {
                        self.issues.push(InventoryIssue {
                            path: report_path,
                            reason: Reason::UnstableDirectory,
                        });
                        continue;
                    }
                };
                if !stat_matches_metadata(&stat, &opened_metadata) {
                    self.issues.push(InventoryIssue {
                        path: report_path,
                        reason: Reason::UnstableDirectory,
                    });
                    continue;
                }
                self.visit_dir(&child_dir, &child_relative, depth + 1);
                continue;
            }

            if !file_type.is_file() {
                self.entries.push(InventoryEntry {
                    path: report_path,
                    kind: EntryKind::Other,
                    status: EntryStatus::Skipped,
                    size_bytes: Some(size),
                    sha256: None,
                    format_hint: None,
                    format_basis: None,
                    reason: Some(Reason::NonRegularFile),
                });
                continue;
            }

            if self.files_attempted >= self.limits.max_files {
                self.issues.push(InventoryIssue {
                    path: report_path,
                    reason: Reason::FileCountLimit,
                });
                self.stopped = true;
                break;
            }
            self.files_attempted += 1;

            let remaining = self.limits.max_total_bytes.saturating_sub(self.bytes_read);
            let outcome = hash_regular_file(
                directory,
                &name,
                report_path,
                &stat,
                size,
                self.limits.max_file_bytes,
                remaining,
            );
            self.bytes_read = self.bytes_read.saturating_add(outcome.bytes_read);
            self.entries.push(outcome.entry);
        }

        // Entry mutation after readdir is complete must still make coverage partial.
        match directory.metadata() {
            Ok(metadata_after_walk) if same_file_version(&metadata_before, &metadata_after_walk) => {}
            _ => self.issue(relative_dir, Reason::UnstableDirectory),
        }
    }

    fn issue(&mut self, relative_path: &Path, reason: Reason) {
        self.issues.push(InventoryIssue {
            path: encode_relative_path(relative_path),
            reason,
        });
    }
}

struct HashOutcome {
    entry: InventoryEntry,
    bytes_read: u64,
}

fn hash_regular_file(
    parent: &File,
    name: &OsStr,
    report_path: String,
    initial_stat: &Stat,
    initial_size: u64,
    max_file_bytes: u64,
    remaining_total_bytes: u64,
) -> HashOutcome {
    if initial_size > max_file_bytes {
        return skipped_file(report_path, initial_size, Reason::FileSizeLimit);
    }
    if initial_size > remaining_total_bytes {
        return skipped_file(report_path, initial_size, Reason::TotalSizeLimit);
    }

    // Open relative to the pinned parent FD and explicitly refuse a final symlink.
    let fd = match openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(_) => return error_file(report_path, initial_size, Reason::UnstableFile, 0),
    };
    let file = File::from(fd);
    let opened_metadata = match file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return error_file(report_path, initial_size, Reason::UnstableFile, 0),
    };
    if !stat_matches_metadata(initial_stat, &opened_metadata) {
        return error_file(report_path, initial_size, Reason::UnstableFile, 0);
    }

    let read_limit = max_file_bytes.min(remaining_total_bytes);
    let mut reader = file.take(read_limit);
    let mut hasher = Sha256::new();
    let mut prefix = Vec::with_capacity(16);
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    let mut bytes_read = 0_u64;

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                bytes_read = bytes_read.saturating_add(count as u64);
                hasher.update(&buffer[..count]);
                let take = 16_usize.saturating_sub(prefix.len()).min(count);
                prefix.extend_from_slice(&buffer[..take]);
            }
            Err(_) => return error_file(report_path, initial_size, Reason::Unreadable, bytes_read),
        }
    }

    let after_handle = match reader.get_ref().metadata() {
        Ok(metadata) => metadata,
        Err(_) => return error_file(report_path, initial_size, Reason::Unreadable, bytes_read),
    };
    let after_name = match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(_) => return error_file(report_path, initial_size, Reason::UnstableFile, bytes_read),
    };

    if bytes_read != initial_size
        || !same_file_version(&opened_metadata, &after_handle)
        || !stat_matches_metadata(&after_name, &after_handle)
    {
        return error_file(report_path, initial_size, Reason::UnstableFile, bytes_read);
    }

    let (format_hint, format_basis) = detect_format(&prefix);
    HashOutcome {
        entry: InventoryEntry {
            path: report_path,
            kind: EntryKind::File,
            status: EntryStatus::Hashed,
            size_bytes: Some(initial_size),
            sha256: Some(to_hex(&hasher.finalize())),
            format_hint: Some(format_hint),
            format_basis: Some(format_basis),
            reason: None,
        },
        bytes_read,
    }
}

fn stat_matches_metadata(stat: &Stat, metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(size) = u64::try_from(stat.st_size) else {
        return false;
    };
    u128::from(stat.st_dev) == u128::from(metadata.dev())
        && u128::from(stat.st_ino) == u128::from(metadata.ino())
        && size == metadata.len()
        && stat.st_mtime == metadata.mtime()
        && i128::from(stat.st_mtime_nsec) == i128::from(metadata.mtime_nsec())
        && stat.st_ctime == metadata.ctime()
        && i128::from(stat.st_ctime_nsec) == i128::from(metadata.ctime_nsec())
}

fn skipped_file(path: String, size: u64, reason: Reason) -> HashOutcome {
    HashOutcome {
        entry: InventoryEntry {
            path,
            kind: EntryKind::File,
            status: EntryStatus::Skipped,
            size_bytes: Some(size),
            sha256: None,
            format_hint: None,
            format_basis: None,
            reason: Some(reason),
        },
        bytes_read: 0,
    }
}

fn error_file(path: String, size: u64, reason: Reason, bytes_read: u64) -> HashOutcome {
    HashOutcome {
        entry: InventoryEntry {
            path,
            kind: EntryKind::File,
            status: EntryStatus::Error,
            size_bytes: Some(size),
            sha256: None,
            format_hint: None,
            format_basis: None,
            reason: Some(reason),
        },
        bytes_read,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}
