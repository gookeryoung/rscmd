# rscmd

English | [简体中文](README.zh-CN.md)

A Rust toolset for small-model invocation: **single entrypoint, stable JSON envelope, self-describing schema**.
All business logic lives on the Rust side, exposed through three call paths: CLI, Python bindings and an MCP stdio server.

- 20 built-in tools covering text, data, crypto, system and network
- GUI frontend does not intrude on the default build: `rscmd reshackw` launches the standalone `rscmd-reshackw` binary
- Versioned success / error envelopes (`rscmd-result/1` / `rscmd-error/1`) with fixed exit-code semantics
- `--json` is machine-readable first; human-readable output never pollutes the stdout contract
- Tool metadata and parameter schemas are generated from code; `rscmd schema --json` is self-describing
- The Python side only converts types and maps exceptions, never duplicating business logic

> Command shorthands are shown in parentheses, e.g. `rscmd doctor(d)`, `rscmd run(r)`.

## Installation

### Native binary

Requires Rust stable (`rust-version = 1.78`):

```bash
cargo build --release -p rscmd-cli
# Artifact: target/release/rscmd (rscmd.exe on Windows)

# Or install into the Cargo bin directory
cargo install --path crates/rscmd-cli
```

### Python package

Requires Python 3.10+. Released wheels can be installed directly:

```bash
pip install rscmd
```

From source in editable mode (reuses the native module in the repo):

```bash
uv venv .venv
uv pip install --python .venv/Scripts/python.exe maturin pytest   # Windows
# or: uv pip install --python .venv/bin/python maturin pytest     # Unix
maturin develop

# You can also register the repo as a global CLI
uv tool install --editable .
```

After any Rust-side change you must run `maturin develop` again, otherwise `_native` silently runs the old implementation;
`rscmd doctor(d)` and `cargo xtask py-fresh-check` check whether that artifact is stale.

## Quick start

```bash
rscmd doctor(d)                                  # environment self-check
rscmd list                                       # list tools (alias l)
rscmd run(r) case --mode snake HelloWorld        # recommended entrypoint: run + tool name
rscmd case HelloWorld                            # direct form (equivalent)
rscmd --json run case --mode snake HelloWorld    # machine-readable envelope
rscmd info(i) case --json                        # full metadata for a single tool
rscmd schema --json                              # all tools + builtin commands + global arg schema
rscmd version(v) --json                          # version info
```

Example output:

```bash
$ rscmd run case --mode snake HelloWorld
hello_world

$ rscmd --json run case --mode snake HelloWorld
{"schema":"rscmd-result/1","ok":true,"tool":"case","data":{"text":"hello_world"}}
```

Errors always write the error envelope to stderr:

```bash
$ rscmd --json run json_fmt '{'
{"schema":"rscmd-error/1","ok":false,"tool":"json_fmt","kind":"invalid_input",
 "code":"INVALID_JSON","message":"...","field":"json","expected":"...","hint":"...",
 "exit_code":3}
```

## Commands

### Builtin commands

| Command | Description |
|---|---|
| `rscmd run(r) <tool> [args]` | Run the given tool (recommended entrypoint) |
| `rscmd list(l)` | List available tools (`--for-llm` filters out GUI tools) |
| `rscmd info(i) <tool>` | Show metadata for a single tool |
| `rscmd schema` | Export metadata and parameter schemas for all tools |
| `rscmd version(v)` | Print version info |
| `rscmd doctor(d)` | Print environment self-check info |
| `rscmd completion <SHELL>` | Generate bash / zsh / fish / powershell completion scripts |
| `rscmd mcp` | Run as an MCP stdio server |

See [docs/commands.md](docs/commands.md) for the full list. Tool aliases (e.g. `casetool`) still work,
but help output and the `Tools:` group only show canonical names; aliases are listed in [docs/tools.md](docs/tools.md).

### Tools

