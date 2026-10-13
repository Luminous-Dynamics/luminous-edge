#!/usr/bin/env bash
# Bounded read-only experiment against the exact published REA package.
set -uo pipefail

VERSION="6.4.0"
SOURCE_COMMIT="4276aaf18bb607cdd0182280f76cf50fcd110c3c"
AUDIT_DIR="${REA_SELF_AUDIT_DIR:-$PWD/rea-self-audit-output}"
if [[ -e "$AUDIT_DIR" || -L "$AUDIT_DIR" ]]; then
  echo "Refusing to overwrite an existing audit output path: $AUDIT_DIR" >&2
  exit 73
fi
mkdir -p "$(dirname "$AUDIT_DIR")" || exit 1
mkdir "$AUDIT_DIR" || exit 1

# Preserve the audit's original exit status while checksumming all evidence,
# including partial evidence produced by a blocked or failed probe.
write_evidence_checksums() {
  local rc=$?
  trap - EXIT
  if [[ -d "$AUDIT_DIR" && ! -f "$AUDIT_DIR/summary.json" ]]; then
    python3 - "$AUDIT_DIR" "$rc" "$VERSION" "$SOURCE_COMMIT" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
try:
    exit_code = int(sys.argv[2])
except ValueError:
    exit_code = 1
version, source_commit = sys.argv[3], sys.argv[4]
summary = {
    "schema_version": "rea-self-audit/v1",
    "outcome": "blocked",
    "classification": "harness_terminated_before_complete_summary",
    "harness_exit_code": exit_code,
    "published_package": {"name": "rea-agents", "version": version},
    "comparison_source_snapshot": {
        "repository": "https://github.com/morluto/rea",
        "commit": source_commit,
        "note": "Source and published package are independent inputs.",
    },
}
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
PY
  fi
  if [[ -d "$AUDIT_DIR" ]]; then
    python3 - "$AUDIT_DIR" <<'PY'
import hashlib
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
manifest = root / "evidence.sha256"
files = sorted(
    (path for path in root.rglob("*") if path.is_file() and path != manifest),
    key=lambda path: path.relative_to(root).as_posix(),
)
with manifest.open("w", encoding="utf-8", newline="\n") as output:
    for path in files:
        digest = hashlib.sha256()
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
        output.write(f"{digest.hexdigest()}  {path.relative_to(root).as_posix()}\n")
PY
    if [[ $? -ne 0 ]]; then
      echo "Could not finalize evidence.sha256" >&2
    fi
  fi
  exit "$rc"
}
trap write_evidence_checksums EXIT

# Never run npm lifecycle scripts while acquiring or executing this release.
export npm_config_cache="${TMPDIR:-/tmp}/rea-self-audit-npm-cache-${BASHPID}"
export npm_config_ignore_scripts=true
node --version > "$AUDIT_DIR/node-version.txt" 2>&1
npm --version > "$AUDIT_DIR/npm-version.txt" 2>&1
printf '%s\n' "$VERSION" > "$AUDIT_DIR/rea-package-version.txt"

if npm pack --ignore-scripts --json "rea-agents@${VERSION}" \
  --pack-destination "$AUDIT_DIR" > "$AUDIT_DIR/npm-pack.json" 2> "$AUDIT_DIR/npm-pack.stderr"; then
  printf '0\n' > "$AUDIT_DIR/npm-pack.exit"
else
  code=$?
  printf '%s\n' "$code" > "$AUDIT_DIR/npm-pack.exit"
  echo "npm pack failed; inspect the preserved log and exit status." >&2
  exit 1
fi

tarball_name="$(python3 - "$AUDIT_DIR/npm-pack.json" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    value = json.load(f)
if not isinstance(value, list) or len(value) != 1 or not value[0].get("filename"):
    raise SystemExit("npm pack metadata does not identify exactly one tarball")
print(value[0]["filename"])
PY
)" || exit 1
TARBALL="$AUDIT_DIR/$tarball_name"
if [[ ! -f "$TARBALL" ]]; then
  echo "npm pack reported a tarball that is not present." >&2
  exit 1
fi
tarball_bytes="$(wc -c < "$TARBALL")"
if (( tarball_bytes > 524288000 )); then
  echo "Published tarball exceeds 500 MiB audit limit." >&2
  exit 1
fi
sha256sum "$TARBALL" | cut -d ' ' -f1 > "$AUDIT_DIR/tarball.sha256"

mkdir -p "$AUDIT_DIR/unpacked"
if ! python3 - "$TARBALL" "$AUDIT_DIR/unpacked" <<'PY'
import sys, tarfile
from pathlib import PurePosixPath

