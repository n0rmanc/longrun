<!--
Sync Impact Report
- Version change: 4.0.0 -> 4.0.1
- Bump rationale: clarify that ephemeral handoffs and named Codex profiles
  apply to Codex hook execution, while direct terminal/CI execution uses the
  same native runner without requiring Codex.
- Modified principles:
  - III. Continue the Same Work: recovery is removed; only the active turn is
    continued and owner loss requires manual rerun.
  - IV. Run Once, Deliver Safely -> IV. One Handoff, One Owned Execution:
    durable execution, delivery retry, and result recovery are removed.
  - VI. Keep Context Small and Evidence Local: completed-result persistence is
    removed; bounded in-memory output is required.
- Added sections:
  - Ephemeral Runtime and Lifetime
  - Handoff and Result Semantics
- Removed sections:
  - Durable supervisor and recovery architecture
  - Persistent job-management and delivery requirements
- Follow-up items: none.
-->
# Longrun Constitution

## Core Principles

These principles express the repository's enduring values. Specifications and
implementations MUST optimize for them rather than for a particular release.

### I. Eliminate Model Polling

Longrun exists to run finite, long-running commands without periodic model
requests. Waiting MUST happen in local Rust processes or Codex hooks, not
through repeated `write_stdin`, status prompts, sleep loops, or agent turns.
The active Codex turn MUST receive one final result after the command completes.

### II. CLI Is the Product

`longrun` MUST be a standalone, installable Rust CLI usable by humans, scripts,
CI, and coding agents. Codex integration, skills, and hooks MUST remain thin
adapters over the same command runner. They MUST NOT create a second execution
backend or expose implementation-only workflow to users.

The canonical user-facing execution form is:

```text
longrun PROGRAM ARG...
```

The Codex integration MUST also accept the transparent RTK wrapper form:

```text
rtk longrun PROGRAM ARG...
```

Small management commands such as `init --codex`, `doctor`, `uninstall --codex`,
and `--version` MAY remain reserved. A target colliding with a management
command MUST be invokable through an explicit separator.

### III. Continue the Same Work

When the originating Codex process and active `PostToolUse` hook remain alive,
completion MUST return to the same session and turn through that hook. Longrun
MUST NOT start a second Codex process for normal completion.

If the active hook or originating session is gone, Longrun MUST NOT recover,
resume, or redeliver the result. The owned command MUST be terminated on every
handled and observable owner-shutdown path, and the user MUST explicitly rerun
the command.

### IV. One Handoff, One Owned Execution

Each accepted Codex-integrated Longrun invocation MUST create one short-lived
handoff and allow at most one execution claim. The handoff MUST transition
monotonically from prepared to armed to claimed and then be deleted. A replay,
duplicate hook, expired handle, malformed record, or claim failure MUST start no
command. Direct terminal/CI invocations execute synchronously without a Codex
handoff.

This is an invocation guarantee, not an exactly-once guarantee across a user
manually rerunning a command after an interrupted execution. Longrun MUST NOT
automatically retry an interrupted or failed command.

Execution and delivery do not require durable job records. For Codex
integration, the active `PostToolUse` hook owns both the command wait and the
one final delivery.

### V. Preserve Security Boundaries

Codex-hook execution MUST NOT gain permissions merely because a hook launches
it. It MUST use an explicitly configured named Codex sandbox profile and MUST
fail closed when that profile is unavailable, disallowed, or invalid. Direct
terminal/CI execution MUST use the shared native process controls and MUST NOT
require a Codex installation. Network access, workspace-external writes, secret
inheritance, shell evaluation, and danger-full-access require explicit
configuration.

Longrun MUST NOT implement a second user approval prompt or automatically
approve permission escalation. It MUST NOT claim to inherit transient approval
state that Codex does not expose to hook input. It MUST never widen the
configured profile between PreToolUse and PostToolUse.

Direct argument execution is the default. Compound shell execution MUST be
explicit in the target argv (for example `/bin/sh -c '...'`); Longrun MUST NOT
evaluate outer-shell syntax. Native arguments MUST remain OS strings internally
and MUST NOT be reconstructed through shell interpolation.

### VI. Keep Context Small and Evidence Local

Longrun MUST drain stdout and stderr concurrently into bounded in-memory
buffers. The model-visible result MUST contain only exit metadata, terminal
reason, duration, byte counts, truncation flags, bounded tails, and an explicit
untrusted-output marker.

Longrun MUST NOT retain completed-result rows, durable delivery records, or
Longrun-owned stdout/stderr logs after the active invocation completes. Wrapped
tools MAY retain their own artifacts according to their own behavior; Longrun
MUST NOT treat those artifacts as recoverable work.

Command output MUST be escaped and labeled as untrusted data rather than
instructions. The target command's exact exit status MUST be included in the
result envelope, but Longrun MUST NOT claim that it can retroactively change
the exit status of the already-completed receipt stub.

## Target Architecture

The final product MUST consist of one Rust command runner with one ephemeral
Codex wait adapter:

1. `longrun PROGRAM ARG...` executes synchronously for humans, scripts, and CI.
2. `PreToolUse` recognizes only the exact Longrun form, rejects shell
   composition and unsupported wrappers, creates a short-lived protected
   handoff, and rewrites the Bash call to a fast internal receipt stub.
3. The receipt stub records the shell-parsed native arguments, arms the
   handoff, emits one opaque marker, and exits without starting the target.
4. `PostToolUse` validates and atomically claims the handoff, executes the
   target through the shared sandbox runner, waits locally, and returns one
   bounded result to the active turn. Direct terminal/CI execution uses the
   same runner without requiring Codex.
5. The runtime MUST use no supervisor, per-job worker process, durable job
   database, IPC job backend, SessionStart recovery, automatic resume, or
   service lifecycle for command execution.
6. Handoff state MUST be short-lived and protected by the current user's
   filesystem permissions for Codex hook execution. It MUST contain only the
   origin binding, target arguments, immutable policy snapshot, expiry, and
   one-way claim state.
7. The shared runner MUST own timeout, cancellation, output draining, and
   process-tree cleanup. For Codex hooks, `codex sandbox` is the permission
   boundary, not a Longrun worker.
8. Codex integration MUST install only the active PreToolUse and PostToolUse
   hooks needed for the wait adapter. It MUST NOT install SessionStart recovery
   hooks or operating-system services.

## Ephemeral Runtime and Lifetime

Only ephemeral execution is supported. For Codex integration, the active
`PostToolUse` hook owns the target process and waits for its terminal result.
Direct terminal/CI execution waits synchronously in the invoking Longrun
process. On timeout, cancellation, handled signal, or observable owner
shutdown, Longrun MUST terminate the entire owned process tree, wait for the
configured grace period, and then force-kill remaining descendants.

Unix implementations MUST use a dedicated process group and test leader exit,
descendant cleanup, timeout, cancellation, and handled signal paths. Linux MAY
use parent-death signaling as defense in depth. macOS MUST document that an
uncatchable `SIGKILL`, crash, or power loss of the sole owner can leave
descendants until external cleanup; guaranteeing zero orphans in that case
would require an external owner and is outside this product boundary.

Windows implementations MUST use a Job Object with kill-on-close. The child
MUST be created suspended, assigned to the Job Object, and resumed only after
assignment succeeds so startup cannot escape containment.

### Handoff and Result Semantics

The handoff lifecycle MUST be:

```text
prepared -> armed -> claimed -> deleted
```

The Codex handoff MUST bind the session, turn, tool-use identity, canonical
working directory, visible command identity, generated stub identity, native
target arguments, immutable permission/timeout/output policy, and expiry. A
random opaque marker MUST be protected by a one-time claim. A cryptographic
receipt MAY be retained temporarily during migration, but it MUST NOT be
required by the final architecture.

Normal target completion, including a nonzero target exit, MUST be returned as
`PostToolUse` feedback with bounded untrusted output. Malformed, forged,
expired, mismatched, or policy-invalid handoffs MUST fail closed. The receipt
stub's own exit status is not the target exit status; the target status is
model-visible result data.

## Quality Gates

Every change MUST pass:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

Lifecycle, protocol, persistence, process, security, and integration changes
MUST include the smallest automated test that fails when the contract breaks.

Every supported execution path MUST have evidence proving:

1. One Codex-integrated Longrun invocation creates one handoff and at most one
   target start; direct terminal/CI invocation starts one target synchronously.
2. No periodic model turn or status polling occurs while the target runs.
3. The active path continues through the original `PostToolUse` hook.
4. Duplicate, forged, expired, mismatched, and malformed handoffs execute
   nothing.
5. No completed Longrun job, result, delivery lease, recovery record, or
   Longrun-owned output log remains after completion.
6. The target exit code and terminal reason are represented exactly in the
   bounded result envelope.
7. Codex sandbox denial fails without automatic permission escalation or retry;
   direct terminal/CI execution does not require a Codex sandbox.
8. Timeout, cancellation, and handled owner shutdown terminate the owned
   process tree.
9. Full output is never copied into model context and protected credentials
   are not exposed.
10. GitHub Actions watch and Oracle browser runs return their final result to
    the active turn without model polling.

Architecture changes MUST prefer deletion, the Rust standard library, and
existing dependencies before adding crates or abstractions. New execution
backends, durable schedulers, recovery paths, and per-job workers are
prohibited unless the constitution is explicitly amended again.

## Governance

This constitution defines Longrun's final product values and architecture.
Feature specifications, plans, and tasks MUST conform to it. Exceptions MUST
be documented in the relevant plan with their necessity, security impact,
owner, and removal condition.

Amendments require a rationale, an updated Sync Impact Report, and a semantic
version change. MAJOR versions remove or redefine a core value or architecture
boundary, MINOR versions add or materially expand governance, and PATCH
versions clarify wording without changing obligations.

Every implementation review MUST map its changes and verification evidence to
the applicable principles and quality gates. Unresolved violations block
completion.

**Version**: 4.0.1 | **Ratified**: 2026-07-31 | **Last Amended**: 2026-08-01