| Domain | Tool | Description |
|---|---|---|
| text | `case` | Naming-style conversion (snake/camel/pascal/kebab) |
| text | `wc` | Count lines, words, characters and bytes |
| text | `regex` | Regex matching, group capture and replacement |
| data | `csv` | CSV <-> JSON conversion with automatic header and delimiter detection |
| data | `json_fmt` | Validate and format JSON; invalid input returns a self-correctable error |
| data | `stat` | Count, sum, mean, median and percentiles |
| data | `timestamp` | Unix seconds <-> RFC3339 with fixed offset and date arithmetic |
| data | `toml` | Validate TOML and convert to JSON / YAML |
| data | `yaml` | Validate YAML and convert to JSON / TOML |
| crypto | `hash` | Compute the SHA-256 of text or a file |
| crypto | `codec` | base64 / hex / URL text encode and decode |
| system | `archive` | Pack a directory into a zip (writes require `--confirm`) |
| system | `env` | Read-only environment variable lookup with name and prefix filters |
| system | `filedate` | Read or write file modification time (writes require `--confirm`) |
| system | `filesearch` | Search files by name glob and content substring |
| system | `path` | Path normalization, joining, relative/absolute checks |
| system | `reshack` | Inspect / export / replace Windows PE resources in place |
| system | `reshackw` | Launch the reshack GUI (standalone `rscmd-reshackw` binary) |
| system | `which` | Locate executables on PATH |
| net | `urlcheck` | Probe a URL's HTTP status, redirect chain and latency |

## Global arguments

| Argument | Description |
|---|---|
| `--json` | Emit results as a JSON envelope |
| `--quiet` | Suppress successful results in human-readable mode |
| `--verbose` / `-v` | Print tool name and timing trace to stderr |
| `--dry-run` | Rehearse only, produce no side effects |
| `--confirm` | Confirm an operation with side effects |
| `--workers <N>` | Parallelism; 0 means automatic |
| `--limit <N>` | Result cap; 0 means unlimited |
| `--timeout <SECONDS>` | Per-call timeout in seconds; 0 means unlimited |

Global arguments may appear before or after the subcommand. The full schema lives in [schema/global.json](schema/global.json),
and the reference in [docs/global-args.md](docs/global-args.md).

## GUI frontend

`rscmd reshackw` only locates and launches; the GUI itself is the standalone `rscmd-reshackw` binary
(in `crates/rscmd-gui`, excluded from the default CLI / wheel build):

```bash
cargo build -p rscmd-gui --features gui   # produces target/<profile>/rscmd-reshackw[.exe]
rscmd reshackw                            # open the window
rscmd reshackw app.exe                    # open with a file (current POC previews images only)
RSCMD_RESHACKW=/path/to/rscmd-reshackw rscmd reshackw   # explicit binary path
```

Lookup order: `RSCMD_RESHACKW` -> next to the `rscmd` executable -> `PATH` ->
`target/{debug,release}` in the source tree (walking up from cwd, `CARGO_TARGET_DIR` included).
The last two matter for Python / `uv tool` entries, where `current_exe` is not the Rust binary;
when nothing matches it returns `GUI_UNAVAILABLE` (exit code 1), lists part of the searched
directories and prints the build command.
The tool is marked `gui: true`: `list --for-llm` and MCP do not expose it, and it is `hidden` on the Python side.

## Output contract

- Success: `{"schema":"rscmd-result/1","ok":true,"tool":"<tool>","data":<result>}`
- Failure: `{"schema":"rscmd-error/1","ok":false,"tool":"<tool>","kind":"...","code":"...","message":"...","exit_code":<code>}`
  - Optional fields: `field` / `expected` / `hint`
- When `rscmd doctor(d)` finds a problem it emits a result envelope with `ok:false` and exits 5
- `rscmd list` emits `data` as `{"schema":"rscmd-tools/1","version":"...","tools":[...]}`
- `rscmd schema` emits `data` as `rscmd-schema/1` with `version` / `tools` / `builtins` / `global`
- Successful results go to stdout, errors go to stderr; `--verbose` writes to stderr only

Exit codes:

| Exit code | kind | Meaning |
|---|---|---|
| 0 | — | Success |
| 1 | `runtime` | Runtime failure (IO, network, timeout, lock, etc.) |
| 2 | `usage` | Usage error (unknown command, missing argument, unconfirmed write) |
| 3 | `invalid_input` | Invalid input (argument, JSON, encoding, regex, parse failure, etc.) |
| 4 | `not_found` | Target does not exist (tool name, etc.) |
| 5 | `check_failed` | A check command did not pass (e.g. `doctor`) |
| 130 | `interrupted` | User pressed Ctrl+C |

