# Implementation Plan: Ephemeral RTK-Style Wait Proxy

**Branch**: `002-ephemeral-wait-proxy` | **Date**: 2026-08-01 | **Spec**:
[spec.md](./spec.md)

**Input**: Feature specification from
`/specs/002-ephemeral-wait-proxy/spec.md`

## Summary

Replace Longrun's durable job system with one ephemeral, RTK-style command
proxy. Humans and CI run `longrun PROGRAM ARG...`; Codex can use the same
surface through `rtk longrun PROGRAM ARG...`. PreToolUse creates a short-lived
handoff and rewrites the Bash call to a fast marker stub. PostToolUse claims the
handoff, runs the target through the configured Codex sandbox, waits locally,
and returns one bounded result to the same active turn. Direct terminal/CI
execution uses the same runner without requiring Codex. There is no
supervisor, per-job worker, durable result, recovery, automatic retry, or second
Longrun approval system.

The implementation reuses the existing Rust CLI, hook integration, runner,
output, and platform process-control foundations while deleting the durable
store, receipt-signing, supervisor, worker, IPC, MCP job backend, service
lifecycle, SessionStart recovery, and job-management surface.

## Technical Context

**Language/Version**: Rust 2024, rust-version 1.88

**Primary Dependencies**: Existing `clap`, `tokio`, `serde`, `serde_json`,
`getrandom`, `time`, `nix`, and `windows-sys`; no new crate planned.

**Storage**: A protected handoff file/record in the platform runtime directory
with a default 300000 ms TTL and a maximum 900000 ms TTL. No runtime SQLite
database, durable job record, completed result, delivery lease, or Longrun-owned
output log.

**Testing**: `cargo test --locked`, focused Rust unit/integration tests, Codex
hook fixtures, Unix/macOS process tests, Windows Job Object tests, and
release-gated live GitHub Actions and Oracle checks.

**Target Platform**: macOS, Linux, and Windows with platform-specific process
ownership. Codex command hooks must support a synchronous PostToolUse timeout
with cleanup margin.

**Project Type**: Standalone Rust CLI with a thin Codex hook/skill integration.

**Performance Goals**:

- Receipt stub completes within 1000 ms in the deterministic hook harness
  without starting the target.
- No model status request occurs while PostToolUse waits.
- Rolling output memory remains bounded by configured limits.
- Handoff claim and deletion add no polling loop or background service.

**Constraints**:

- No durable supervisor, per-job worker process, recovery, resume, automatic
  retry, or second Codex process.
- No silent permission widening.
- Direct argument mode must preserve native arguments without shell rebuilding.
- Target exit status is result data after the receipt stub has already exited.
- macOS uncatchable owner death is documented best effort.
- The PostToolUse timeout must satisfy
  `post_tool_use_timeout_ms >= target_timeout_ms + termination_grace_ms +
  forced_cleanup_margin_ms + result_serialization_margin_ms`; diagnostics must
  expose the arithmetic.

**Scale/Scope**: One finite target per active Longrun invocation; concurrent
Codex integrations use independent handoff records, while direct invocations
remain independent synchronous processes. No job history or cross-session
workload management.

## Constitution Check

*GATE: Must pass before Phase 0 research and re-check after Phase 1 design.*

| Principle / gate | Status | Evidence |
| --- | --- | --- |
| I. Eliminate Model Polling | PASS | PostToolUse waits locally; no status loop or model continuation is required. |
| II. CLI Is the Product | PASS | Generic `longrun PROGRAM ARG...` and `rtk longrun PROGRAM ARG...` are canonical. |
| III. Continue the Same Work | PASS | Only the active PostToolUse turn receives the result; lost ownership requires manual rerun. |
| IV. One Handoff, One Owned Execution | PASS | Protected `prepared -> armed -> claimed -> deleted` handoff and no automatic retry. |
| V. Preserve Security Boundaries | PASS | Explicit named sandbox profile, fail-closed policy, no Longrun approval hook or auto escalation. |
| VI. Keep Context Small and Evidence Local | PASS | Concurrent bounded in-memory tails; no completed Longrun result persistence. |
| Quality gates | PASS | Format, clippy, locked tests, process, hook, security, live GH, and Oracle evidence are planned. |

