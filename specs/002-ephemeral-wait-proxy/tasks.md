---

description: "Implementation tasks for the Ephemeral RTK-Style Wait Proxy"

---

# Tasks: Ephemeral RTK-Style Wait Proxy

**Input**: Design documents from
`/specs/002-ephemeral-wait-proxy/`

**Prerequisites**: `plan.md`, `spec.md`, `research.md`, `data-model.md`,
`contracts/`, `quickstart.md`

**Tests**: Required by the Longrun constitution and the feature specification.
Write the smallest failing test before the implementation it protects.

**Organization**: Tasks are grouped by user story so each priority slice can be
implemented and validated independently after the shared foundation.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel because it touches different files and has no
  dependency on incomplete work.
- **[Story]**: Maps the task to a user story in `spec.md`.
- Every task includes an exact repository-relative file path.

## Phase 1: Setup (Shared Contract Baseline)

**Purpose**: Establish the new command surface and deterministic test fixtures
before changing runtime ownership.

- [X] T001 Add removed-command migration cases for `run`, `run-shell`, `submit`, `submit-shell`, job-management commands, and `mcp`, plus canonical generic command cases in `tests/cli.rs`
- [X] T002 [P] Add deterministic long-command, large-output, child-tree, and configurable-exit fixtures in `tests/fixtures/longrun_target.rs`
- [X] T003 [P] Update isolated runtime-directory and hook-session test setup for ephemeral state in `tests/setup.rs`
- [X] T004 Record the old-to-new command, hook, and state migration inventory in `specs/002-ephemeral-wait-proxy/implementation-log.md`

**Checkpoint**: New syntax, removed command behavior, and deterministic fixtures
are represented by failing tests.

## Phase 2: Foundational (Ephemeral Runtime)

**Purpose**: Implement the one handoff and one shared executor required by every
user story.

**⚠️ CRITICAL**: No user story work can finish until this phase passes.

- [X] T005 [P] Add failing prepared/armed/claimed/deleted, expiry, mismatch, duplicate-claim, and 100-attempt sequential/concurrent stress tests in `tests/handoff.rs`
- [X] T006 [P] Add failing bounded-tail, byte-count, truncation, invalid-byte, and untrusted-output tests in `tests/output.rs`
- [X] T007 [P] Add failing child-drop, timeout, cancellation, descendant, and leader-exit process tests in `tests/process_tree.rs`
- [X] T008 Implement protected ephemeral handoff serialization, atomic transitions, expiry cleanup, and one-time claim in `src/handoff.rs`
- [X] T009 Simplify native argument, target, terminal-reason, and result-envelope types for ephemeral execution in `src/protocol.rs`
- [X] T010 Simplify handoff TTL, timeout, termination/forced-cleanup margins, output, result-serialization-margin, and PostToolUse-timeout configuration in `src/config.rs`
- [X] T011 Add ephemeral runtime paths and restrictive ownership checks without creating a durable job database in `src/paths.rs`
- [X] T012 Implement the shared direct executor with `Child::kill_on_drop(true)`, concurrent rolling output, exact target status, and no complete-log reread in `src/runner.rs`
- [X] T013 Implement Unix process-group cleanup and observable owner-shutdown handling in `src/platform/mod.rs` and `src/platform/unix.rs`
- [X] T014 Implement suspended Windows child assignment, kill-on-close Job Object ownership, nested-job failure cleanup, and graceful/forced termination in `src/platform/windows.rs`
- [X] T015 Wire the reduced module graph and remove obsolete runtime exports in `src/lib.rs` and `src/main.rs`

**Checkpoint**: Handoff, runner, bounded result, and platform ownership tests
pass without supervisor, worker, SQLite, or recovery code.

## Phase 3: User Story 1 - Wait Without Model Polling (Priority: P1) 🎯 First usable slice

**Goal**: One Codex command waits locally in PostToolUse and returns its final
result to the same active turn without model polling.

**Independent Test**: Run the active-session harness with a target longer than
the normal shell wait threshold and verify one target start, no polling, and
one same-turn result.

### Tests for User Story 1

- [X] T016 [P] [US1] Add failing exact Longrun/RTK hook-recognition and shell-composition tests in `tests/hooks.rs`
- [X] T017 [P] [US1] Add failing same-turn, sub-1000-ms fast-stub, no-polling, and duplicate-PostToolUse tests in `tests/active_session.rs`
- [X] T018 [US1] Add the ignored live active-session harness for a 125-second target and event-stream evidence in `tests/live/active_session.rs`

### Implementation for User Story 1