The mapping from error code to category lives in `ERROR_CODE_KINDS`; every new error code must be registered.

## Python bindings

```python
import rscmd

rscmd.__version__
tools = rscmd.tools()                                  # summaries of all tools
meta = rscmd.describe("case")                          # full metadata for a single tool
text = rscmd.invoke("case", {"text": "HelloWorld", "mode": "snake"})
envelope = rscmd.run_argv(["--json", "doctor"])        # invoke with CLI semantics
code, stdout, stderr = rscmd.cli_main(["version"])     # CLI-level entrypoint
```

Failures raise `rscmd.RscmdError`, with structured fields available via `.code` / `.kind` / `.exit_code` /
`.field` / `.expected` / `.hint`.

```python
try:
    rscmd.invoke("json_fmt", {"json": "{"})
except rscmd.RscmdError as err:
    assert err.code == "INVALID_JSON"
    assert err.kind == "invalid_input"
    assert err.exit_code == 3
```

## MCP and completions

```bash
# MCP stdio server: line-delimited JSON-RPC, supports initialize / ping / tools/list / tools/call
rscmd mcp

# shell completions
rscmd completion bash        > rscmd.bash
rscmd completion zsh         > _rscmd
rscmd completion fish        > rscmd.fish
rscmd completion powershell  > rscmd.ps1
```

MCP only exposes non-GUI tools that have a JSON entrypoint; `inputSchema` reuses the tool schema directly,
protocol version 2025-06-18. The Rust binary and the Python console script share the same stdio loop,
so `python -m rscmd mcp` also works inside a venv; `rscmd.cli_main` returns output in one shot and does not support this mode.

## Development

```bash
make help            # list all targets
make check-docs      # L0: generated docs + ADR validation
make check-fast      # L1: fmt + clippy + registry/arch/docs/schema/deny/machete
make flow            # L2: check-fast + cargo test
make check           # full: check-fast + Rust tests + pytest + mypy
make cov             # llvm-cov, 95% line coverage threshold
make gen-docs        # regenerate docs/*.md
make gen-schema      # regenerate schema/*.json
make wheel-smoke     # build the wheel, install into .venv and smoke test
make bench           # startup-time benchmark regression
make version-check   # tag / Cargo / wheel / CHANGELOG version consistency
make push            # push all remotes and tags (never creates tags)
```

The full gate also needs `cargo-nextest`, `cargo-llvm-cov`, `maturin`, `pytest` and `mypy`.
Gate tiers and commit conventions are in [AGENTS.md](AGENTS.md); the AI-driven development flow is in
[docs/AI_DRIVEN_DEV_FLOW.md](docs/AI_DRIVEN_DEV_FLOW.md).

### Generated artifacts

The following files are generated, **do not edit them by hand**:

- `docs/*.md`: `make gen-docs`
- `schema/*.json`: `make gen-schema`
- `python/rscmd/_native.*`: `maturin develop`

### Directory layout

```
crates/
  rscmd-cli        executable entrypoint (dispatch / signals / MCP)
  rscmd-core       scheduling, envelopes, errors, schema, builtin commands
  rscmd-py         PyO3 bindings
  rscmd-runtime    fs / net / proc / lock / cache / home runtime helpers
  rscmd-tools      tool aggregation and feature switches
  rscmd-tool-*     text / data / crypto / system / net tool domains
  xtask            build, generated artifacts and gate scripts
python/rscmd/      Python package and native boundary type declarations
schema/            generated JSON Schema
docs/              generated docs + ADR / requirements / analysis
tests/py/          Python contract tests
```

## Documentation

- [docs/README.md](docs/README.md) — documentation index (auto-generated)
- [docs/commands.md](docs/commands.md) — command reference
- [docs/tools.md](docs/tools.md) — tool metadata and aliases
- [docs/global-args.md](docs/global-args.md) — global arguments
- [docs/schemas.md](docs/schemas.md) — tool parameter schema summary
- [docs/adr/README.md](docs/adr/README.md) — architecture decision records
- [CHANGELOG.md](CHANGELOG.md) — changelog

## License

MIT
