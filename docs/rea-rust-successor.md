# REA-RS: a Rust-native successor, bootstrapped by REA

**Status:** research plan plus an uncompiled Rust prototype; no compatibility or security-completeness claim  
**Baseline under study:** published `rea-agents@6.4.0` plus upstream repository snapshot `morluto/rea@4276aaf18bb607cdd0182280f76cf50fcd110c3c` (captured 2026-10-13); the npm tarball and source checkout are distinct inputs  
**Goal:** use the existing REA to learn from its shipped artifact and observable behavior, then incrementally implement a Rust-native CLI/MCP with evidence-based parity checks.

## Executive decision

Yes, use REA to investigate REA—but treat the result as evidence, not as an automatic TypeScript-to-Rust translation. The proper method is **self-analysis → contract extraction → independent Rust implementation → differential conformance → gradual default switch**.

The upstream repository is MIT-licensed; preserve upstream copyright/license notices for any copied code and inventory all transitive licenses before redistributing a port. A clean reimplementation of contracts and behavior is different from mechanically translating source files. The upstream repository is a layered workbench, not a single command wrapper. Its current architecture includes a shared domain/contracts layer, application workflows and investigation records, a provider registry/session router, CLI and MCP adapters, static artifact analyzers, browser/runtime capture, and adapters for engines such as Ghidra, Hopper and IDA. It also carries Python, Java, Swift, and native platform bridges. A complete capability-for-capability rewrite is therefore a multi-phase project.

## Define “pure Rust” precisely

Use three labels so we do not overstate what has been replaced:

1. **Rust-native core:** the CLI, MCP server, contracts, evidence model, static analyzers, process supervision, and Nix package are implemented in Rust. Node/npm are not needed to build or run the native product.
2. **Rust-controlled providers:** Rust owns the typed interface, capability checks, process lifetime, deadlines, cancellation, logs and evidence normalization, while optionally invoking caller-installed Ghidra/IDA/Hopper or other native tools. This is a practical portability target but is not “all analysis implemented in Rust.”
3. **Fully self-contained Rust analysis:** every promised analyzer and decompiler is implemented in Rust or shipped as a separately qualified Rust component. This is a much larger goal and must not be implied by the first two milestones.

Recommended target: reach **Rust-native core** first; retain optional, explicitly declared providers behind narrow contracts. Do not silently fall back to a different provider or present a degraded result as equivalent.

## Phase 0 — REA examines the shipped REA artifact

The purpose of this phase is to create a fixed, repeatable baseline before writing a replacement.

### Freeze identity and inputs

- Pin the published npm version, tarball integrity/hash, retrieval timestamp, and package metadata.
- Identify the upstream source commit/tag independently. Do not claim the npm tarball is byte-identical to a checkout of `main`.
- Record the exact host, runtime version, REA command line, environment allowlist, and any provider/tool versions used.
- Keep source-repository observations, shipped-package observations, and live-runtime observations as distinct evidence classes.

### Capture the same baseline on each run

- `rea --help` and the command catalog.
- `rea doctor --json` (read-only diagnostics).
- The running MCP server's advertised tools and input schemas, plus a protocol transcript for initialize/discovery and representative read-only calls.
- Package file manifest and per-file cryptographic hashes.
- Static analysis of the *released package* using REA's JavaScript/application workflow where that input is supported. First prove the exact input form is accepted; a failed or unsupported workflow is **blocked/unknown**, never a pass.
- Selected end-to-end outputs for deterministic fixture binaries and app artifacts.

REA's own guidance distinguishes shipped-artifact/runtime investigation from ordinary source architecture review. Use REA for the former and normal source review for the latter.

### First-pass self-audit recipe

The repository now carries a repeatable shell harness, `scripts/rea-self-audit.sh`. On Ubuntu with Node.js 26.10.0 and npm available, run it from the repository root:

