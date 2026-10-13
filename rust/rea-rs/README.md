# REA-RS: first Rust-native slice

This crate is an independent implementation prototype for the staged Rust-native REA successor described in [the research plan](../../docs/rea-rust-successor.md). It does **not** claim feature parity with upstream REA.

## What it does

`rea-rs` performs offline, read-only inventory of a caller-selected directory. It records bounded SHA-256 digests and conservative format hints. It does not execute inspected files, install providers, contact a network, or edit agent configuration.

```sh
cargo run --manifest-path rust/rea-rs/Cargo.toml -- --root /path/to/unpacked/rea --max-files 100000
```

The command emits a versioned JSON report to stdout. Exit status `0` means the walk completed under the configured policy; exit status `2` means some paths were skipped, changed, unreadable, or exceeded limits; exit status `1` is a fatal invocation/root/output failure. Preserve the exit code instead of treating any JSON output as a pass.

## Default resource bounds

- 100,000 regular-file attempts
- 100,000 total child paths admitted to traversal (files, symlinks, special files and directories)
- 1,024 bytes maximum per percent-encoded relative path
- 16 MiB aggregate budget for admitted encoded path bytes
- 512 MiB maximum file size for hashing
- 4 GiB maximum bytes read across the run
- 64 directory levels below the selected root
- 100,000 directory entries; directories beyond this bound are skipped as a whole
- On Linux, traversal is anchored to open directory descriptors and uses Rustix `statat`/`openat` with no-follow flags; symlink entries are recorded and not traversed

CLI flags can lower or tune limits within hard ceilings of 250,000 file attempts and child paths, 4,096 bytes per encoded path, 64 MiB aggregate encoded path bytes, 4 GiB per file, 16 GiB total bytes read, depth 256, and 100,000 entries per directory. Requests above those ceilings are rejected. The Linux path uses descriptor-relative child lookups, so renaming a parent path or substituting a symlink cannot redirect nested lookups outside the already-open parent directory. It also verifies file identity and modification metadata before/after hashing. This is not a filesystem snapshot: concurrent modifications can still make the overall report partial, and metadata checks are not cryptographic proof of immutability. Run it on a stable copy when evidence integrity matters. Non-Linux builds use a portability fallback that has not had the same descriptor-relative hardening review; do not use that fallback against a hostile concurrently-mutated tree.

## Evidence semantics

- A SHA-256 digest is emitted only when the full file was read within limits and metadata checks remained consistent.
- Format detection is a **hint based on magic bytes or a short UTF-8 prefix**, not a parser validation or proof the file is well-formed.
- Symlinks, special files, errors, oversized files, and traversal caps make overall coverage `partial`; omitted analysis is never marked successful.
- Paths are relative to the supplied root; absolute local paths are not included in the report.
- Non-ASCII/unsafe path bytes are percent-encoded so reports do not require lossy UTF-8 conversion.

## Current verification status

The source includes unit tests, but this environment did not have `cargo` or `rustc` available. Until CI or a Rust-enabled host runs `cargo fmt --check`, `cargo test`, and `cargo clippy -- -D warnings`, treat this crate as **uncompiled prototype code**. Do not package it in the Nix flake yet; generate and commit `Cargo.lock`, verify an offline build, and record the exact toolchain first.