- [X] T019 [US1] Implement generic `longrun` and `rtk longrun` PreToolUse recognition, strict argument validation, and prepared handoff creation in `src/hook/pre_tool_use.rs`
- [X] T020 [US1] Implement the internal receipt-stub command that arms the handoff, emits one marker, and exits without starting the target in `src/cli.rs`
- [X] T021 [US1] Implement PostToolUse marker validation, atomic claim, direct execution, local wait, and manual-rerun-on-loss behavior in `src/hook/post_tool_use.rs`
- [X] T022 [US1] Implement active-turn result serialization with `continue: false`, exact target status data, bounded tails, and untrusted-output labeling in `src/hook/output.rs`
- [X] T023 [US1] Update hook input/output dispatch and remove SessionStart handling from `src/hook/input.rs`, `src/hook/mod.rs`, and `src/cli.rs`
- [X] T024 [US1] Run the focused hook suite and active-session harness, then record no-polling and same-turn evidence in `specs/002-ephemeral-wait-proxy/implementation-log.md`

**Checkpoint**: User Story 1 is independently usable for a finite command
longer than the normal shell wait threshold.

## Phase 4: User Story 2 - Transparent Generic Command Surface (Priority: P1)

**Goal**: Humans, CI, and agents use the same generic `longrun PROGRAM ARG...`
surface for GitHub Actions, Oracle, and arbitrary finite commands.

**Independent Test**: Run direct success/failure fixtures and both Longrun/RTK
forms; verify literal argv and target status preservation.

### Tests for User Story 2

- [X] T025 [P] [US2] Add direct terminal/CI success, failure, timeout, and literal-argv tests in `tests/cli.rs`
- [X] T026 [P] [US2] Add generic wrapper normalization tests for `longrun` and `rtk longrun` in `tests/hooks.rs`
- [X] T027 [P] [US2] Add ignored, release-gated GitHub Actions watch scenarios for success, failure, cancellation, auth failure, and no polling in `tests/live/github_watch.rs`
- [X] T028 [P] [US2] Add ignored, release-gated Oracle browser scenarios for one invocation, bounded output, failure, cleanup, and no reattachment in `tests/live/oracle.rs`

### Implementation for User Story 2

- [X] T029 [US2] Implement canonical target parsing, reserved management commands, explicit separator handling, and removed-command errors in `src/cli.rs`
- [X] T030 [US2] Implement direct terminal/CI result rendering and target exit-code propagation in `src/cli.rs`
- [X] T031 [US2] Add deterministic generic passthrough and quickstart smoke coverage for GitHub/Oracle argument preservation in `tests/fixtures/longrun_target.rs`
- [X] T032 [US2] Run direct, GitHub, and Oracle validation according to `specs/002-ephemeral-wait-proxy/quickstart.md` and record evidence in `specs/002-ephemeral-wait-proxy/implementation-log.md`

**Checkpoint**: User Story 2 works without special CI/Oracle Longrun commands
and without exposing the handoff protocol.

## Phase 5: User Story 3 - Ephemeral Ownership and Manual Rerun (Priority: P1)

**Goal**: The active hook owns the target; owner loss ends the attempt and never
creates a background result or automatic recovery.

**Independent Test**: Exercise timeout, cancellation, handled signal, owner
shutdown, duplicate delivery, lost delivery, and stale-state paths.

### Tests for User Story 3

- [X] T033 [P] [US3] Add failing no-recovery, lost-delivery, manual-rerun, and no-persistent-artifact tests in `tests/active_session.rs`
- [X] T034 [P] [US3] Add failing Unix/macOS process-group, descendant, leader-exit, signal, and documented hard-kill limitation checks in `tests/process_tree.rs`
- [X] T035 [US3] Add failing old-state inertness and removed durable command tests in `tests/cli.rs`

### Implementation for User Story 3

