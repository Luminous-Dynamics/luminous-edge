use super::*;
use sha2::{Digest, Sha256};
use crate::model::{detect_format, FormatBasis};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rea-rs-test-{}-{suffix}", std::process::id()));
        fs::create_dir(&path).expect("create unique test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sha256_matches_known_vector() {
    assert_eq!(
        to_hex(&Sha256::digest(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn inventory_hashes_files_and_orders_paths() {
    let temp = TestDirectory::new();
    fs::write(temp.0.join("z.txt"), b"last").expect("write z");
    fs::write(temp.0.join("a.txt"), b"abc").expect("write a");

    let report = inventory(&temp.0, Limits::default()).expect("inventory");
    assert_eq!(report.coverage, Coverage::Complete);
    assert_eq!(report.entries[0].path, "a.txt");
    assert_eq!(
        report.entries[0].sha256.as_deref(),
        Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(report.summary.files_hashed, 2);
    assert_eq!(report.summary.bytes_hashed, 7);
    assert_eq!(report.summary.path_bytes_accounted, 10);
}

#[test]
fn oversized_file_is_partial_and_has_no_digest() {
    let temp = TestDirectory::new();
    fs::write(temp.0.join("large.bin"), b"123").expect("write file");
    let limits = Limits { max_file_bytes: 2, ..Limits::default() };

    let report = inventory(&temp.0, limits).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert_eq!(report.entries[0].status, EntryStatus::Skipped);
    assert_eq!(report.entries[0].reason, Some(Reason::FileSizeLimit));
    assert!(report.entries[0].sha256.is_none());
    assert_eq!(report.summary.bytes_read, 0);
}

#[test]
fn global_byte_budget_is_explicit() {
    let temp = TestDirectory::new();
    fs::write(temp.0.join("a"), b"aa").expect("write a");
    fs::write(temp.0.join("b"), b"bb").expect("write b");
    let limits = Limits { max_total_bytes: 3, ..Limits::default() };

    let report = inventory(&temp.0, limits).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert_eq!(report.summary.bytes_read, 2);
    assert_eq!(report.entries[1].status, EntryStatus::Skipped);
    assert_eq!(report.entries[1].reason, Some(Reason::TotalSizeLimit));
    assert!(report.entries[1].sha256.is_none());
}

#[test]
fn format_hints_are_signatures_not_validation() {
    assert_eq!(
        detect_format(b""),
        ("empty_file".to_owned(), FormatBasis::Unknown)
    );
    assert_eq!(
        detect_format(b"\x7fELFgarbage"),
        ("elf".to_owned(), FormatBasis::MagicBytes)
    );
    assert_eq!(
        detect_format(b"\0asm\x01\0\0\0"),
        ("wasm".to_owned(), FormatBasis::MagicBytes)
    );
    assert_eq!(
        detect_format(b"{\n  \"key\":"),
        ("json_candidate".to_owned(), FormatBasis::Utf8Prefix)
    );
}

#[cfg(unix)]
#[test]
fn symlink_is_reported_but_never_followed() {
    use std::os::unix::fs::symlink;

    let temp = TestDirectory::new();
    let outside = TestDirectory::new();
    fs::write(outside.0.join("secret.txt"), b"not read").expect("write outside");
    symlink(outside.0.join("secret.txt"), temp.0.join("link.txt")).expect("create symlink");

    let report = inventory(&temp.0, Limits::default()).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].kind, EntryKind::Symlink);
    assert_eq!(report.entries[0].reason, Some(Reason::SymlinkNotFollowed));
    assert_eq!(report.summary.bytes_read, 0);
}

#[test]
fn file_count_limit_is_reported_not_silently_passed() {
    let temp = TestDirectory::new();
    fs::write(temp.0.join("a"), b"a").expect("write a");
    fs::write(temp.0.join("b"), b"b").expect("write b");
    let limits = Limits { max_files: 1, ..Limits::default() };

    let report = inventory(&temp.0, limits).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert_eq!(report.summary.files_hashed, 1);
    assert!(report.issues.iter().any(|issue| issue.reason == Reason::FileCountLimit));
}

#[cfg(unix)]
#[test]
fn non_utf8_path_is_percent_encoded_without_loss() {
    use std::os::unix::ffi::OsStringExt;

    let temp = TestDirectory::new();
    let name = std::ffi::OsString::from_vec(b"bad\xffname".to_vec());
    fs::write(temp.0.join(name), b"data").expect("write non-UTF8 name");
    let report = inventory(&temp.0, Limits::default()).expect("inventory");
    assert_eq!(report.entries[0].path, "bad%FFname");
    assert_eq!(report.coverage, Coverage::Complete);
}

#[cfg(unix)]
#[test]
fn symlink_to_directory_is_recorded_without_scanning_target() {
    use std::os::unix::fs::symlink;

    let root = TestDirectory::new();
    let outside = TestDirectory::new();
    fs::write(outside.0.join("outside-secret.bin"), b"should not be read")
        .expect("write outside fixture");
    symlink(&outside.0, root.0.join("external-dir")).expect("create directory symlink");

    let report = inventory(&root.0, Limits::default()).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].kind, EntryKind::Symlink);
    assert_eq!(report.entries[0].path, "external-dir");
    assert_eq!(report.summary.bytes_read, 0);
    assert!(!report.entries.iter().any(|entry| entry.path.contains("outside-secret")));
}

#[test]
fn too_many_directory_entries_are_partial() {
    let root = TestDirectory::new();
    fs::write(root.0.join("a"), b"a").expect("write a");
    fs::write(root.0.join("b"), b"b").expect("write b");
    let limits = Limits {
        max_entries_per_directory: 1,
        ..Limits::default()
    };

    let report = inventory(&root.0, limits).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert!(report.entries.is_empty());
    assert!(report.issues.iter().any(|issue| issue.reason == Reason::DirectoryEntryLimit));
}

#[test]
fn global_path_budget_caps_files_links_and_directories() {
    let root = TestDirectory::new();
    fs::create_dir(root.0.join("a-dir")).expect("create directory");
    fs::write(root.0.join("b-file"), b"b").expect("write file");
    let limits = Limits {
        max_paths: 1,
        ..Limits::default()
    };

    let report = inventory(&root.0, limits).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert_eq!(report.summary.paths_reported, 0);
    assert!(report.issues.iter().any(|issue| issue.reason == Reason::PathCountLimit));
}

#[test]
fn long_relative_path_stops_traversal_without_emitting_a_truncated_path() {
    let root = TestDirectory::new();
    fs::write(root.0.join("long-name"), b"x").expect("write fixture");
    let limits = Limits {
        max_path_bytes: 4,
        ..Limits::default()
    };

    let report = inventory(&root.0, limits).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert!(report.entries.is_empty());
    assert_eq!(report.issues[0].path, ".");
    assert_eq!(report.issues[0].reason, Reason::PathLengthLimit);
    assert_eq!(report.summary.path_bytes_accounted, 0);
}

#[test]
fn aggregate_path_byte_budget_uses_sorted_paths() {
    let root = TestDirectory::new();
    fs::write(root.0.join("b"), b"b").expect("write b");
    fs::write(root.0.join("a"), b"a").expect("write a");
    let limits = Limits {
        max_total_path_bytes: 1,
        ..Limits::default()
    };

    let report = inventory(&root.0, limits).expect("inventory");
    assert_eq!(report.coverage, Coverage::Partial);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].path, "a");
    assert_eq!(report.summary.path_bytes_accounted, 1);
    assert!(report.issues.iter().any(|issue| issue.reason == Reason::PathBytesLimit));
}

