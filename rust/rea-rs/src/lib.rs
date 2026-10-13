pub mod model;
pub use model::{Coverage, EntryKind, EntryStatus, FormatBasis, InventoryEntry, InventoryIssue, InventoryReport, Limits, Reason, Summary};

#[cfg(not(target_os = "linux"))]
use model::{detect_format, relative_path, same_file_version};
#[cfg(not(target_os = "linux"))]
use sha2::{Digest, Sha256};
#[cfg(not(target_os = "linux"))]
use std::{
    fs::{self, File, Metadata},
    io::{self, Read},
    path::{Path, PathBuf},
};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::inventory;

#[cfg(not(target_os = "linux"))]
const HARD_MAX_FILES: usize = 250_000;
const HARD_MAX_PATHS: usize = 250_000;
const HARD_MAX_PATH_BYTES: usize = 4_096;
const HARD_MAX_TOTAL_PATH_BYTES: usize = 64 * 1024 * 1024;
#[cfg(not(target_os = "linux"))]
const HARD_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
#[cfg(not(target_os = "linux"))]
const HARD_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
#[cfg(not(target_os = "linux"))]
const HARD_MAX_DEPTH: usize = 256;
#[cfg(not(target_os = "linux"))]
const HARD_MAX_ENTRIES_PER_DIRECTORY: usize = 100_000;
#[cfg(not(target_os = "linux"))]
const READ_BUFFER_BYTES: usize = 64 * 1024;

#[cfg(not(target_os = "linux"))]
/// Inventory a directory without following nested symlinks or executing inputs.
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
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "requested limits exceed hard safety bounds"));
    }
    let root = fs::canonicalize(root)?;
    if !fs::metadata(&root)?.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "inventory root must be a directory"));
    }
    let mut walker = Walker {
        root: root.clone(),
        limits: limits.clone(),
        entries: Vec::new(),
        issues: Vec::new(),
        bytes_read: 0,
        files_attempted: 0,
        paths_examined: 0,
        total_path_bytes: 0,
        stopped: false,
    };
    walker.visit_dir(&root, 0);

    walker.entries.sort_by(|a, b| a.path.cmp(&b.path));
    walker.issues.sort_by(|a, b| {
        a.path.cmp(&b.path).then_with(|| a.reason.cmp(&b.reason))
    });
    let files_hashed = walker.entries.iter()
        .filter(|e| e.kind == EntryKind::File && e.status == EntryStatus::Hashed).count();
    let bytes_hashed = walker.entries.iter()
        .filter(|e| e.kind == EntryKind::File && e.status == EntryStatus::Hashed)
        .filter_map(|e| e.size_bytes).fold(0_u64, u64::saturating_add);
    let skipped_paths = walker.entries.iter().filter(|e| e.status == EntryStatus::Skipped).count();
    let error_paths = walker.entries.iter().filter(|e| e.status == EntryStatus::Error).count();
    let partial = !walker.issues.is_empty()
        || walker.entries.iter().any(|e| e.status != EntryStatus::Hashed);

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

#[cfg(not(target_os = "linux"))]
struct Walker {
    root: PathBuf,
    limits: Limits,
    entries: Vec<InventoryEntry>,
    issues: Vec<InventoryIssue>,
    bytes_read: u64,
    files_attempted: usize,
    paths_examined: usize,
    total_path_bytes: usize,
    stopped: bool,
}

#[cfg(not(target_os = "linux"))]
impl Walker {
    fn visit_dir(&mut self, dir: &Path, depth: usize) {
        if self.stopped { return; }
        if depth > self.limits.max_depth {
            self.issue(dir, Reason::DepthLimit);
            return;
        }
        let read_dir = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => { self.issue(dir, Reason::Unreadable); return; }
        };

        // Skip an oversized directory as a whole, not an order-dependent subset.
        let mut children = Vec::new();
        let mut too_many = false;
        for result in read_dir {
            match result {
                Ok(entry) => {
                    let child = entry.path();
                    let parent_relative = relative_path(&self.root, dir);
                    if self.paths_examined >= self.limits.max_paths {
                        self.issues.push(InventoryIssue { path: parent_relative, reason: Reason::PathCountLimit });
                        self.stopped = true;
                        break;
                    }
                    self.paths_examined += 1;
                    children.push(child);
                    if children.len() > self.limits.max_entries_per_directory {
                        too_many = true;
                        break;
                    }
                }
                Err(_) => {
                    self.issue(dir, Reason::Unreadable);
                    break;
                }
            }
        }
        if self.stopped {
            return;
        }
        if too_many {
            self.issue(dir, Reason::DirectoryEntryLimit);
            return;
        }
        children.sort_by_key(|p| relative_path(&self.root, p));