- [X] T036 [US3] Route timeout, cancellation, handled signals, and observable owner shutdown through one process-tree cleanup path in `src/runner.rs`
- [X] T037 [US3] Delete durable job, worker, supervisor, IPC, MCP job backend, service, SessionStart, and recovery modules in `src/store.rs`, `src/worker.rs`, `src/supervisor.rs`, `src/ipc/`, `src/mcp.rs`, `src/integration/service.rs`, and `src/hook/session_start.rs`
- [X] T038 [US3] Remove durable CLI dispatch, daemon/service commands, recovery configuration, retention, delivery leases, and retry settings in `src/cli.rs`, `src/config.rs`, and `src/main.rs`
- [X] T039 [US3] Remove durable-only dependencies and obsolete protocol/build wiring in `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, and `src/protocol.rs`
- [X] T040 [US3] Implement inert handling and cleanup guidance for old SQLite/job artifacts during repair and diagnostics in `src/integration/codex.rs` and `src/paths.rs`
- [X] T041 [US3] Run state inspection after success, failure, timeout, cancellation, and lost delivery and record manual-rerun evidence in `specs/002-ephemeral-wait-proxy/implementation-log.md`

**Checkpoint**: No active Longrun process can create or recover a durable job.

## Phase 6: User Story 4 - Explicit Permission and Bounded Context (Priority: P1)

**Goal**: Codex owns approval and sandbox policy; the wait proxy directly runs
the approved target in the inherited hook environment and returns only bounded,
untrusted result data.

**Independent Test**: Attempt denied filesystem/network operations, secret
inheritance, shell composition, large output, invalid bytes, and injection-like
output.

### Tests for User Story 4

- [X] T042 [P] [US4] Add failing second-sandbox, second-approval, environment-filtering, and direct-inherited-environment tests in `tests/security.rs`
- [X] T043 [US4] Add failing shell-composition, wrapper, metacharacter, and literal-native-argv tests in `tests/security.rs`
- [X] T044 [P] [US4] Add failing large-output, invalid-byte, fake-hook-JSON, fake-receipt, prompt-injection, and no-hook-spill output tests in `tests/output.rs`

### Implementation for User Story 4

- [X] T045 [US4] Keep Codex approval and sandbox policy outside Longrun; execute Codex-hook targets directly with the inherited environment and no second permission gate in `src/config.rs` and `src/runner.rs`
- [X] T046 [US4] Implement rolling bounded stdout/stderr capture, byte counts, truncation metadata, escaped result data, and untrusted-output envelopes in `src/output.rs` and `src/hook/output.rs`
- [X] T047 [US4] Add PostToolUse timeout arithmetic, `additionalContextLimit: 0`, and direct-runner diagnostics without a PermissionRequest hook in `assets/codex/hooks.json` and `src/integration/codex.rs`
- [X] T048 [US4] Run the security and bounded-context suites and record direct-execution/no-second-boundary evidence in `specs/002-ephemeral-wait-proxy/implementation-log.md`

**Checkpoint**: Longrun cannot widen permissions or flood model context while
waiting.

## Phase 7: User Story 5 - Thin Integration Install and Upgrade (Priority: P2)

**Goal**: Installation and repair expose only the ephemeral wait adapter and
remove obsolete durable guidance.

**Independent Test**: Install into an isolated Codex home, inspect generated
assets, repair an old installation, and preserve unrelated configuration.

### Tests for User Story 5

- [X] T049 [P] [US5] Add failing hook/skill/plugin rendering snapshots for the new command surface in `tests/integration_codex.rs`
- [X] T050 [US5] Add failing init, repair, moved-binary, obsolete-SessionStart, and unrelated-file-preservation tests in `tests/integration_codex.rs`
- [X] T051 [US5] Add failing doctor checks for active hooks, timeout margin, direct runner availability, and absence of supervisor/service health checks in `tests/integration_codex.rs`

### Implementation for User Story 5

- [X] T052 [US5] Render only active PreToolUse/PostToolUse hooks with the absolute executable path, Unix `exec` wrapper, `additionalContextLimit: 0`, and timeout margin in `assets/codex/hooks.json`
- [X] T053 [US5] Rewrite the installed Longrun skill for generic `longrun PROGRAM ARG...` and `rtk longrun PROGRAM ARG...` with no polling/recovery/submit guidance in `assets/codex/skills/longrun/SKILL.md`
- [X] T054 [US5] Update plugin description and ownership metadata for the generic command surface while preserving the release version policy in `assets/codex/plugin.json`, `assets/codex/marketplace.json`, and `src/integration/codex.rs`
- [X] T055 [US5] Remove remaining service lifecycle generation and supervisor diagnostics from `src/integration/codex.rs`
- [X] T056 [US5] Rewrite README installation, command, security, migration, manual-rerun, and GitHub/Oracle examples in `README.md`
- [X] T057 [US5] Run isolated Codex-home install, repair, doctor, and uninstall validation and record evidence in `specs/002-ephemeral-wait-proxy/implementation-log.md`

**Checkpoint**: A repaired installation has only the thin active wait adapter.

## Phase 8: Polish and Cross-Cutting Verification

**Purpose**: Remove stale tests/assets and prove the whole feature against the
constitution and quickstart.

- [X] T058 [P] Remove obsolete durable, recovery, supervisor, worker, store, MCP, and SessionStart tests from `tests/durable_session.rs`, `tests/live/durable_session.rs`, `tests/live/session_start.rs`, `tests/recovery.rs`, `tests/supervisor.rs`, `tests/worker.rs`, `tests/store.rs`, `tests/mcp.rs`, and `tests/session_start.rs`
- [X] T059 Update active integration fixtures and source references that still mention `submit`, workers, recovery, or result persistence in `tests/active_session.rs`, `tests/integration_codex.rs`, and `assets/codex/skills/longrun/SKILL.md`
- [X] T060 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --locked`, then record exact results in `specs/002-ephemeral-wait-proxy/implementation-log.md`
- [X] T061 Run a final repository search for durable command, supervisor, worker, SessionStart recovery, polling, and automatic-retry references in `README.md`, `src/`, `tests/`, and `assets/`
- [X] T062 Validate every scenario in `specs/002-ephemeral-wait-proxy/quickstart.md` and map evidence to FR/SC coverage in `specs/002-ephemeral-wait-proxy/implementation-log.md`

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No implementation dependency; establishes test fixtures
  and the migration inventory.
