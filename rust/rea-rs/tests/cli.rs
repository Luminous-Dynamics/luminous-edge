use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rea-rs-cli-test-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create unique test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_cli(root: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rea-rs"))
        .arg("--root")
        .arg(root)
        .output()
        .expect("execute rea-rs CLI")
}

fn parse_report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("CLI should emit JSON report")
}

#[test]
fn complete_inventory_emits_json_and_exit_zero() {
    let root = TestDirectory::new();
    fs::write(root.0.join("artifact.txt"), b"abc").expect("write artifact");

    let output = run_cli(&root.0);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let report = parse_report(&output);
    assert_eq!(report["schema_version"], "rea-rs-inventory/v0.1");
    assert_eq!(report["coverage"], "complete");
    assert_eq!(report["summary"]["files_hashed"], 1);
    assert_eq!(
        report["entries"][0]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[cfg(unix)]
#[test]
fn skipped_symlink_emits_partial_json_and_exit_two() {
    use std::os::unix::fs::symlink;

    let root = TestDirectory::new();
    let outside = TestDirectory::new();
    fs::write(outside.0.join("outside.txt"), b"not part of root").expect("write outside artifact");
    symlink(outside.0.join("outside.txt"), root.0.join("link.txt")).expect("create link");

    let output = run_cli(&root.0);
    assert_eq!(output.status.code(), Some(2), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let report = parse_report(&output);
    assert_eq!(report["coverage"], "partial");
    assert_eq!(report["summary"]["bytes_read"], 0);
}

#[test]
fn invalid_root_emits_no_json_and_exit_one() {
    let root = TestDirectory::new();
    let path = root.0.join("not-a-directory");
    fs::write(&path, b"file").expect("write file root");

    let output = run_cli(&path);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot inventory selected root"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
