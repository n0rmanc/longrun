<!--
Sync Impact Report
- Version change: 1.0.0 -> 2.0.0
- Bump rationale: the generic long-process rules were replaced by a concrete
  CLI-first product boundary, Codex hook protocol, delivery model, and phased
  architecture; this is a backward-incompatible governance redefinition.
- Modified principles:
  - I. Rust-First and Minimal -> I. CLI-First Product Boundary
  - II. Process Lifecycle Correctness -> II. Local Wait, Same-Turn Continuation
  - III. Explicit State and Recovery -> III. At-Most-Once Execution
  - IV. Lifecycle Tests Are Mandatory -> IV. Execution and Delivery Are Separate
  - V. Observable CLI Contracts -> V. Fail-Closed Execution
- Added principle:
  - VI. Bounded, Untrusted Output
- Added sections:
  - Product Architecture
  - MVP Scope
  - Quality Gates
- Removed sections:
  - Technical Constraints
  - Development Workflow
- Follow-up TODOs: none
-->
# Codex Longrun Constitution

## Core Principles

### I. CLI-First Product Boundary
`codex-longrun` MUST be a standalone, installable Rust CLI and the sole owner of
job execution, state, logs, receipts, and Codex hook processing. The Codex
plugin MUST contain only skills, hook definitions, manifests, and other thin
integration assets; it MUST NOT contain an independent execution backend.
Humans and CI MUST use `codex-longrun run`; Codex agents MUST use
`codex-longrun submit`.

### II. Local Wait, Same-Turn Continuation
A submitted command MUST cause no periodic model polling while it runs. The
synchronous Codex `PostToolUse` hook MUST wait locally for completion and return
the bounded result to the same active Codex session and turn. Normal execution
MUST NOT use `write_stdin` polling, sleep loops, detached shell jobs, or
`codex exec resume`. Resume is reserved for a future, explicit crash-recovery
path after active-hook ownership has ended.

### III. At-Most-Once Execution
Each accepted submission MUST execute its command at most once. `submit` MUST
create an immutable job specification and a verifiable, single-use receipt; it
MUST NOT start the real command. Before execution, the hook MUST bind the
receipt to the pending job, session, turn, tool use, working directory, command
hash, nonce, and expiry. Replayed, forged, expired, mismatched, or already
consumed receipts MUST fail without executing a command.

### IV. Execution and Delivery Are Separate
Job execution state and result-delivery state MUST be modeled independently.
Execution reaches exactly one terminal state: succeeded, failed, timed out, or
cancelled. Delivery MAY be retried, but it MUST never cause re-execution.
Only one active hook or recovery lease may own result delivery for a session.
State changes MUST be atomic and crash-consistent.

### V. Fail-Closed Execution
Long-running commands MUST execute under an explicit Codex sandbox permission
profile, defaulting to workspace access. The system MUST NOT silently enable
network access, workspace-external writes, secret inheritance, shell
evaluation, or danger-full-access. Direct program-and-argument execution is the
default; compound shell execution requires an explicit, separately named mode.
Cancellation and timeout MUST terminate the owned process tree, not only its
top-level process.

### VI. Bounded, Untrusted Output
Full stdout and stderr MUST remain in local per-job logs. Codex context receives
only the exit status, duration, log paths, truncation metadata, and bounded
tails. Command output MUST be labeled and handled as untrusted data, never as
agent instructions. Secrets MUST be excluded from inherited environments and
redacted from returned output wherever detection is possible.

## Product Architecture

The initial architecture MUST use one Rust binary and thin generated Codex
integration:

1. `codex-longrun run -- PROGRAM ARG...` executes synchronously for humans,
   scripts, and CI and returns the child exit code.
2. `codex-longrun submit -- PROGRAM ARG...` writes an immutable job request,
   emits only a machine-readable versioned receipt, and exits quickly.
3. The `PreToolUse` hook validates only Longrun submissions and MUST be a no-op
   for unrelated tool calls. It MUST NOT auto-approve or rewrite general shell
   commands.
4. The `PostToolUse` hook validates the receipt, starts the real sandboxed
   command, waits locally, stores complete logs, and returns a bounded result.
5. Hook adapters MUST deserialize stdin JSON, invoke Rust business logic, and
   serialize stdout JSON. Shell or Python MUST NOT contain job logic.
6. `codex-longrun init --codex` MUST generate the plugin, skill, and hooks using
   the absolute path from `current_exe()`. The plugin MUST NOT attempt to install
   the CLI or rely on the invoking shell's `PATH`.
7. The CLI MUST provide `doctor` and Codex integration uninstall or repair
   operations before the integration is considered distributable.

Versioned receipts, hook input, hook output, job specifications, and result
schemas are public compatibility contracts. Command arguments MUST remain
native OS strings internally and MUST NOT be reconstructed as an interpolated
shell command.

## MVP Scope

Version 0.1 MUST implement only the shortest architecture that proves the
product:

- one installable Rust binary;
- `run`, `submit`, `init --codex`, integration uninstall, and `doctor`;
- Codex skill plus `PreToolUse` and synchronous `PostToolUse` hooks;
- embedded process ownership with timeout and cleanup;
- workspace-sandboxed direct argv execution;
- separate stdout and stderr logs with bounded completion output;
- macOS and Linux support;
- no periodic model polling.

Version 0.1 MUST NOT add a daemon, operating-system service, SQLite, MCP server,
automatic session resume, Windows support, telemetry, command auto-detection,
or a second execution backend. These features require a demonstrated need and a
new specification. Any later MCP interface MUST reuse the same supervisor and
MUST NOT execute commands independently.

## Quality Gates

Every change MUST pass `cargo fmt --check`, `cargo clippy -- -D warnings`, and
`cargo test`. Non-trivial lifecycle or security behavior MUST include the
smallest automated check that would fail on regression.

Before version 0.1 is complete, end-to-end evidence MUST prove all of the
following:

1. A submitted finite command starts exactly once.
2. No `write_stdin` or periodic model turn occurs while it runs.
3. Completion continues the same active Codex session and turn.
4. Exit code and bounded stdout and stderr results are delivered once.
5. Forged, replayed, or mismatched receipts execute nothing.
6. Sandbox denial fails without permission escalation.
7. Timeout and cancellation clean up the owned process tree.
8. Full logs remain local and model-visible output stays within its configured
   byte limit.

Architecture changes MUST prefer deletion, the Rust standard library, and
already selected dependencies before adding new crates or abstractions.

## Governance

This constitution governs all specifications, plans, tasks, code, integration
assets, and releases. Exceptions MUST be documented in the relevant plan with
their necessity, security impact, owner, and removal condition.

Amendments require a rationale, an updated Sync Impact Report, and a semantic
version change. MAJOR versions remove or redefine a principle or product
boundary, MINOR versions add or materially expand governance, and PATCH versions
clarify wording without changing obligations. Every implementation review MUST
map its changes and verification evidence to the applicable principles and
quality gates; unresolved violations block completion.

**Version**: 2.0.0 | **Ratified**: 2026-07-31 | **Last Amended**: 2026-07-31