The constitution was amended from 3.0.0 to 4.0.0 before this plan because the
previous final architecture required durable execution and recovery, which
conflicted with this explicitly requested RTK-style product boundary. It was
clarified to 4.0.1 so direct terminal/CI execution is not coupled to Codex
hooks or a Codex installation.

## Research Summary

Research decisions and source evidence are recorded in
[research.md](./research.md). The key conclusions are:

1. PostToolUse must own the wait; direct Bash execution cannot reliably cover
   multi-minute targets.
2. RTK's transparent rewrite model is the right user-facing shape.
3. One ephemeral, atomically claimed handoff is the smallest cross-process
   state.
4. Tokio `Child::kill_on_drop(true)` helps direct-child cleanup but does not
   replace Unix process groups or Windows Job Objects for descendants.
5. Explicit named permission profiles are required for Codex-hook execution;
   hook trust and the immutable profile are the authorization boundary, exact
   transient Codex approval inheritance is not claimed, and direct terminal/CI
   execution remains independent of Codex.
6. GitHub Actions and Oracle remain generic wrapped CLIs, not Longrun protocols.

## Design

### Command surface and parsing

Update `src/cli.rs` so the target command is parsed after the Longrun program
name, with only a small reserved management surface for version, integration,
and diagnostics. Preserve an explicit separator for target names that collide
with management commands. Do not add a Longrun shell parser; `/bin/sh -c` (or
the platform shell) is an explicit target argv when shell semantics are needed.

Accept the RTK wrapper form by normalizing `rtk longrun ...` to the same target
argv. Reuse the current strict shell-word parser and composition rejection, but
remove the requirement that the target appear after `submit --`.

Keep direct terminal/CI execution synchronous and return the target's real exit
status. Removed durable commands return a clear migration error.

### Ephemeral handoff

Add a focused handoff module (planned path: `src/handoff.rs`) that owns:

- protected runtime directory creation;
- native-string serialization;
- origin and command identity hashes;
- configurable expiry (`handoff_ttl_ms`, default 300000 ms, maximum 900000 ms);
- `prepared`, `armed`, and `claimed` states;
- atomic state transitions;
- one-time claim and cleanup.

Use the existing random-byte dependency and standard filesystem operations.
Avoid HMAC keys, SQLite, UUID job identity, delivery leases, and a second
receipt schema in the final design.

The handoff record must snapshot the target argv, canonical cwd, the named
permission profile when the invocation is Codex-integrated, environment policy,
timeout, termination grace, forced-cleanup margin, result-serialization margin,
and output limit. A crash after claim is inert and never retried.

### Codex hooks

`src/hook/pre_tool_use.rs` will:

1. Ignore unrelated Bash commands.
2. Recognize only `longrun ...` and `rtk longrun ...`.
3. Reject composition, shell evaluation, unsupported wrappers, and multiple
   Longrun invocations.
4. Create a prepared handoff.
5. Return an absolute-path internal receipt stub as updated input.

The stub path will:

1. Record the platform's shell-parsed native target arguments where required.
2. Atomically arm the prepared handoff.
3. Emit exactly one opaque marker.
4. Exit within 1000 ms in the deterministic hook harness without starting the
   target.

`src/hook/post_tool_use.rs` will:

1. Parse exactly one marker.
2. Validate session, turn, tool use, cwd, command/stub identity, expiry, and
   policy snapshot.
3. Atomically claim the handoff.
4. Call the shared runner directly.
5. Return one bounded result envelope through `PostToolUse`.
6. Delete the handoff and transient output state.

Remove `src/hook/session_start.rs` and its dispatch/asset because there is no
recovery path.

### Shared runner and bounded output

Refactor `src/runner.rs` to be the only target executor:

- for Codex-hook execution, launch `codex sandbox` with the immutable named
  profile, `--include-managed-config`, and cwd; fail closed if the launcher
  rejects that option;
- for direct terminal/CI execution, launch the target with the same native
  process controls without requiring the Codex executable;
- clear the environment and apply the explicit allowlist;
- call `Child::kill_on_drop(true)` for direct-child cleanup;
- concurrently drain stdout and stderr into fixed-capacity rolling buffers;
- track total bytes and truncation without reading complete files back;
- wait for exit, timeout, cancellation, and owner shutdown;
- terminate the whole process group or Job Object;
- return an in-memory `TargetExecution`/`ResultEnvelope`.

