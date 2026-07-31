# Implementation Plan: Long-Running Command Execution

**Branch**: `main` | **Date**: 2026-07-31 |
**Spec**: [spec.md](./spec.md)

**Input**: Feature specification from
`/specs/001-long-command-execution/spec.md`

## Summary

Build Longrun as one installable Rust CLI with two user-facing execution paths:
`longrun run` blocks directly for humans and CI, while `longrun submit` emits a
single-use receipt for Codex. Thin Codex hooks validate submissions, hand the
real command to the same runtime, wait locally without model polling, and
return bounded results to the originating turn. The same runtime supports
embedded ownership and an explicitly installed durable supervisor, with
transactional state, local logs, process-tree cleanup, sandbox preservation,
and recovery delivery.

## Technical Context

**Language/Version**: Rust 2024 edition; MSRV Rust 1.85; development toolchain
Rust 1.97.0

**Primary Dependencies**: `clap` 4, `tokio` 1.49, `serde`, `serde_json`,
`toml`, `uuid`, `rusqlite` with bundled SQLite, `directories`, `thiserror`,
`anyhow`, `tracing`, `tracing-subscriber`, `base64`, `hmac`, `sha2`, and
`zeroize`; target-specific `nix` and `windows-sys`

**Storage**: SQLite in WAL mode for execution and delivery state; immutable JSON
job specifications and results; separate append-only stdout/stderr log files;
OS-specific per-user state and data directories

**Testing**: Rust unit and integration tests, CLI process tests, hook JSON
fixtures, supervisor IPC tests, process-tree tests, sandbox denial tests,
cross-platform compile checks, and live Codex hook tests

**Target Platform**: macOS 13+, Linux with Unix-domain sockets and Codex sandbox
support, Windows 11 with named pipes and Job Objects

**Project Type**: Single Rust CLI application with library modules, generated
Codex plugin assets, and an optional per-user supervisor mode

**Performance Goals**: `submit` returns within 250 ms p95; hook no-op within
50 ms p95; completion delivery begins within two seconds of process exit;
status lookup within 100 ms p95 for 10,000 retained jobs; bounded model output
defaults to 32 KiB

**Constraints**: Zero periodic model polling; at-most-once execution; retryable
exactly-once-effective delivery; no silent permission escalation; full logs
remain local; direct argv preserves native OS strings; all persistent changes
must be atomic and crash-consistent

**Scale/Scope**: One local user, up to 32 concurrent durable jobs, 10,000
retained job records, multi-hour commands, and logs bounded by configurable
retention rather than model context

## Constitution Check

*GATE: Passed before research and re-checked after design.*

| Principle | Design Evidence | Status |
|-----------|-----------------|--------|
| Eliminate Model Polling | Synchronous `PostToolUse` or local IPC wait owns the entire wait; no agent status loop | PASS |
| CLI Is the Product | One Rust binary and shared runtime; plugin, hooks, skill, and optional MCP are adapters | PASS |
| Continue the Same Work | Active execution returns via `PostToolUse`; recovery is ordered and lease-protected | PASS |
| Run Once, Deliver Safely | Transactional execution/delivery state, one-time receipt, lease, and idempotent recovery | PASS |
| Preserve Security Boundaries | Real command runs through `codex sandbox -P PROFILE -C CWD -- ...`; no auto-escalation | PASS |
| Keep Context Small and Evidence Local | Full local logs and a configurable bounded result envelope marked untrusted | PASS |
| Quality Gates | Format, lint, unit, integration, hook, live, and cross-platform checks are planned | PASS |

Post-design review confirms no constitution exception is required. The
supervisor, SQLite, platform modules, and optional MCP all serve requirements
explicitly mandated by the final architecture and do not create another
execution backend.

## Project Structure

### Documentation (this feature)

```text
specs/001-long-command-execution/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── cli.md
│   ├── hook-protocol.md
│   ├── ipc.schema.json
│   └── receipt.schema.json
├── checklists/
│   └── requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
Cargo.toml
Cargo.lock
src/
├── main.rs
├── cli.rs
├── config.rs
├── error.rs
├── paths.rs
├── protocol.rs
├── receipt.rs
├── store.rs
├── output.rs
├── runner.rs
├── supervisor.rs
├── ipc/
│   ├── mod.rs
│   ├── unix.rs
│   └── windows.rs
├── platform/
│   ├── mod.rs
│   ├── unix.rs
│   └── windows.rs
├── hook/
│   ├── mod.rs
│   ├── input.rs
│   ├── output.rs
│   ├── pre_tool_use.rs
│   ├── post_tool_use.rs
│   └── session_start.rs
└── integration/
    ├── mod.rs
    └── codex.rs
assets/
└── codex/
    ├── plugin.json
    ├── hooks.json
    ├── marketplace.json
    └── skills/
        └── longrun/
            └── SKILL.md
tests/
├── cli.rs
├── hooks.rs
├── receipts.rs
├── runner.rs
├── supervisor.rs
├── recovery.rs
├── integration_codex.rs
├── fixtures/
│   ├── hooks/
│   └── commands/
└── live/
    └── README.md
```

