use serde::Serialize;
use std::path::{Component, Path};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Coverage { Complete, Partial }

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind { File, Symlink, Other }

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus { Hashed, Skipped, Error }

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    SymlinkNotFollowed,
    FileSizeLimit,
    TotalSizeLimit,
    FileCountLimit,
    PathCountLimit,
    PathLengthLimit,
    PathBytesLimit,
    DepthLimit,
    DirectoryEntryLimit,
    Unreadable,
    UnstableFile,
    UnstableDirectory,
    NonRegularFile,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FormatBasis { MagicBytes, Utf8Prefix, Unknown }

#[derive(Clone, Debug, Serialize)]
pub struct Limits {
    pub max_files: usize,
    pub max_paths: usize,
    pub max_path_bytes: usize,
    pub max_total_path_bytes: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_depth: usize,
    pub max_entries_per_directory: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: 100_000,
            max_paths: 100_000,
            max_path_bytes: 1024,
            max_total_path_bytes: 16 * 1024 * 1024,
            max_file_bytes: 512 * 1024 * 1024,
            max_total_bytes: 4 * 1024 * 1024 * 1024,
            max_depth: 64,
            max_entries_per_directory: 100_000,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InventoryEntry {
    /// Root-relative path; unsafe path bytes are percent-encoded.
    pub path: String,
    pub kind: EntryKind,
    pub status: EntryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// A hint, not proof that the file is a valid artifact of this format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_basis: Option<FormatBasis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InventoryIssue {
    pub path: String,
    pub reason: Reason,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub paths_reported: usize,
    pub path_bytes_accounted: usize,
    pub files_hashed: usize,
    pub bytes_hashed: u64,
    /// Includes bytes read from files later rejected as unstable.
    pub bytes_read: u64,
    pub skipped_paths: usize,
    pub error_paths: usize,
    pub traversal_issues: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct InventoryReport {
    pub schema_version: &'static str,
    pub coverage: Coverage,
    pub policy: Limits,
    pub summary: Summary,
    pub entries: Vec<InventoryEntry>,
    pub issues: Vec<InventoryIssue>,
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn relative_path(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    encode_relative_path(relative)
}

pub(crate) fn encode_relative_path(path: &Path) -> String {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(encode_component(value)),
            _ => None,
        })
        .collect();
    if components.is_empty() { ".".to_owned() } else { components.join("/") }
}

#[cfg(unix)]
pub(crate) fn encode_component(value: &std::ffi::OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    percent_encode(value.as_bytes())
}

#[cfg(windows)]
fn encode_component(value: &std::ffi::OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    if let Some(text) = value.to_str() {
        return percent_encode(text.as_bytes());
    }
    let mut encoded = String::from("~w~");
    for unit in value.encode_wide() {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{unit:04x}");
    }
    encoded
}

#[cfg(not(any(unix, windows)))]
fn encode_component(value: &std::ffi::OsStr) -> String {
    percent_encode(value.to_string_lossy().as_bytes())
}

fn percent_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    for byte in bytes {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => output.push(char::from(*byte)),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(&mut output, "%{byte:02X}");
            }
        }
    }
    output
}

pub(crate) fn detect_format(prefix: &[u8]) -> (String, FormatBasis) {
    if prefix.is_empty() {
        return ("empty_file".to_owned(), FormatBasis::Unknown);
    }
    let signatures: &[(&[u8], &str)] = &[
        (b"\x7fELF", "elf"),
        (b"MZ", "pe_or_dos_mz"),
        (b"\0asm", "wasm"),
        (b"PK\x03\x04", "zip"),
        (b"PK\x05\x06", "zip"),
        (b"PK\x07\x08", "zip"),
        (b"!<arch>\n", "ar_archive"),
        (b"SQLite format 3\0", "sqlite"),
        (b"\x1f\x8b", "gzip"),
        (&[0xfe, 0xed, 0xfa, 0xce], "mach_o"),
        (&[0xce, 0xfa, 0xed, 0xfe], "mach_o"),
        (&[0xfe, 0xed, 0xfa, 0xcf], "mach_o"),
        (&[0xcf, 0xfa, 0xed, 0xfe], "mach_o"),
        (&[0xca, 0xfe, 0xba, 0xbe], "mach_o_fat_or_class"),
        (&[0xbe, 0xba, 0xfe, 0xca], "mach_o_fat_or_class"),
        (&[0xca, 0xfe, 0xba, 0xbf], "mach_o_fat_or_class"),
        (&[0xbf, 0xba, 0xfe, 0xca], "mach_o_fat_or_class"),
    ];
    if let Some((_, hint)) = signatures.iter().find(|(signature, _)| {
        prefix.len() >= signature.len() && prefix.starts_with(signature)
    }) {
        return ((*hint).to_owned(), FormatBasis::MagicBytes);
    }
    if prefix.starts_with(b"#!") { return ("script".to_owned(), FormatBasis::MagicBytes); }

    if let Ok(text) = std::str::from_utf8(prefix) {
        let trimmed = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return ("json_candidate".to_owned(), FormatBasis::Utf8Prefix);
        }
        if text.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t' | '\u{000c}')) {
            return ("utf8_text_candidate".to_owned(), FormatBasis::Utf8Prefix);
        }
    }
    ("unknown_binary".to_owned(), FormatBasis::Unknown)
}

pub(crate) fn same_file_version(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() { return false; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        // Size and modification time only; this fallback is not race-free.
        true
    }
}
