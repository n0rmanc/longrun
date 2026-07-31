<!--
Sync Impact Report
- Version change: 2.0.0 -> 3.0.0
- Bump rationale: release-specific scope restrictions were replaced with the
  repository's enduring core values and complete target architecture. This
  changes the governed product boundary and is therefore a major amendment.
- Modified principles:
  - I. CLI-First Product Boundary -> I. Eliminate Model Polling
  - II. Local Wait, Same-Turn Continuation -> II. CLI Is the Product
  - III. At-Most-Once Execution -> III. Continue the Same Work
  - IV. Execution and Delivery Are Separate -> IV. Run Once, Deliver Safely
  - V. Fail-Closed Execution -> V. Preserve Security Boundaries
  - VI. Bounded, Untrusted Output -> VI. Keep Context Small and Evidence Local
- Product rename:
  - Previous product and executable names -> Longrun / longrun
- Added sections:
  - Target Architecture
  - Runtime Modes and Recovery
- Removed sections:
  - Release-specific scope section
- Follow-up TODOs: none
-->
# Longrun Constitution

## Core Principles

These principles express the repository's enduring values. Specifications and
implementations MUST optimize for them rather than for a particular release
milestone.

### I. Eliminate Model Polling
Longrun exists to run finite, long-running commands without periodic
model requests. Waiting MUST happen in local Rust processes, hooks, or IPC, not
through repeated `write_stdin`, status prompts, sleep loops, or agent turns.
Any execution path that reintroduces model polling violates the product's
primary value.

### II. CLI Is the Product
`longrun` MUST be a standalone, installable Rust CLI usable by humans,
scripts, CI, and coding agents. The Codex plugin, skills, hooks, and any future
agent integrations MUST remain thin adapters over the same CLI and runtime.
They MUST NOT duplicate job execution, persistence, security, or recovery
logic.

### III. Continue the Same Work
When the originating Codex process remains active, completion MUST return to
the same session and turn through the synchronous `PostToolUse` hook. Starting
a separate Codex process is not normal continuation. Recovery MAY resume an
unavailable session only after proving that no active hook still owns delivery.

### IV. Run Once, Deliver Safely
Every accepted job MUST execute at most once. Execution state and result
delivery state MUST remain separate so delivery can be retried without
re-executing the command. Receipts, leases, locks, and state transitions MUST
prevent forgery, replay, duplicate execution, duplicate resume, and concurrent
delivery ownership.

### V. Preserve Security Boundaries
A long-running command MUST NOT gain permissions merely because a hook or
supervisor launches it. Execution MUST be fail-closed under an explicit Codex
sandbox profile. Network access, workspace-external writes, secret inheritance,
shell evaluation, and danger-full-access require explicit user configuration;
the runtime MUST never escalate automatically.

### VI. Keep Context Small and Evidence Local
Complete stdout, stderr, specifications, and results MUST be retained locally.
Only bounded tails, exit metadata, duration, truncation state, and log paths
enter model context. Command output MUST be labeled and treated as untrusted
data rather than instructions. The system MUST preserve enough local evidence
to diagnose failures without spending model tokens on progress output.

## Target Architecture

The final product MUST consist of one Rust execution system with multiple thin
interfaces:

1. `longrun run -- PROGRAM ARG...` executes synchronously for humans,
   scripts, and CI and returns the child process exit code.
2. `longrun submit -- PROGRAM ARG...` creates an immutable job
   specification, emits only a versioned machine-readable receipt, and exits
   without starting the real command.
3. `longrun hook codex pre-tool-use` validates Longrun submissions and is
   a no-op for unrelated tool calls. It MUST NOT approve or rewrite general
   shell commands.
4. `longrun hook codex post-tool-use` validates and consumes the receipt,
   submits the job to the runtime, waits locally for completion, and returns a
   bounded result to Codex.
5. `longrun hook codex session-start` reports integration health and
   delivers eligible completed-but-undelivered results during recovery.