The target's exit code is placed in model-visible data. The implementation must
not claim that the receipt stub's already-returned process status changed.

### Platform lifetime

Update `src/platform/unix.rs` and `src/platform/mod.rs` to:

- retain a dedicated process group;
- terminate the group on timeout, cancellation, handled signal, and observable
  owner shutdown;
- clean descendants even when the leader exits first;
- use a bounded grace period followed by forced group kill;
- add parent/owner observation only where the platform can observe it;
- document macOS uncatchable `SIGKILL`/crash limitations.

Update `src/platform/windows.rs` to:

- create the target suspended;
- assign it to a Job Object before resuming;
- set kill-on-close and no-breakaway policy;
- support graceful console break followed by `TerminateJobObject`;
- terminate a suspended child if assignment fails.

### Output and policy configuration

Simplify `src/config.rs` to keep only:

- target timeout;
- termination grace;
- forced cleanup margin;
- result serialization margin;
- handoff TTL;
- PostToolUse timeout, validated against the target timeout plus all cleanup and
  serialization margins;
- named permission profile and explicit danger opt-in for Codex-hook execution;
- environment pass/deny policy;
- bounded output limit;

Remove recovery, retry budget, retention, service, delivery lease, and durable
supervisor settings. Ensure the generated PostToolUse timeout satisfies the
documented arithmetic and that forced cleanup remains inside that timeout.

### Integration and documentation

Update:

- `assets/codex/hooks.json`: retain only active PreToolUse/PostToolUse wait
  hooks, set `additionalContextLimit` to `0` for the bounded Longrun result, and
  remove SessionStart recovery. Unix hook commands should use `exec` so the
  Longrun hook process is the process owned by Codex rather than an extra shell
  layer.
- `assets/codex/skills/longrun/SKILL.md`: teach generic `longrun PROGRAM ARG...`
  and `rtk longrun PROGRAM ARG...`; remove `submit`, worker, recovery, and
  polling guidance.
- `assets/codex/plugin.json`: update description and ownership metadata to the
  generic command surface; preserve the existing version unless the repository
  release policy explicitly requires a bump.
- `src/integration/codex.rs`: render/repair/remove only the thin integration.
- `src/integration/service.rs`: delete service lifecycle integration.
- `README.md`: remove durable commands, supervisor, recovery, service,
  retention, and result persistence; document the manual-rerun contract and
  target-exit-code limitation.
- `install.sh` and release metadata only where integration repair/upgrade
  behavior requires it.

Old Longrun state must be inert. Repair may delete Longrun-owned old
SessionStart/service assets, but the new runtime must never interpret old
SQLite jobs or results as recoverable work.

### Test strategy

Use fixture executables and hook JSON fixtures for deterministic tests. Keep
live tests separate, ignored by default, and opt-in through explicit release
validation. The active-session acceptance test uses a
125-second target; shorter local fixtures may cover fast-path behavior but do
not replace that acceptance run:

- hook recognition and one-time handoff;
- same-turn wait/no-polling;
- exact target exit result;
- bounded output, injection resistance, and no Codex hook-output spill;
- no completed persistence;
- sandbox/environment fail-closed behavior;
- Unix/macOS process tree;
- Windows Job Object containment;
- installation/repair;
- live GitHub Actions watch;
- live Oracle browser wait.

## Project Structure

### Documentation (this feature)

```text
specs/002-ephemeral-wait-proxy/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── implementation-log.md
├── contracts/
│   ├── cli.md
│   └── hook-protocol.md
├── checklists/
│   └── requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
src/
├── cli.rs                  # generic target surface and management commands
├── config.rs               # timeout, profile, environment, output policy
├── handoff.rs              # new ephemeral prepared/armed/claimed state
├── hook/
│   ├── input.rs
│   ├── output.rs
│   ├── pre_tool_use.rs
│   └── post_tool_use.rs
├── integration/
│   └── codex.rs            # thin active-hook install/repair/uninstall
├── output.rs               # bounded in-memory tails/result envelope
├── platform/
│   ├── mod.rs
│   ├── unix.rs
│   └── windows.rs
├── protocol.rs             # native argv, handoff, target, result contracts
├── runner.rs               # sole target executor
└── main.rs

tests/
├── cli.rs
├── handoff.rs
├── hooks.rs
├── output.rs
├── process_tree.rs
├── runner.rs
├── security.rs
├── integration_codex.rs
└── live/
    ├── active_session.rs
    ├── github_watch.rs
    └── oracle.rs
```