#[test]
fn hard_resource_ceilings_cannot_be_overridden() {
    let root = TestDirectory::new();
    let limits = [
        Limits {
            max_paths: 250_001,
            ..Limits::default()
        },
        Limits {
            max_path_bytes: 4_097,
            ..Limits::default()
        },
        Limits {
            max_total_path_bytes: 64 * 1024 * 1024 + 1,
            ..Limits::default()
        },
    ];

    for policy in limits {
        let error = inventory(&root.0, policy).expect_err("excessive limits are rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}

#[test]
fn json_report_is_stable_and_does_not_leak_absolute_root() {
    let temp = TestDirectory::new();
    fs::write(temp.0.join("hello.txt"), b"hello").expect("write fixture");
    let report = inventory(&temp.0, Limits::default()).expect("inventory");
    let json = serde_json::to_string(&report).expect("serialize report");

    assert!(!json.contains(&temp.0.to_string_lossy().to_string()));
    assert!(json.contains(r#""schema_version":"rea-rs-inventory/v0.1""#));
    assert!(json.contains(r#""path":"hello.txt""#));
}

#[test]
fn a_file_cannot_be_used_as_the_inventory_root() {
    let temp = TestDirectory::new();
    let file = temp.0.join("file");
    fs::write(&file, b"contents").expect("write file");
    let error = inventory(&file, Limits::default()).expect_err("file root is invalid");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}
