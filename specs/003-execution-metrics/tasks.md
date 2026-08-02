---

description: "Task list for local execution metrics and the gain command"
---

# Tasks: Local Execution Metrics

**Input**: Design documents from
`/specs/003-execution-metrics/`

**Prerequisites**: `plan.md`, `spec.md`, `research.md`, `data-model.md`,
`contracts/cli.md`, `quickstart.md`

**Tests**: Automated tests are required by the constitution for persistence,
CLI, security, and hook changes.

**Organization**: Tasks are grouped by user story so each story can be
implemented and validated independently after the shared metrics foundation.

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Add the feature module without changing command behavior.

- [x] T001 Create and export the metrics module in `src/metrics.rs` and `src/lib.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Build the minimal local metric model and storage used by every
user story.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [x] T002 Define `ExecutionMetric`, `GainReport`, `OutcomeCounts`, `ProgramSummary`, and strict serde validation in `src/metrics.rs`
- [x] T003 Implement terminal-outcome classification, executable-basename normalization, and completion timestamp handling in `src/metrics.rs`
- [x] T004 Implement private per-record metric publication, invalid-record skipping, metrics scanning, and metrics-only clear in `src/metrics.rs`
- [x] T005 Add focused tests for metric schema, all terminal outcome classifications, outcome-count invariants, atomic record visibility, invalid records, privacy fields, and clear ordering in `tests/metrics.rs`

**Checkpoint**: The metrics module can safely record and aggregate terminal
metadata without knowing whether the caller is direct CLI or Codex hook mode.

---

## Phase 3: User Story 1 - See Longrun Usage at a Glance (Priority: P1) 🎯 MVP

**Goal**: `longrun gain` reports global recorded execution count, elapsed time,
average duration, and terminal-outcome counts, with both direct and Codex-hook
executions recorded exactly once.

**Independent Test**: In isolated XDG directories, run successful, nonzero, and
timed-out direct targets plus one Codex-hook target; run `longrun gain` and
verify the global totals and outcome counts.

### Tests for User Story 1

- [x] T006 [US1] Add CLI parsing tests for `gain`, `gain --clear`, and explicit `longrun -- gain` target collision in `tests/cli.rs`
- [x] T007 [US1] Add isolated direct-execution integration tests for success, nonzero exit, timeout, empty history, outcome-count sums, exactly-once totals, and a 100-record aggregate in `tests/cli.rs`
- [x] T008 [P] [US1] Add hook integration tests for terminal recording and timeout coverage, duplicate `PostToolUse` suppression, and incomplete/forged handoff non-recording in `tests/hooks.rs`; keep owner-shutdown classification covered by `tests/metrics.rs`

### Implementation for User Story 1

- [x] T009 [US1] Add the visible `Gain` subcommand and `GainArgs` options in `src/cli.rs`
- [x] T010 [US1] Record direct `Runner::execute` terminal results through the metrics module while preserving the target-derived exit code in `src/cli.rs`
- [x] T011 [US1] Record Codex-hook `Runner::execute` terminal results after handoff cleanup while preserving the existing bounded same-turn result in `src/hook/post_tool_use.rs`
- [x] T012 [US1] Render the human-readable global gain report and terminal-outcome counts from `src/metrics.rs` through `src/cli.rs`
- [x] T013 [US1] Keep `gain` as a management command in the hook recognizer and require `longrun -- gain` for an external target collision in `src/hook/pre_tool_use.rs`

**Checkpoint**: The P1 report works for both supported execution paths, does
not count the receipt stub, and never changes target execution semantics.

---

## Phase 4: User Story 2 - Understand Which Programs Consume the Wait (Priority: P2)

**Goal**: The report groups executions by executable basename with count,
total duration, and average duration without exposing arguments or paths.

**Independent Test**: Record executions for at least three executable names,
including repeated names with different arguments, and verify that per-program
counts sum to the global count and no raw arguments appear.

### Tests for User Story 2

- [x] T014 [P] [US2] Add per-program aggregation tests for repeated executable names, different arguments, sorted summaries, and count/duration invariants in `tests/metrics.rs`
- [x] T015 [US2] Add CLI output assertions for per-program count, total duration, average duration, and argument/path privacy in `tests/cli.rs`

### Implementation for User Story 2

- [x] T016 [US2] Implement deterministic per-program aggregation and human table formatting in `src/metrics.rs`
- [x] T017 [US2] Wire per-program summaries into the human gain report without adding command-line argument or working-directory fields in `src/cli.rs`

**Checkpoint**: The P2 breakdown is actionable, compact, and privacy-safe while
preserving the P1 totals.

---

## Phase 5: User Story 3 - Automate and Reset the Report (Priority: P2)

**Goal**: Scripts can consume exact JSON metrics, and users can clear only
Longrun execution history.

**Independent Test**: Compare `longrun gain` with `longrun gain --json`, clear
metrics, verify zero totals, and confirm unrelated Longrun state remains
usable.

### Tests for User Story 3

- [x] T018 [P] [US3] Add JSON schema/value parity tests for `gain --json` and global `--json` in `tests/cli.rs`
- [x] T019 [US3] Add clear-scope and ordering tests proving `gain --clear` does not execute or stop a target, preserves a metric published after clear completes, and leaves configuration, handoff, and integration paths unchanged in `tests/cli.rs` and `tests/hooks.rs`

### Implementation for User Story 3

- [x] T020 [US3] Implement `--json` report serialization and `--clear` dispatch behavior in `src/cli.rs`
- [x] T021 [US3] Emit the documented `cleared` JSON response and preserve exact integer millisecond values in `src/metrics.rs`

**Checkpoint**: Human and JSON reports express the same values, and clear
starts a fresh metrics period without touching execution state.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Document the contract and prove the complete feature against the
repository gates.

- [x] T022 [P] Update `README.md` with `longrun gain`, `--json`, `--clear`, management-command collision, the no-token-estimate boundary, and the feature quickstart link
- [x] T023 [P] Validate the end-to-end scenarios in `specs/003-execution-metrics/quickstart.md`
- [x] T024 [P] Add a 10,000-record aggregation performance check for the stated scan goal in `tests/metrics.rs`
- [ ] T025 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --locked` against `Cargo.toml`, `src/`, and `tests/`
- [ ] T026 Review implementation evidence against the constitution gates and update `specs/003-execution-metrics/plan.md` if any design or verification claim is no longer true

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies; creates the module entry point.
- **Foundational (Phase 2)**: Depends on T001 and blocks every user story.
- **User Story 1 (Phase 3)**: Depends on Phase 2; it is the MVP and establishes
  the public command plus both recording entry points.