archive_path, destination = sys.argv[1:3]
with tarfile.open(archive_path, "r:gz") as archive:
    members = archive.getmembers()
    if len(members) > 100_000:
        raise SystemExit("refusing more than 100,000 tar members")
    total = 0
    for member in members:
        path = PurePosixPath(member.name)
        if path.is_absolute() or not path.parts or path.parts[0] != "package":
            raise SystemExit(f"unexpected tar member path: {member.name!r}")
        if any(part in (".", "..") for part in path.parts):
            raise SystemExit(f"path traversal in tar member: {member.name!r}")
        if member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
            raise SystemExit(f"refusing link or special tar member: {member.name!r}")
        if member.size < 0:
            raise SystemExit(f"negative tar member size: {member.name!r}")
        total += member.size
        if total > 1073741824:
            raise SystemExit("refusing more than 1 GiB of unpacked package content")
    archive.extractall(destination, members=members, filter="data")
PY
then
  printf '1\n' > "$AUDIT_DIR/extract.exit"
  echo "Tarball extraction was rejected or failed." >&2
  exit 1
fi
printf '0\n' > "$AUDIT_DIR/extract.exit"
PACKAGE_DIR="$AUDIT_DIR/unpacked/package"
if [[ ! -d "$PACKAGE_DIR" ]]; then
  echo "Tarball did not produce package/." >&2
  exit 1
fi