        for path in children {
            if self.stopped { break; }
            let relative = relative_path(&self.root, &path);
            if relative.len() > self.limits.max_path_bytes {
                self.issues.push(InventoryIssue { path: relative_path(&self.root, dir), reason: Reason::PathLengthLimit });
                self.stopped = true;
                break;
            }
            let next_path_bytes = self.total_path_bytes.saturating_add(relative.len());
            if next_path_bytes > self.limits.max_total_path_bytes {
                self.issues.push(InventoryIssue { path: relative_path(&self.root, dir), reason: Reason::PathBytesLimit });
                self.stopped = true;
                break;
            }
            self.total_path_bytes = next_path_bytes;
            let metadata = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => {
                    self.issues.push(InventoryIssue { path: relative, reason: Reason::Unreadable });
                    continue;
                }
            };
            let ty = metadata.file_type();
            if ty.is_symlink() {
                self.entries.push(InventoryEntry {
                    path: relative, kind: EntryKind::Symlink, status: EntryStatus::Skipped,
                    size_bytes: None, sha256: None, format_hint: None, format_basis: None,
                    reason: Some(Reason::SymlinkNotFollowed),
                });
                continue;
            }
            if ty.is_dir() {
                self.visit_dir(&path, depth + 1);
                continue;
            }
            if !ty.is_file() {
                self.entries.push(InventoryEntry {
                    path: relative, kind: EntryKind::Other, status: EntryStatus::Skipped,
                    size_bytes: Some(metadata.len()), sha256: None, format_hint: None,
                    format_basis: None, reason: Some(Reason::NonRegularFile),
                });
                continue;
            }
            if self.files_attempted >= self.limits.max_files {
                self.issues.push(InventoryIssue { path: relative, reason: Reason::FileCountLimit });
                self.stopped = true;
                break;
            }
            self.files_attempted += 1;
            let remaining = self.limits.max_total_bytes.saturating_sub(self.bytes_read);
            let outcome = hash_file(&self.root, &path, &metadata, self.limits.max_file_bytes, remaining);
            self.bytes_read = self.bytes_read.saturating_add(outcome.bytes_read);
            self.entries.push(outcome.entry);
        }
    }

    fn issue(&mut self, path: &Path, reason: Reason) {
        self.issues.push(InventoryIssue { path: relative_path(&self.root, path), reason });
    }
}

#[cfg(not(target_os = "linux"))]
struct HashOutcome { entry: InventoryEntry, bytes_read: u64 }

#[cfg(not(target_os = "linux"))]
fn hash_file(root: &Path, path: &Path, before: &Metadata, max_file: u64, remaining: u64) -> HashOutcome {
    let relative = relative_path(root, path);
    let size = before.len();
    if size > max_file { return skipped(relative, size, Reason::FileSizeLimit); }
    if size > remaining { return skipped(relative, size, Reason::TotalSizeLimit); }

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return error_entry(relative, size, Reason::Unreadable, 0),
    };
    let opened = match file.metadata() {
        Ok(m) => m,
        Err(_) => return error_entry(relative, size, Reason::Unreadable, 0),
    };
    if !opened.is_file() || !same_file_version(before, &opened) {
        return error_entry(relative, size, Reason::UnstableFile, 0);
    }

    // The reader can consume at most the remaining global budget.
    let limit = max_file.min(remaining);
    let mut reader = file.take(limit);
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
            Err(_) => return error_entry(relative, size, Reason::Unreadable, bytes_read),
        }
    }

    let after_handle = match reader.get_ref().metadata() {
        Ok(m) => m,
        Err(_) => return error_entry(relative, size, Reason::Unreadable, bytes_read),
    };
    let after_path = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return error_entry(relative, size, Reason::UnstableFile, bytes_read),
    };
    if bytes_read != size || !same_file_version(&opened, &after_handle)
        || !after_path.is_file() || !same_file_version(&opened, &after_path) {
        return error_entry(relative, size, Reason::UnstableFile, bytes_read);
    }

    let (hint, basis) = detect_format(&prefix);
    HashOutcome {
        entry: InventoryEntry {
            path: relative, kind: EntryKind::File, status: EntryStatus::Hashed,
            size_bytes: Some(size), sha256: Some(to_hex(&hasher.finalize())),
            format_hint: Some(hint), format_basis: Some(basis), reason: None,
        },
        bytes_read,
    }
}

#[cfg(not(target_os = "linux"))]
fn skipped(path: String, size: u64, reason: Reason) -> HashOutcome {
    HashOutcome {
        entry: InventoryEntry {
            path, kind: EntryKind::File, status: EntryStatus::Skipped, size_bytes: Some(size),
            sha256: None, format_hint: None, format_basis: None, reason: Some(reason),
        },
        bytes_read: 0,
    }
}

#[cfg(not(target_os = "linux"))]
fn error_entry(path: String, size: u64, reason: Reason, bytes_read: u64) -> HashOutcome {
    HashOutcome {
        entry: InventoryEntry {
            path, kind: EntryKind::File, status: EntryStatus::Error, size_bytes: Some(size),
            sha256: None, format_hint: None, format_basis: None, reason: Some(reason),
        },
        bytes_read,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests;