- **User Story 2 (Phase 4)**: Depends on Phase 2 and the report wiring in T012;
  its metrics aggregation tests can run after T004.
- **User Story 3 (Phase 5)**: Depends on the command/report wiring in T009 and
  T012; it changes the same CLI surface and should follow the P1 slice.
- **Polish (Phase 6)**: Depends on all desired stories.

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Phase 2; no dependency on User Story 2
  or User Story 3.
- **User Story 2 (P2)**: Its aggregation foundation is in Phase 2; final
  presentation depends on the P1 report wiring, but it does not change target
  execution.
- **User Story 3 (P2)**: Depends on the P1 command/report path so JSON and clear
  preserve the same totals; it has no dependency on per-program behavior.

### Parallel Opportunities

- T008 can run in parallel with T006/T007 after the shared test helpers exist.
- T014 can run in parallel with T015 after T004 and T012 are available.
- T018 and T019 can run in parallel after the P1 CLI command is wired.
- T022, T023, and T024 can run in parallel after implementation is complete.

## Parallel Example: User Story 1

```text
Task T006: CLI parsing and collision tests in tests/cli.rs
Task T008: Codex hook lifecycle tests in tests/hooks.rs
```

Both tests target separate files and can be prepared in parallel after the
foundational metrics API exists. Implementation tasks T010, T011, and T013
touch separate runtime boundaries and can be reviewed independently before the
P1 checkpoint.

## Implementation Strategy

### MVP First

1. Complete Phase 1 and Phase 2.
2. Complete User Story 1, including direct and Codex-hook recording.
3. Validate the P1 checkpoint with the isolated CLI and hook tests.
4. Stop for a usable global human-readable `longrun gain` report.

### Incremental Delivery

1. Add User Story 2 for per-program actionability.
2. Add User Story 3 for JSON automation and metrics reset.
3. Update README and run the full quickstart and quality gates.

The MVP intentionally does not claim token savings or provider request counts;
those require data Longrun does not receive.

## Notes

- Every task has a checkbox, sequential ID, required story label where
  applicable, and at least one concrete repository path.
- `[P]` marks only tasks that can usefully proceed in parallel without
  modifying the same file or waiting on incomplete work.
- No task adds a worker, daemon, SQLite queue, retry loop, recovery mechanism,
  or new dependency.
