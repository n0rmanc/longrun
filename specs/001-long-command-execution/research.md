# Research: Long-Running Command Execution

> Historical research. Decisions about Longrun-owned sandboxing, environment
> filtering, workers, and durable recovery are superseded by the direct
> Codex-hook execution boundary documented in `specs/002-ephemeral-wait-proxy/`.

## Decision 1: Use synchronous Codex hooks for active-session continuation

**Decision**: Match Codex `Bash` calls with `PreToolUse` and `PostToolUse`.
Keep the long wait inside the synchronous `PostToolUse` command. Return
`continue: false` plus `hookSpecificOutput.additionalContext` containing the
bounded result.

**Rationale**: Current Codex hooks run synchronously; command-hook `async` is
parsed but unsupported. `PostToolUse` can replace the model-visible result and
continue the turn. This creates the required local wait without periodic model
requests. `continue: false` avoids rejecting nested tool promises while still
replacing the original result.

**Alternatives considered**:

- `write_stdin` long polling: still requires model turns and does not satisfy
  the primary outcome.
- `decision: "block"`: works for normal turns but rejects nested code-mode tool
  promises.
- `Stop` hook: adds a later continuation path and is less direct than
  `PostToolUse`.
- `codex exec resume`: starts a separate process and is recovery, not normal
  continuation.

**Source**: Current Codex manual sections "Hooks", "PreToolUse", and
"PostToolUse"; verified against Codex CLI 0.146.0 on 2026-07-31.

## Decision 2: Keep submit non-executing and bind it to hook context

**Decision**: `PreToolUse` records the exact tool context and command hash,
creates the claimed pending submission, signs the versioned receipt, and
retains it in trusted local state before the sandboxed tool command runs. It
rewrites only the verified non-executing Longrun wrapper call with hook-owned
token and short receipt-handle fields. The sandboxed receipt-only `longrun
submit` echoes that handle without accessing private Longrun state.
`PostToolUse` verifies the handle, then the stored signature and all receipt
fields against the pending record before atomically consuming it and starting
the command.

**Rationale**: Starting a detached command from the short Bash invocation makes
process survival, sandbox inheritance, cancellation, and duplicate prevention
platform-dependent. Hook-owned validation gives one authoritative transition
from submission to execution.

**Alternatives considered**:

- Detached child from `submit`: unclear lifetime and duplicated ownership.
- Trusting stdout marker alone: forgeable by shell composition or a different
  executable.
- Correlating by command hash alone: concurrent identical submissions can claim
  each other's pending context.
- Depending on undocumented session environment variables: current local
  command environments expose no session, turn, or tool-use identifiers.

The targeted rewrite is acceptable because the approved wrapper cannot execute
the requested command; it only emits a receipt. The real command is still
subject to Longrun's explicit fail-closed sandbox policy.

The installation HMAC is defense in depth against forged or mixed tool output;
the hard authorization boundary is the one-time pending hook context, exact
absolute executable, exact command comparison, and transactional consumption.
It is not treated as protection from arbitrary malicious code already running
as the same local user.

## Decision 3: Validate the absolute executable and reject outer shell syntax

**Decision**: Session integration teaches the absolute Longrun executable path.
`PreToolUse` accepts only that path followed by `submit` and a direct argv
payload, or explicit `submit-shell` when shell mode is configured. Pipes,
redirections, separators, backgrounding, command substitution, and shell
wrappers are rejected. It safely reconstructs the verified invocation with
hook-owned token and short receipt-handle fields; the retained signed receipt
never enters the sandbox, which never accesses private Longrun state.

**Rationale**: A receipt is trustworthy only if the hook knows which binary
produced it and the wrapper invocation cannot append forged output.

**Alternatives considered**:

- Resolve `longrun` through `PATH`: vulnerable to path shadowing and desktop
  environment differences.
- General shell parsing and rewriting: complex, fragile, and unnecessary for
  direct argv.

## Decision 4: Use one Rust package with Tokio

**Decision**: Build one Rust 2024 package with modules and one multi-call binary.
Use Tokio 1.49 for child I/O, timers, cancellation coordination, Unix sockets,
Windows named pipes, and the supervisor event loop.

**Rationale**: The product needs concurrent stdout/stderr streaming, wait versus
cancel selection, and cross-platform local IPC. Tokio provides these in one
runtime. A single package avoids premature workspace boundaries.

**Alternatives considered**:

- Standard-library threads only: viable for embedded execution but duplicates
  cross-platform async IPC and supervisor coordination.
- Multiple crates from the start: more manifests and public boundaries before
  protocols stabilize.
- A second daemon language: violates the single Rust product boundary.

**Source**: Tokio 1.49 documentation for `tokio::process`, `select!`, signals,
Unix sockets, and Windows named pipes, retrieved through Context7.

## Decision 5: Use SQLite WAL plus log files

**Decision**: Persist job, receipt-consumption, execution, lease, delivery,
retention, and recovery state in SQLite WAL transactions. Store full stdout and
stderr in per-job files and write immutable spec/result JSON through atomic
rename.

**Rationale**: Execution and delivery require independent, atomic transitions
across crashes and concurrent sessions. Large logs do not belong in database
rows or model context.

**Alternatives considered**:

- JSON state and advisory locks only: simple for one hook but fragile for a
  durable multi-session supervisor and crash recovery.
- Store logs in SQLite: increases database write amplification and complicates
  follow mode.
- External database: unnecessary for a per-user local tool.

## Decision 6: Use the installed Codex sandbox CLI shape

**Decision**: Spawn real jobs through
`codex sandbox -P PROFILE -C CWD -- PROGRAM ARG...`.

**Rationale**: Local Codex CLI 0.146.0 exposes this cross-platform command shape.
It preserves named permission profiles and allows the working directory to
participate in profile resolution. The locally installed CLI does not recognize
`codex sandbox macos` as a subcommand.

**Alternatives considered**:

- Launch the requested program directly from the hook: silently bypasses Codex
  sandbox policy.
- Use remembered platform-specific sandbox subcommands: contradicted by the
  current installed CLI.
- Automatically fall back to direct execution: fail-open and prohibited.

## Decision 7: Use explicit runtime modes with one execution backend

**Decision**: Embedded mode calls the runner in the hook process. Durable mode
submits the same protocol to a local supervisor. Both use the same runner,
store, output, security, and process-control modules.

**Rationale**: Embedded mode gives the shortest zero-polling path; durable mode
supports process survival and recovery without creating divergent semantics.

**Alternatives considered**:

- Always require a service: unnecessary operational burden for ordinary jobs.
- Separate MCP execution backend: risks duplicate behavior and security drift.

## Decision 8: Use ordered, leased recovery

**Decision**: Delivery priority is active `PostToolUse`, then `SessionStart`,
then optional `codex exec resume`. Delivery requires a lease and per-session
lock. Automatic resume is disabled by default and has a strict attempt budget.

**Rationale**: An active hook is the only true same-turn path. Recovery must not
race it or create multiple Codex processes.

**Alternatives considered**:

- Always run `codex exec resume`: creates duplicate session writers and spends
  a model request even when the original hook is alive.
- Mark the job delivered before resume succeeds: loses results after resume
  failure.

## Decision 9: CLI installs the plugin, not the reverse

**Decision**: The Rust CLI renders a local plugin marketplace and invokes the
stable `codex plugin marketplace add`, `codex plugin add`, and
`codex plugin remove` commands. Hooks use the resolved absolute binary path.

**Rationale**: Codex plugins package skills, hooks, MCP configuration, and
assets but do not install a global CLI or run package lifecycle scripts. The
CLI is independently useful and is therefore the installation authority.

**Alternatives considered**:

- Bundle platform binaries in the plugin: does not add them to `PATH`, bloats
  each plugin, and complicates updates.
- Directly edit user `config.toml`: more invasive and less compatible than the
  stable plugin commands.

**Source**: Current Codex manual plugin packaging and marketplace sections;
verified with local `codex plugin` help.

## Decision 10: Keep output bounded and explicitly untrusted

**Decision**: Write complete byte streams to local files. Return bounded byte
tails, metadata, hashes, and log paths. Prefix model-facing output with a clear
untrusted-data warning and redact configured secret patterns.

**Rationale**: Continuous logs are the second major source of token waste and
may contain prompt-injection text or credentials.

**Alternatives considered**:

- Return all output: unbounded token use and injection exposure.
- Store only tails: removes evidence needed for diagnostics.

## Decision 11: Put the at-most-once boundary in an internal worker

**Decision**: Both embedded hooks and the durable supervisor launch
`longrun internal worker JOB_ID`. The worker must acquire an exclusive execution
claim before spawning the requested command. Only the worker can transition the
job to running.

**Rationale**: No parent process can atomically combine OS process creation and
database persistence. A crash after spawn but before recording the PID can make
a recovery path start the command twice. A separate claim-owning worker makes
duplicate parent launches harmless because only one worker reaches command
spawn.

**Alternatives considered**:

- Mark running before spawn: a crash can strand a job that never started.
- Mark running after spawn: a crash gap can duplicate execution.
- Detect only by PID: the PID may never have been persisted and can be reused.

## Decision 12: Define delivery as effectively once

**Decision**: Every delivery has a stable idempotency key and one active lease.
Retries reuse the same identity. Exactly one resume process may be started, but
a hook envelope may be repeated after an uncertain crash boundary.

**Rationale**: Current Codex hooks do not acknowledge that the host consumed
stdout after the hook process exits. Strict exactly-once delivery is therefore
not provable. Effectively-once delivery preserves safety and lets duplicate
envelopes be recognized without claiming an impossible guarantee.

**Alternatives considered**:

- Mark delivered before writing stdout: can lose a result.
- Mark delivered after writing stdout: can repeat an envelope after a crash.
- Claim strict exactly once: not supported by the current hook protocol.

## Decision 13: Publish one CLI through binary and source channels

**Decision**: Produce checksummed release archives for macOS, Linux, and
Windows, an installer that verifies them, a Homebrew formula, and Cargo package
metadata.

**Rationale**: Ordinary users should not need a Rust toolchain, while developers
retain a standard source-install path. All channels install the same binary,
which remains the authority for Codex integration.

**Alternatives considered**:

- Cargo-only installation: requires a toolchain and long local build.
- Binary inside the Codex plugin: does not create a normal terminal command and
  multiplies plugin size by platform.