python3 - "$PACKAGE_DIR" "$AUDIT_DIR/package-manifest.json" <<'PY'
import hashlib, json, os, sys
root, output = sys.argv[1:3]
records = []
for current, dirs, files in os.walk(root, followlinks=False):
    dirs[:] = sorted(d for d in dirs if not os.path.islink(os.path.join(current, d)))
    for name in sorted(files):
        full = os.path.join(current, name)
        if os.path.islink(full) or not os.path.isfile(full):
            raise SystemExit(f"link or special file in package: {name!r}")
        digest, size = hashlib.sha256(), 0
        with open(full, "rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                size += len(chunk)
                digest.update(chunk)
        records.append({"path": os.path.relpath(full, root).replace(os.sep, "/"),
                        "size_bytes": size, "sha256": digest.hexdigest()})
records.sort(key=lambda item: item["path"])
with open(output, "w", encoding="utf-8") as out:
    json.dump({"schema_version": "rea-package-manifest/v1", "files": records}, out, indent=2)
    out.write("\n")
PY

REA=(npm exec --yes --package="rea-agents@${VERSION}" -- rea)
run_probe() {
  local name="$1"
  local timeout_seconds="$2"
  shift 2
  if (ulimit -f 524288; timeout "${timeout_seconds}s" "$@" > "$AUDIT_DIR/$name.stdout" 2> "$AUDIT_DIR/$name.stderr"); then
    printf '0\n' > "$AUDIT_DIR/$name.exit"
  else
    local code=$?
    printf '%s\n' "$code" > "$AUDIT_DIR/$name.exit"
  fi
}
run_probe version 30 "${REA[@]}" --version
run_probe help 30 "${REA[@]}" --help
run_probe doctor 90 "${REA[@]}" doctor --json
run_probe providers 30 "${REA[@]}" providers --json
run_probe capabilities 30 "${REA[@]}" capabilities --json

# A small protocol client waits for initialize before sending notifications and
# pages tools/list. It bounds time, transcript size, and the number of pages.
if python3 - "$VERSION" "$AUDIT_DIR" <<'PY'
import asyncio
import json
import os
import pathlib
import signal
import sys

VERSION, audit_dir = sys.argv[1], pathlib.Path(sys.argv[2])
TRANSCRIPT = audit_dir / "mcp-transcript.jsonl"
STDERR = audit_dir / "mcp-stderr.txt"
CATALOG = audit_dir / "mcp-tools.json"
MAX_TRANSCRIPT_BYTES = 64 * 1024 * 1024
MAX_STDERR_BYTES = 4 * 1024 * 1024
MAX_PAGES = 32
TIMEOUT_SECONDS = 15

async def main():
    env = dict(__import__("os").environ)
    proc = await asyncio.create_subprocess_exec(
        "npm", "exec", "--yes", f"--package=rea-agents@{VERSION}", "--", "rea", "mcp",
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env=env,
        limit=MAX_TRANSCRIPT_BYTES + 1,
        start_new_session=True,
    )
    stderr_size = 0

    async def terminate_process_group():
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            await asyncio.wait_for(proc.wait(), 5)
        except asyncio.TimeoutError:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            await proc.wait()
        try:
            await asyncio.wait_for(asyncio.shield(stderr_task), 3)
        except asyncio.TimeoutError:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            if proc.returncode is None:
                await proc.wait()
            stderr_task.cancel()
            await asyncio.gather(stderr_task, return_exceptions=True)

    async def drain_stderr():
        nonlocal stderr_size
        with STDERR.open("wb") as output:
            while True:
                chunk = await proc.stderr.read(65536)
                if not chunk:
                    break
                remaining = MAX_STDERR_BYTES - stderr_size
                if remaining > 0:
                    output.write(chunk[:remaining])
                    stderr_size += min(len(chunk), remaining)

    stderr_task = asyncio.create_task(drain_stderr())
    transcript_size = 0
    message_count = 0

    async def receive(expected_id):
        nonlocal transcript_size, message_count
        while True:
            line = await asyncio.wait_for(proc.stdout.readline(), TIMEOUT_SECONDS)
            if not line:
                raise RuntimeError(f"MCP server closed stdout before response id={expected_id}")
            transcript_size += len(line)
            if transcript_size > MAX_TRANSCRIPT_BYTES:
                raise RuntimeError("MCP transcript exceeded 64 MiB")
            message_count += 1
            with TRANSCRIPT.open("ab") as output:
                output.write(line)
            try:
                message = json.loads(line)
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise RuntimeError(f"MCP stdout is not JSON-RPC JSONL: {error}") from error
            if not isinstance(message, dict):
                raise RuntimeError("MCP stdout message is not an object")
            if message.get("id") == expected_id:
                if "error" in message:
                    raise RuntimeError(f"MCP returned an error for request {expected_id}")
                if "result" not in message:
                    raise RuntimeError(f"MCP response {expected_id} has no result")
                return message["result"]

    try:
        initialize = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "rea-self-audit", "version": "0.1.0"},
            },
        }
        proc.stdin.write((json.dumps(initialize) + "\n").encode())
        await proc.stdin.drain()
        init_result = await receive(1)
        if not isinstance(init_result.get("protocolVersion"), str):
            raise RuntimeError("MCP initialize response omitted protocolVersion")

        proc.stdin.write((json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        }) + "\n").encode())
        await proc.stdin.drain()

        all_tools = []
        cursor = None
        seen_cursors = set()
        for request_id in range(2, 2 + MAX_PAGES):
            params = {} if cursor is None else {"cursor": cursor}
            request = {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "tools/list",
                "params": params,
            }
            proc.stdin.write((json.dumps(request) + "\n").encode())
            await proc.stdin.drain()
            result = await receive(request_id)
            page = result.get("tools")
            if not isinstance(page, list):
                raise RuntimeError("MCP tools/list response omitted a tools array")
            all_tools.extend(page)
            if len(all_tools) > 20_000:
                raise RuntimeError("MCP catalog exceeded 20,000 tools")
            cursor = result.get("nextCursor")
            if cursor is None:
                break
            if not isinstance(cursor, str) or cursor in seen_cursors:
                raise RuntimeError("MCP tools/list returned an invalid or repeated cursor")
            seen_cursors.add(cursor)
        else:
            raise RuntimeError("MCP tools/list exceeded the 32-page audit limit")

        CATALOG.write_text(json.dumps({
            "schema_version": "rea-mcp-catalog/v1",
            "protocol_version": init_result["protocolVersion"],
            "server_info": init_result.get("serverInfo"),
            "tool_count": len(all_tools),
            "tools": all_tools,
        }, indent=2) + "\n", encoding="utf-8")

        proc.stdin.close()
        await proc.stdin.wait_closed()
        try:
            return_code = await asyncio.wait_for(proc.wait(), 20)
        except asyncio.TimeoutError:
            await terminate_process_group()
            raise RuntimeError("MCP server did not exit after stdin closed")
        if return_code != 0:
            raise RuntimeError(f"MCP server exited with status {return_code}")
        try:
            await asyncio.wait_for(asyncio.shield(stderr_task), 5)
        except asyncio.TimeoutError:
            await terminate_process_group()
            raise RuntimeError("MCP server left a child holding the stderr pipe open")
        summary = {
            "exit_code": 0,
            "initialize_response_present": True,
            "tools_list_response_present": True,
            "protocol_version": init_result["protocolVersion"],
            "tool_count": len(all_tools),
            "transcript_messages": message_count,
            "transcript_bytes": transcript_size,
            "stderr_bytes_recorded": stderr_size,
            "catalog_file": CATALOG.name,
        }
        (audit_dir / "mcp-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps(summary, indent=2))
        return 0
    except Exception as error:
        await terminate_process_group()
        (audit_dir / "mcp-summary.json").write_text(json.dumps({
            "exit_code": 2,
            "classification": "blocked_or_failed",
            "error": str(error),
            "transcript_messages": message_count,
            "transcript_bytes": transcript_size,
        }, indent=2) + "\n")
        print(f"MCP self-audit blocked: {error}", file=sys.stderr)
        return 2

try:
    exit_code = asyncio.run(main())
except Exception as error:
    print(f"MCP harness failed: {error}", file=sys.stderr)
    exit_code = 2
raise SystemExit(exit_code)
PY
then
  printf '0\n' > "$AUDIT_DIR/mcp.exit"
else
  code=$?
  printf '%s\n' "$code" > "$AUDIT_DIR/mcp.exit"
fi

# This is the self-analysis experiment. Nonzero means blocked/failed, never pass.
run_probe self-analysis 120 "${REA[@]}" analyze-javascript-application "$PACKAGE_DIR" --json

python3 - "$AUDIT_DIR" "$VERSION" "$SOURCE_COMMIT" <<'PY'
import hashlib, json, pathlib, sys
root, version, source_commit = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]