- **Foundational (Phase 2)**: Depends on Setup and blocks every user story.
- **User Story 1 (Phase 3)**: Depends on the handoff, runner, output, and
  platform foundation; this is the first usable implementation slice, not a
  separate product boundary or release constitution.
- **User Story 2 (Phase 4)**: Depends on User Story 1's hook path and shared
  CLI runner.
- **User Story 3 (Phase 5)**: Depends on User Story 1 and deletes the durable
  paths that would otherwise compete with the ephemeral owner.
- **User Story 4 (Phase 6)**: Depends on the shared runner and hook result path;
  it can begin after Phase 2, but final integration depends on US1.
- **User Story 5 (Phase 7)**: Depends on the final CLI and hook command surface.
- **Polish (Phase 8)**: Depends on all desired user stories.

### User Story Dependencies

- **US1 (P1)**: Starts after Phase 2; no dependency on US2-US5.
- **US2 (P1)**: Depends on US1's generic hook normalization and runner.
- **US3 (P1)**: Depends on US1; removes old competing execution owners.
- **US4 (P1)**: Depends on Phase 2; integrates with US1 result delivery.
- **US5 (P2)**: Depends on US1-US4's final command and lifecycle contracts.

### Parallel Opportunities

- T002 and T003 can run in parallel during setup; T005, T006, and T007 can
  run in parallel after the Phase 1 checkpoint.
- T016, T017, and T018 can run in parallel before US1 implementation.
- T025, T027, and T028 can run in parallel after the generic command surface is
  stable.
- T033, T034, and T035 can run in parallel because they cover separate
  lifecycle fixtures.
- T042, T043, and T044 can run in parallel before US4 implementation.
- T049, T050, and T051 can run in parallel before integration changes.
- T058 and T059 can run in parallel after the durable modules are deleted.

## Parallel Example: User Story 1

```text
Task T016: hook grammar fixtures in tests/hooks.rs
Task T017: active-session fixtures in tests/active_session.rs
Task T018: live no-polling harness in tests/live/active_session.rs
```

After those tests exist:

```text
Task T019: PreToolUse handoff preparation
Task T020: receipt-stub command
Task T021: PostToolUse direct wait
```

T021 depends on T008 and T012 and must not introduce a second target-spawn
implementation.

## Implementation Strategy

### First usable slice (User Story 1)

1. Complete Phase 1 and Phase 2.
2. Implement the new handoff and direct runner.
3. Complete User Story 1.
4. Run the 125-second active-session/no-polling validation.
5. Stop and verify same-turn completion before deleting the remaining durable
   surface.

### Incremental Delivery

1. Add transparent direct/RTK command behavior (US2).
2. Delete recovery and durable ownership paths (US3).
3. Harden permission and bounded output behavior (US4).
4. Repair installed integration and documentation (US5).
5. Run the complete quickstart and cross-platform gates.

### Notes

- `[P]` tasks touch disjoint files and have no incomplete dependency.
- Every task has a concrete file path and is independently reviewable.
- No task adds a new execution backend or durable scheduler.
- Manual rerun is the only recovery behavior for lost ownership.

## Phase 9: Convergence

The deterministic generic wait path is implemented, and the following
convergence evidence is complete:

- [X] T063 Add lifecycle tests for cancellation, handled signals, leader exit, descendant cleanup, and documented uncatchable owner death per FR-014, FR-015, and SC-005 (completed)
- [X] T064 Add lost-delivery, manual-rerun, old-state inertness, and no-persistent-artifact tests per FR-012, FR-013, FR-022, and SC-004 (completed)
- [X] T065 Add direct-inherited-environment, forged-receipt, prompt-injection, and no-hook-spill tests per FR-011, FR-017, FR-019, and SC-006/SC-007 (completed)
- [X] T066 Add ignored Codex-hook GitHub Actions and Oracle browser acceptance harnesses covering one invocation, failure/auth behavior, bounded output, and no reattachment per US2 and SC-011 (completed)
- [X] T067 Run the complete quickstart matrix and map direct, RTK, active-session, GitHub, Oracle, lifecycle, security, and upgrade evidence to FR/SC coverage per SC-001–SC-012 (completed)