\`\`\`sh
REA_SELF_AUDIT_DIR="$PWD/rea-self-audit-output" bash scripts/rea-self-audit.sh
\`\`\`

The harness does not run `rea setup`, connect/configure an agent, or allow npm lifecycle scripts. It records Node/npm versions, npm integrity metadata and a tarball SHA-256; validates tar member paths/types and caps archive/unpacked sizes; creates a per-file package manifest; captures `rea --version`, `--help`, `doctor --json`, `providers --json`, and `capabilities --json`; performs an ordered MCP initialize + paginated `tools/list` exchange; and runs the released CLI against the unpacked npm package. `doctor` is diagnostic and its exit code is preserved without treating absent optional providers as a harness pass.

The output directory contains the raw tarball, per-file package manifest, command stdout/stderr and exit codes, MCP transcript/catalog, and `summary.json`. `summary.json` records the source repository snapshot separately from the npm package identity, because those are not presumed byte-identical. An exit trap emits `evidence.sha256` for every evidence file present when the script exits, including partial artifacts; the checksum manifest excludes itself. Each CLI probe's stdout/stderr file is capped at 512 MiB; hitting that limit is a failed probe rather than an unbounded evidence artifact. In CI, the audit evidence directory is uploaded as a short-lived artifact even when the self-audit job is blocked. The self-analysis command has a 120-second timeout and a nonzero exit deliberately makes the audit job fail. Inspect the artifact to distinguish an unsupported input, a provider/environment limitation, an upstream error, and a genuine successful analysis.

`summary.json` uses `executed_unreviewed` only when the CLI probes, MCP protocol/catalog capture, and self-analysis all complete with zero exit status. This is **not** a claim of behavioral correctness or parity; a human/independent checker must review the recorded result and fixture findings. The harness's release identity is pinned independently of the upstream source revision. If the analyzer rejects an npm package as an application target, preserve that blocked result rather than silently swapping in a friendlier target.

## Baseline artifacts

Produce a machine-readable audit bundle, with no target secrets or ambient credentials:

- `baseline.json`: source/package identities, command and environment metadata.
- `manifest.json`: relative path, size, content digest, detected format and parser coverage for every admitted file.
- `mcp-catalog.json`: tool names, schema digests and declared capabilities.
- `observations/`: raw bounded outputs and protocol transcripts.
- `evidence-ledger.json`: claim, source class, artifact digest, evidence reference, limitations, and state (`observed`, `inferred`, `unknown`, `unavailable`, or `blocked`).
- `checksums.txt`: hashes for every bundle member.

Keep raw evidence immutable and put interpretations in separate records. Redact actual credentials and authentication material; do not discard caller-selected paths, digests, offsets, or non-secret diagnostics.

## Phase 1 — Rust contracts and process boundary

Create a separate Rust workspace for the successor rather than embedding a second implementation into the Nix meta-flake.

Proposed first crates (names are provisional):

- `rea-contracts`: versioned serializable request/result types; typed error and coverage states; stable evidence records.
- `rea-evidence`: digest-addressed artifacts, source provenance, evidence references, bounded diagnostics, and deterministic JSON.
- `rea-process`: owned subprocesses, explicit environment allowlists, deadlines, cancellation, private temporary roots, capped stdout/stderr, and cleanup receipts.
- `rea-core`: provider-neutral workflows and capability selection.
- `rea-cli`: CLI and structured output.
- `rea-mcp`: MCP stdio adapter reusing the same application workflows as the CLI.
- `rea-analyzers`: independent static analyzer crates added by capability, not one monolith.

Use the official Rust MCP SDK (`rmcp`) and a mature Rust CLI parser (for example, `clap`). Pin exact dependency versions and commit the lockfile once a Rust toolchain is available. Do not implement a custom MCP protocol unless a required protocol feature is demonstrably missing.

The contracts must describe **what was actually checked**: `complete`, `partial`, `unknown`, `unavailable`, and `blocked` are distinct outcomes. “Tool started” or “provider pinged” must never stand in for proof that the requested analysis capability succeeded.

## Phase 2 — first useful native analyzer

Begin with deterministic, read-only **artifact inventory and evidence capture**, then extend it into static application reconstruction:

1. Bounded filesystem traversal; preserve exact relative path, size, file identity where available, and SHA-256 digest.
2. Magic-byte format detection; never trust a filename extension as proof of format.
3. ELF/PE/Mach-O metadata and symbol/section/relocation inventory using a reviewed Rust object-format library.
4. WebAssembly module/custom-section/import/export inspection with a dedicated WASM parser.
5. JavaScript/TypeScript module graph and syntax-level facts with Tree-sitter grammars; represent imports, exports and Electron boundaries without executing target code.
6. ASAR/package inspection, source-map provenance and bundle-to-source comparison only when fixtures prove the parsing and coverage claims.

Candidate ecosystem building blocks include `object` or `goblin` for executable-format parsing, `wasmparser` for WebAssembly, `tree-sitter` with JavaScript/TypeScript grammars for syntax facts, `sha2` for hashing, `serde`/`serde_json` for stable data interchange, `clap` for CLI arguments, and `rmcp` for MCP. Treat these as candidates to evaluate and pin, not as a claim that the code is already integrated or tested.

Static inspection must be byte-only and execution-free by default. No loading the target into a runtime, no running target-controlled build hooks, and no implicit network requests. A separate, explicitly requested runtime-observation workflow can be added behind an opt-in capability and a guarded process boundary.

## Phase 3 — differential compatibility harness

Do not compare free-form prose or provider-specific pseudocode byte-for-byte. Compare normalized facts and explicit limitations.

For every shared fixture and admitted command:

- Run the same fixed input through upstream REA and the Rust implementation.
- Normalize paths and nondeterministic timestamps only where the schema explicitly says they are non-semantic; never normalize away missing evidence or changed artifact identity.
- Compare tool schemas, exit behavior, structured error categories, findings, coverage, digests and provider claims.
- Require every mismatch to be classified as: Rust defect, upstream behavior change, intentional schema/API change, environment-specific capability gap, or unresolved.
- Archive exact inputs, both outputs, normalizer version, test command, exit status and result digest.
- Maintain negative fixtures: truncation, malformed headers, cycles, duplicate names, huge declared lengths, invalid offsets, symlink loops, timeouts, child process failure and cancellation.

A match in the happy path is not sufficient. The conformance gate must prove fail-closed behavior, bounds, cancellation and truthful partial/unknown reporting.

## Phase 4 — provider parity, not provider imitation

Only after the contract and static core are stable, add optional adapters one by one:

- Ghidra, IDA and Hopper for deep native analysis.
- Browser/CDP and Playwright runtime capture.
- Managed-code metadata and IL inspection.
- Firmware, Android, EVM, source-map and other specialist workflows.

For each provider, advertise capabilities and side effects explicitly. Bind a selected provider to a target/session deterministically, validate the target identity, and do not silently switch engines. External tools remain user-provided unless the user explicitly approves a separately reviewed installer.

Some bridges may remain in the language/runtime required by their host (for example, Ghidra's Java scripting surface). If the controller and core are Rust-native, label the remaining bridge honestly; “no Node.js runtime” is a valuable milestone even before every bridge is rewritten.

## Current prototype: offline inventory

The research branch now contains `rust/rea-rs`, the first small Rust-native slice:

- Emits schema-versioned JSON with root-relative, percent-encoded paths.
- Hashes regular files with SHA-256 only when the full read fits within policy and pre/post metadata checks are consistent.
- Emits non-validating magic-byte/UTF-8 format hints for ELF, PE/DOS-MZ, Mach-O, WASM, ZIP, archives, SQLite, gzip, scripts, JSON candidates and text candidates.
- On Linux, uses Rustix descriptor-relative `statat`/`openat` with no-follow flags and retained parent directory handles; records symlinks without traversing them. File identity/modification metadata is checked before and after hashing.
- Applies defaults of 100,000 regular-file attempts and child paths, 1,024 bytes per encoded path, 16 MiB aggregate encoded path bytes, 512 MiB per file, 4 GiB total bytes read, depth 64, and 100,000 entries per directory. Hard ceilings prevent raising these above 250,000 files and child paths, 4,096 bytes per encoded path, 64 MiB aggregate encoded path bytes, 4 GiB per file, 16 GiB total read, depth 256 and 100,000 entries per directory.
- Includes unit tests for known SHA-256 output, deterministic sorting, byte/file/path limits, including discovery-time path-byte budgets, symlink non-following, non-UTF-8 paths, and format hints.

The Linux path received a security-hardening pass after review found a time-of-check/time-of-use weakness in the original path-based traversal. Descriptor-relative lookup now anchors child operations to an already-open directory handle, with explicit no-follow flags. This narrows symlink-swap risk substantially; stable-copy operation and CI verification remain required. The non-Linux portability fallback is not yet held to the same adversarial-filesystem threat model. The prototype is intentionally not wired into the Nix flake yet. It currently has exact pins for direct Cargo dependencies, but a checked-in `Cargo.lock` is still required for reproducible dependency resolution. CI was added to generate a per-run lock, run formatting/tests/Clippy/offline tests, and preserve the resolved lock as a short-lived artifact. The observed GitHub Actions run is queued; until step results are present, all tests remain **unverified**. Local `rustc`, `cargo`, and `nix` were unavailable in the work environment.

## Acceptance gates

A milestone is not complete until its evidence exists:

- [ ] Exact upstream npm tarball integrity and independent source revision recorded.
- [ ] Baseline bundle is reproducible from pinned inputs and contains its own checksums.
- [ ] Rust workspace builds with locked dependencies on the supported Nix systems.
- [ ] CLI and MCP share application workflows and return equivalent typed results.
- [ ] Every admitted command has deterministic fixtures, malformed-input tests and bounded-resource tests.
- [ ] Linux traversal tests prove directory-symlink targets are not inventoried and no-follow/FD-relative behavior passes CI.
- [ ] Differential reports preserve all mismatches; no “queued = pass” shortcut.
- [ ] Nix builds are offline/store-hermetic for Rust dependencies; runtime analysis performs no implicit package installation.
- [ ] Security review confirms read-only static analyzers do not execute the target and process adapters do not inherit ambient secrets.
- [ ] Capability matrix distinguishes implemented, partial, external-provider, unsupported and unverified behavior.

## Immediate next actions

1. Obtain actual CI step results for the current Rust slice; fix compilation, test, lint, and formatting failures before calling it a working baseline.
2. Commit the resolved `Cargo.lock` after reviewing its exact graph and dependency provenance.
3. Run the pinned REA 6.4.0 self-audit under supported Node 26, preserving the npm integrity record, tarball SHA-256, command outputs and exit statuses.
4. Freeze the resulting baseline bundle and grow the fixture corpus for known parser edge cases.
5. Add one real format parser (WASM or object files) behind the evidence contract and compare its outputs with independent tools/REA where the same facts are available.
6. Integrate a locked, offline Rust build into the Nix flake only after CI and `nix flake check` prove the package works.

The REA self-analysis recipe has not yet been executed in this environment, and the Rust prototype has not yet been compiled or tested here. No parity, security-completeness, or Nix hermeticity claim is made. These remain explicit acceptance gates.

## References

- Upstream project and CLI: https://github.com/morluto/rea
- Upstream architecture: https://github.com/morluto/rea/blob/main/docs/architecture.mermaid
- Upstream CLI and evidence: https://github.com/morluto/rea/blob/main/docs/cli.md
- Upstream provider evaluation: https://github.com/morluto/rea/blob/main/docs/provider-evaluation.md
- Published package baseline: https://www.npmjs.com/package/rea-agents
- Rust MCP SDK: https://docs.rs/rmcp/latest/rmcp/
- Tree-sitter Rust bindings: https://docs.rs/tree-sitter/latest/tree_sitter/
- Rust object parser: https://docs.rs/object/latest/object/
- WebAssembly parser: https://docs.rs/wasmparser/latest/wasmparser/\n- Rustix filesystem syscalls: https://docs.rs/rustix/1.1.5/rustix/fs/fn.openat.html