The following current runtime components are deleted rather than left dormant:

```text
src/store.rs
src/worker.rs
src/supervisor.rs
src/ipc/
src/mcp.rs
src/hook/session_start.rs
src/integration/service.rs
```

**Structure Decision**: Keep one Rust binary and one shared runner. Add only
the small handoff module needed to bridge separate Codex hook processes.
Delete the durable scheduler and its state/protocol surface instead of
maintaining a second inactive architecture.

## Implementation Phases

### Phase 0 - Governance and contract baseline

1. Validate the v4 constitution amendment, feature spec, research, data model,
   contracts, and quickstart.
2. Add a compatibility inventory for current commands, files, tests, generated
   assets, and old runtime state.
3. Establish failing contract tests for new syntax and removed durable commands.

### Phase 1 - Ephemeral handoff and direct runner

1. Implement the handoff state and atomic claim.
2. Refactor CLI parsing and direct execution.
3. Refactor the runner to bounded in-memory output and direct result envelopes.
4. Add process ownership behavior and tests.

### Phase 2 - Active Codex hooks

1. Rewrite PreToolUse for generic Longrun/RTK syntax.
2. Implement the fast receipt stub.
3. Rewrite PostToolUse to claim, execute, wait, deliver, and delete.
4. Remove SessionStart and durable dispatch.

### Phase 3 - Delete durable architecture

1. Remove store, worker, supervisor, IPC, MCP job backend, service lifecycle,
   recovery, retention, and retry configuration.
2. Remove old commands and stale protocol types/dependencies.
3. Add upgrade/repair cleanup for old integration assets and inert state.

### Phase 4 - Integration, docs, and acceptance

1. Update hooks, skill, plugin metadata, README, and installation diagnostics.
2. Run static and deterministic tests.
3. Run release-gated GitHub Actions and Oracle live scenarios.
4. Verify no model polling, no durable artifacts, and manual-rerun behavior.

## Verification Matrix

| Requirement group | Task evidence |
| --- | --- |
| FR-001, FR-002, FR-003, FR-004, FR-005 | T001, T002, T016, T019, T025, T026, T029, T030 |
| FR-006, FR-007, FR-008, FR-009, FR-010 | T003, T005, T008, T010, T011, T017, T019, T020, T021, T023 |
| FR-011, FR-012, FR-013 | T002, T006, T009, T012, T015, T022, T033, T038, T046, T047 |
| FR-014, FR-015, FR-016 | T007, T013, T014, T034, T036 |
| FR-017, FR-018, FR-019, FR-020 | T009, T010, T025, T042, T043, T045, T047 |
| FR-021, FR-022, FR-023, FR-024 | T001, T004, T029, T035, T049, T050, T051, T052, T053, T054, T055, T056, T057, T059 |
| SC-001 | T018, T024 |
| SC-002 | T017, T018, T021, T022, T024 |
| SC-003 | T005, T017, T033 |
| SC-004 | T015, T033, T035, T037, T038, T039, T040, T041, T058, T061 |
| SC-005 | T007, T013, T014, T034, T036 |
| SC-006 | T002, T006, T044, T046, T047, T062 |
| SC-007 | T042, T045, T048 |
| SC-008 | T025, T029, T030, T031 |
| SC-009 | T016, T025, T026, T031 |
| SC-010 | T004, T049, T050, T051, T052, T053, T054, T056, T057, T059 |
| SC-011 | T027, T028, T032, T062 |
| SC-012 | T004, T034, T040, T056, T059, T061, T062 |
| Constitution quality gates | T015, T060, T061, T062 |

## Complexity Tracking

No constitution violations remain after the v4.0.1 amendment. The only
intentional platform limitation is documented macOS behavior after uncatchable
owner death; adding a helper or supervisor to eliminate it would violate the
product boundary rather than simplify the design.