6. The CLI MUST expose job operations for waiting, status, listing, logs,
   cancellation, diagnostics, garbage collection, and integration lifecycle.
7. `longrun init --codex` MUST install generated Codex integration assets
   using the absolute binary path returned by `current_exe()`. A plugin MUST
   NOT attempt to install the CLI or depend on the caller's `PATH`.
8. A structured MCP interface MAY expose the same job operations, but it MUST
   use the same supervisor protocol and MUST NOT become a second execution
   backend.

The Rust runtime MUST be the sole authority for:

- versioned job specifications, receipts, hook messages, and result schemas;
- command execution, timeout, cancellation, and process-tree cleanup;
- atomic job and delivery state transitions;
- stdout and stderr logs plus bounded result generation;
- sandbox and environment policy;
- session delivery leases and crash recovery.

Direct program-and-argument execution is the default. Compound shell execution
MUST use an explicit shell-specific command. Native command arguments MUST
remain OS strings internally and MUST NOT be reconstructed through shell
interpolation.

Durable state MUST use transactional, crash-consistent storage with execution
and delivery modeled separately. The runtime MUST support process-tree ownership
appropriate to each supported platform, including process groups on Unix and
Job Objects on Windows.

## Runtime Modes and Recovery

The product MUST support two modes without changing its execution semantics:

- Embedded mode: the `PostToolUse` hook owns the child process and waits for it
  directly. Closing the originating Codex process ends that ownership.
- Durable mode: an explicitly installed per-user supervisor owns jobs and
  hooks wait for completion events over local IPC. Jobs survive Codex process
  termination.

Installing Codex integration MUST NOT silently install or start an operating
system service. Durable mode requires an explicit user action.

Completion delivery MUST follow this order:

1. Deliver through the original active `PostToolUse` hook.
2. If that owner is gone, deliver an undelivered result through
   `SessionStart`.
3. Use optional `codex exec resume` only when enabled by the user, the active
   lease has expired, a per-session lock is held, and the result remains
   undelivered.

Recovery MUST be idempotent and bounded. A recovery attempt MUST NOT execute the
job again, compete with an active hook, or start more than one resume process
for the same delivery.

## Quality Gates

Every change MUST pass `cargo fmt --check`, `cargo clippy -- -D warnings`, and
`cargo test`. Lifecycle, protocol, persistence, process, and security changes
MUST include the smallest automated test that fails when their contract breaks.

Every supported execution path MUST have evidence proving:

1. A submitted command starts at most once.
2. No periodic model turn occurs while the command runs.
3. The active-session path continues through the original hook.
4. Delivery retry never causes command re-execution.
5. Forged, replayed, expired, or mismatched receipts execute nothing.
6. Sandbox denial fails without automatic permission escalation.
7. Timeout and cancellation terminate the owned process tree.
8. Full logs remain local and model-visible output respects configured bounds.
9. Output and environment handling do not expose protected credentials.
10. Recovery cannot race an active hook or duplicate a resume.

Architecture changes MUST prefer deletion, the Rust standard library, and
existing dependencies before adding crates or abstractions. New execution
backends are prohibited; new interfaces MUST reuse the existing runtime.

## Governance

This constitution defines the repository's final product values and
architecture. Release scope, milestones, and implementation phases belong in
feature specifications and plans, not in this document.

This constitution governs all specifications, plans, tasks, code, integration
assets, and releases. Exceptions MUST be documented in the relevant plan with
their necessity, security impact, owner, and removal condition.

Amendments require a rationale, an updated Sync Impact Report, and a semantic
version change. MAJOR versions remove or redefine a core value or architecture
boundary, MINOR versions add or materially expand governance, and PATCH versions
clarify wording without changing obligations. Every implementation review MUST
map its changes and verification evidence to the applicable principles and
quality gates; unresolved violations block completion.

**Version**: 3.0.0 | **Ratified**: 2026-07-31 | **Last Amended**: 2026-07-31