def exit_code(name):
    try:
        return int((root / f"{name}.exit").read_text(encoding="utf-8").strip())
    except (OSError, ValueError):
        return None

try:
    mcp_summary = json.loads((root / "mcp-summary.json").read_text(encoding="utf-8"))
except (OSError, ValueError):
    mcp_summary = {}
try:
    mcp_catalog = json.loads((root / "mcp-tools.json").read_text(encoding="utf-8"))
except (OSError, ValueError):
    mcp_catalog = {}
tools = mcp_catalog.get("tools")
if not isinstance(tools, list):
    tools = None

try:
    pack = json.loads((root / "npm-pack.json").read_text(encoding="utf-8"))[0]
except (OSError, ValueError, IndexError, TypeError):
    pack = {}
try:
    manifest = json.loads((root / "package-manifest.json").read_text(encoding="utf-8"))
except (OSError, ValueError):
    manifest = {"files": []}
try:
    tarball_hash = (root / "tarball.sha256").read_text(encoding="utf-8").split()[0]
except (OSError, IndexError):
    tarball_hash = None

names = ["version", "help", "doctor", "providers", "capabilities", "mcp", "self-analysis"]
probes = {}
for name in names:
    code = exit_code(name)
    output = root / f"{name}.stdout"
    probes[name] = {
        "exit_code": code,
        "stdout_bytes": output.stat().st_size if output.exists() else (
            (root / "mcp-transcript.jsonl").stat().st_size
            if name == "mcp" and (root / "mcp-transcript.jsonl").exists() else None
        ),
    }
base_cli_ok = all(exit_code(name) == 0 for name in ("version", "help", "providers", "capabilities"))
mcp_ok = (
    exit_code("mcp") == 0
    and mcp_summary.get("initialize_response_present") is True
    and mcp_summary.get("tools_list_response_present") is True
    and tools is not None
)
analysis_code = exit_code("self-analysis")
outcome = "executed_unreviewed" if base_cli_ok and mcp_ok and analysis_code == 0 else "blocked"

summary = {
    "schema_version": "rea-self-audit/v1",
    "comparison_source_snapshot": {
        "repository": "https://github.com/morluto/rea",
        "commit": source_commit,
        "note": "Source snapshot is tracked independently and is not asserted to be byte-identical to the published npm tarball.",
    },
    "published_package": {
        "name": "rea-agents",
        "version": version,
        "npm_integrity": pack.get("integrity"),
        "tarball_sha256": tarball_hash,
    },
    "runtime": {
        "node": (root / "node-version.txt").read_text(encoding="utf-8").strip(),
        "npm": (root / "npm-version.txt").read_text(encoding="utf-8").strip(),
    },
    "package_manifest": {
        "file_count": len(manifest.get("files", [])),
        "sha256": hashlib.sha256((root / "package-manifest.json").read_bytes()).hexdigest(),
    },
    "probes": probes,
    "mcp": {
        "initialize_response_present": mcp_summary.get("initialize_response_present") is True,
        "tools_list_response_present": mcp_summary.get("tools_list_response_present") is True,
        "protocol_version": mcp_summary.get("protocol_version"),
        "tool_count": len(tools) if tools is not None else None,
        "tools_sha256": hashlib.sha256(json.dumps(
            tools, sort_keys=True, separators=(",", ":")
        ).encode()).hexdigest() if tools is not None else None,
        "error": mcp_summary.get("error"),
    },
    "self_analysis": {
        "exit_code": analysis_code,
        "classification": "executed_unreviewed" if analysis_code == 0 else "blocked_or_failed",
    },
    "outcome": outcome,
}
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"outcome": outcome, "package_file_count": len(manifest.get("files", [])),
                  "mcp_tool_count": len(tools) if tools is not None else None,
                  "self_analysis_exit": analysis_code}, indent=2))
if outcome != "executed_unreviewed":
    raise SystemExit(2)
PY