**Structure Decision**: Use one package and one binary. Modules separate
protocol, storage, process, hook, IPC, and installer responsibilities without
creating a workspace or one-crate-per-layer abstractions. Platform-specific
files are selected with `cfg` and share public contracts through their parent
modules.

## Design Decisions

### Submission and hook ownership

`PreToolUse` accepts only a direct invocation of the absolute installed
`longrun` executable with the `submit` subcommand. It rejects outer shell
composition, records a pending submission keyed by session, turn, tool use,
working directory, and exact command hash, then rewrites only that verified
wrapper call to add an opaque one-time hook token. The hook returns
`permissionDecision: "allow"` only for this non-executing wrapper invocation;
unrelated Bash calls remain untouched and follow normal approval behavior.

`submit` claims the one-time hook token, parses the requested argv, loads the
installation secret, creates an immutable specification and signed
`LONGRUN_RECEIPT_V1` envelope containing the hook context, then exits. It never
starts the requested command. `PostToolUse` accepts the envelope only when its
signature and every field match the pending hook record, consumes it
transactionally, and starts the real command. The HMAC covers the exact encoded
payload bytes, so verification never depends on JSON reserialization order.

The hook returns `continue: false` with `PostToolUse` additional context rather
than `decision: "block"`. Current Codex hook semantics replace the original
tool result and continue the model, while avoiding a rejected nested tool
promise in code-mode callers.

### Runtime authority

The runner is the only component allowed to spawn requested commands. Embedded
mode invokes it in the hook process. Durable mode invokes the same runner
through the supervisor. An MCP interface, if enabled, calls supervisor
operations only and cannot spawn commands independently.

### Sandboxing and environment

The runner executes the command as:

```text
codex sandbox -P <permission-profile> -C <cwd> -- <program> <args...>
```

This matches the installed Codex CLI 0.146.0 interface. Longrun does not use
the older platform-name subcommand form. The default profile is `:workspace`.
Danger-full-access requires both configuration permission and an explicit
per-command request.

The child environment starts from a safe allowlist. Secret-like names are
removed unless explicitly allowed. Arguments are retained as `OsString`;
`submit-shell` is a separate explicit interface.

### Persistence and output

SQLite WAL transactions protect state transitions and delivery leases.
Specifications and results are also written atomically as human-inspectable
JSON. Full stdout and stderr are streamed to separate files. The completion
envelope contains bounded byte tails, truncation flags, log paths, hashes, and
an explicit untrusted-output warning.

### Process ownership

On Unix, every job owns a process group. Cancellation sends a graceful signal,
waits the configured grace period, then kills the group. On Windows, every job
owns a Job Object configured to terminate descendants when ownership closes.
Timeout, cancellation, hook termination, and supervisor shutdown all use the
same cleanup path.

### Durable supervisor and recovery

Durable mode uses a per-user local supervisor with length-prefixed JSON
messages over a Unix-domain socket or Windows named pipe. Installation of the
Codex plugin does not install this service. `longrun service install` is the
only operation that creates a launchd user agent, systemd user unit, or Windows
per-user startup entry.

Delivery order is original hook, `SessionStart`, then optional
`codex exec resume`. Recovery requires an expired prior lease, a per-session
lock, an undelivered result, and an unspent retry budget. Marking delivery and
releasing ownership are atomic.

### Codex integration distribution

`longrun init --codex` resolves `current_exe()`, renders a local marketplace
and plugin under Longrun's data directory, and uses:

```text
codex plugin marketplace add <local-marketplace-root>
codex plugin add longrun@longrun-local
```

Generated hooks contain the absolute executable path. Users must review and
trust hooks in Codex. Repair is idempotent; uninstall calls
`codex plugin remove`, removes only Longrun's marketplace and generated files,
and leaves unrelated Codex configuration untouched.

## Complexity Tracking

No constitution violations require justification.
