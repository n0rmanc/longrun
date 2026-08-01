---

description: "Implementation tasks for Longrun"
---

# Tasks: Long-Running Command Execution

**Input**: Design documents from
`/specs/001-long-command-execution/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/,
quickstart.md

**Tests**: Required by the Longrun constitution. Tests are written before the
implementation they protect and include unit, integration, hook fixture,
process, recovery, cross-platform, and live Codex checks.

**Organization**: Tasks are grouped by user story. The complete task list
delivers the final architecture; phase boundaries are implementation order, not
product-scope exclusions.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel because it touches different files and does not
  depend on incomplete work.
- **[Story]**: Maps the task to a user story in spec.md.
- Every task includes an exact file path.

## Phase 1: Setup

**Purpose**: Establish the single-package Rust project and shared validation.

- [X] T001 Create the Rust 2024 package and binary named `longrun` in `Cargo.toml`, `src/lib.rs`, and `src/main.rs`
- [X] T002 Add the locked dependency set from plan.md with target-specific Unix and Windows dependencies in `Cargo.toml`
- [X] T003 [P] Define shared error types and process exit-code mapping in `src/error.rs`
- [X] T004 [P] Define OS-specific application, state, log, socket, and integration paths in `src/paths.rs`
- [X] T005 [P] Configure repository ignores for build, live-test, and local Longrun state artifacts in `.gitignore`
- [X] T006 [P] Add reusable command fixture binaries for success, failure, output, sleep, and descendant spawning in `tests/fixtures/commands/`
- [X] T007 Add the implementation evidence log and significant-step review checklist in `specs/001-long-command-execution/implementation-log.md`

**Checkpoint**: `cargo check`, `cargo fmt --check`, and fixture compilation pass.

---

## Phase 2: Foundational Runtime

**Purpose**: Implement contracts required by every user story.

**⚠️ CRITICAL**: User-story work starts only after this phase passes tests.

- [X] T008 [P] Add failing unit tests for lossless UTF-8, Unix-byte, and Windows-UTF-16 argument round trips in `tests/protocol.rs`
- [X] T009 [P] Add failing state-transition and terminal-state tests for execution and delivery models in `tests/store.rs`
- [X] T010 [P] Add failing configuration default, override, validation, and secret-pattern tests in `tests/config.rs`
- [X] T011 Implement versioned `NativeString`, job, result, delivery, and IPC domain types in `src/protocol.rs`
- [X] T012 Implement validated execution, output, environment, recovery, concurrency, and retention configuration in `src/config.rs`
- [X] T013 Implement SQLite WAL migrations and transaction helpers for pending submissions, jobs, executions, results, leases, and integrations in `src/store.rs`
- [X] T014 Implement atomic JSON specification/result writes and secure state-directory permissions in `src/store.rs`
- [X] T015 [P] Implement bounded byte-tail extraction, truncation metadata, hashes, and untrusted-output rendering in `src/output.rs`
- [X] T016 [P] Implement structured tracing with secret-safe fields and configurable diagnostics in `src/main.rs`
- [X] T017 Define CLI parsing for all commands in the CLI contract without business logic in `src/cli.rs`
- [X] T018 Wire async runtime, configuration loading, path resolution, command dispatch, and exit mapping in `src/main.rs`
- [X] T019 Run foundational unit tests and record review evidence in `specs/001-long-command-execution/implementation-log.md`

**Checkpoint**: Domain, configuration, storage, output, and CLI parsing contracts
are independently tested.

---

## Phase 3: User Story 1 - Run Long Commands Without Model Polling (Priority: P1)

**Goal**: A Codex submission starts once, waits locally, and continues the same
active work without periodic model requests.

**Independent Test**: Feed PreToolUse and PostToolUse fixtures around a
90-second command and prove one process start, no Longrun polling interface
call, and one same-turn bounded completion result.

### Tests for User Story 1

- [X] T020 [P] [US1] Add failing receipt encode, HMAC, expiry, mismatch, and single-consumption tests in `tests/receipts.rs`
- [X] T021 [P] [US1] Add failing PreToolUse no-op, strict-path, shell-rejection, token-rewrite, and wrapper-allow tests in `tests/hooks.rs`
- [X] T022 [P] [US1] Add failing PostToolUse receipt extraction, pending-context match, same-turn output, and duplicate-call tests in `tests/hooks.rs`
- [X] T023 [P] [US1] Add a parameterized active-hook live harness with a 90-second smoke and 30-minute SC-001 acceptance mode in `tests/live/active_session.rs`

### Implementation for User Story 1

- [X] T024 [US1] Implement canonical receipt payload encoding, exact-byte HMAC signing, verification, expiry, and zeroized secret handling in `src/receipt.rs`
- [X] T025 [US1] Implement Codex common, PreToolUse, PostToolUse, and SessionStart input deserialization in `src/hook/input.rs`
- [X] T026 [US1] Implement Codex allow/deny, continue, system-message, and additional-context output serialization in `src/hook/output.rs`
- [X] T027 [US1] Implement strict absolute-binary submission parsing and shell-composition rejection in `src/hook/pre_tool_use.rs`
- [X] T028 [US1] Implement one-time hook-token creation, pending-state persistence, targeted updatedInput generation, and wrapper-only allow output in `src/hook/pre_tool_use.rs`
- [X] T029 [US1] Implement hidden hook-token claim and receipt-only stdout for `longrun submit` and `submit-shell` in `src/cli.rs`
- [X] T030 [US1] Implement PostToolUse receipt extraction, signature/context verification, atomic consumption, and job acceptance in `src/hook/post_tool_use.rs`
- [X] T031 [US1] Implement embedded local completion wait and `continue: false` bounded-result delivery in `src/hook/post_tool_use.rs`
- [X] T032 [US1] Wire `longrun hook codex pre-tool-use`, `post-tool-use`, and `session-start` dispatch in `src/hook/mod.rs`
- [X] T033 [US1] Run hook fixtures and the active-session harness, review the diff, and record evidence in `specs/001-long-command-execution/implementation-log.md`

**Execution dependency**: T030, T031, and the end-to-end portion of T033
require the single sandboxed worker from T036-T037. T024-T029 and pre-tool
hook dispatch can be completed first; no PostToolUse path may introduce an
alternate direct command spawn while that shared runner is incomplete.

**Checkpoint**: US1 passes without `write_stdin`, sleep-status loops, duplicate
execution, or a second Codex process.

---

## Phase 4: User Story 2 - Use Longrun Directly from a Terminal (Priority: P1)

**Goal**: Humans and CI can execute commands directly with preserved output and
exit status.

**Independent Test**: Run successful, failing, timed-out, and mixed-output
fixtures through `longrun run`.

### Tests for User Story 2

- [X] T034 [P] [US2] Add failing direct-run exit-code, stdout/stderr, timeout, and non-UTF-8 argv tests in `tests/cli.rs`
- [X] T035 [P] [US2] Add failing process-spawn, result-persistence, and output-log tests in `tests/runner.rs`

### Implementation for User Story 2

- [X] T036 [US2] Implement sandbox command construction using `codex sandbox -P PROFILE -C CWD -- ...` in `src/runner.rs`
- [X] T037 [US2] Implement exclusive execution claims plus the hidden worker's asynchronous child execution, separate output streaming, timeout selection, and result persistence in `src/worker.rs` and `src/runner.rs`
- [X] T038 [US2] Implement `longrun run` and explicit `run-shell` command behavior with child-status propagation in `src/cli.rs`
- [X] T039 [US2] Add human-readable and JSON result rendering without mixing stdout and diagnostics in `src/cli.rs`
- [X] T040 [US2] Run direct CLI live fixtures, review the diff, and record evidence in `specs/001-long-command-execution/implementation-log.md`

**Checkpoint**: US2 is usable without Codex plugin installation.

---

## Phase 5: User Story 6 - Preserve Execution Security (Priority: P1)

**Goal**: Longrun retains sandbox, environment, receipt, and argument security
boundaries.

**Independent Test**: Attempt denied writes/network, secret inheritance, shell
injection, forged receipts, and path shadowing; none gains execution authority
or elevated access.

### Tests for User Story 6

- [X] T041 [P] [US6] Add failing environment allowlist, deny-pattern, and explicit secret-pass tests in `tests/security.rs`
- [X] T042 [P] [US6] Add failing sandbox-denial and no-escalation tests in `tests/security.rs`
- [X] T043 [P] [US6] Add failing absolute-binary, PATH-shadowing, shell-metacharacter, and replay tests in `tests/security.rs`
- [X] T044 [P] [US6] Add failing Unix process-group and Windows Job Object cleanup tests in `tests/process_tree.rs`

### Implementation for User Story 6

- [X] T045 [US6] Implement safe environment construction and explicit `--env-pass` policy in `src/runner.rs`
- [X] T046 [US6] Implement danger-full-access dual opt-in and fail-closed sandbox validation in `src/config.rs`
- [X] T047 [US6] Implement Unix process-group creation, graceful termination, and forced group kill in `src/platform/unix.rs`
- [X] T048 [US6] Implement Windows Job Object creation, assignment, graceful stop, and kill-on-close in `src/platform/windows.rs`
- [X] T049 [US6] Route timeout, cancellation, hook shutdown, and supervisor shutdown through one process-tree cleanup interface in `src/platform/mod.rs`
- [X] T050 [US6] Implement constant-time receipt verification, replay rejection, and pending-token expiry cleanup in `src/receipt.rs`
- [X] T051 [US6] Run sandbox and process-tree live tests, review the diff, and record evidence in `specs/001-long-command-execution/implementation-log.md`

**Checkpoint**: Longrun cannot silently execute outside its configured
permission and environment boundary.

---

## Phase 6: User Story 3 - Inspect and Control Jobs (Priority: P2)

**Goal**: Users can wait, inspect, list, follow logs, cancel, and clean jobs
without model polling.

**Independent Test**: Start a job and control it from a second terminal through
every job operation.

### Tests for User Story 3

- [X] T052 [P] [US3] Add failing wait, status, list, and JSON contract tests in `tests/cli.rs`
- [X] T053 [P] [US3] Add failing log read/follow, stdout/stderr selection, and binary-output tests in `tests/cli.rs`
- [X] T054 [P] [US3] Add failing idempotent cancellation and retention-safe garbage-collection tests in `tests/store.rs`

### Implementation for User Story 3

- [X] T055 [US3] Implement wait, status, and newest-first filtered list queries in `src/store.rs`
- [X] T056 [US3] Implement byte-safe log reads and local follow behavior in `src/output.rs`
- [X] T057 [US3] Implement idempotent cancellation state and owner notification in `src/runner.rs`
- [X] T058 [US3] Implement age and total-log-byte retention selection that excludes active, leased, and undelivered jobs in `src/store.rs`
- [X] T059 [US3] Wire `wait`, `status`, `list`, `logs`, `cancel`, and `gc` command output in `src/cli.rs`
- [X] T060 [US3] Run two-terminal job-control validation, review the diff, and record evidence in `specs/001-long-command-execution/implementation-log.md`

**Checkpoint**: All job operations work locally without agent involvement.

---

## Phase 7: User Story 4 - Recover Completed Work Safely (Priority: P2)

**Goal**: Durable jobs survive Codex termination and deliver once through the
ordered recovery paths.

**Independent Test**: Submit a durable job, terminate Codex, complete the job,
restart the session, and observe exactly one recovery delivery.

### Tests for User Story 4

- [X] T061 [P] [US4] Add failing framed IPC request, response, event, version, and malformed-frame tests in `tests/supervisor.rs`
- [X] T062 [P] [US4] Add failing Unix socket and Windows named-pipe transport tests in `tests/supervisor.rs`
- [X] T063 [P] [US4] Add failing delivery-lease, expiry, stable-idempotency, per-session lock, retry-budget, and duplicate-resume tests in `tests/recovery.rs`
- [X] T064 [P] [US4] Add failing supervisor restart and completed-before-persist recovery tests in `tests/recovery.rs`
- [X] T065 [P] [US4] Add a durable Codex termination/restart live harness in `tests/live/durable_session.rs`

### Implementation for User Story 4

- [X] T066 [US4] Implement length-prefixed JSON request, response, and event framing in `src/ipc/mod.rs`
- [X] T067 [US4] Implement per-user Unix-domain socket server and client transport in `src/ipc/unix.rs`
- [X] T068 [US4] Implement per-user Windows named-pipe server and client transport in `src/ipc/windows.rs`
- [X] T069 [US4] Implement durable supervisor ownership, concurrency limits, completion events, and health in `src/supervisor.rs`
- [X] T070 [US4] Implement durable submit, wait, status, logs, cancel, and gc routing through the shared runtime in `src/supervisor.rs`
- [X] T071 [US4] Implement delivery leases, lease expiry, session locks, attempt budgets, and atomic delivery marking in `src/store.rs`
- [X] T072 [US4] Implement ordered active-hook then SessionStart recovery with stable idempotency envelopes in `src/hook/session_start.rs`
- [X] T073 [US4] Implement disabled-by-default guarded `codex exec resume` recovery in `src/supervisor.rs`
- [X] T074 [US4] Implement launchd, systemd-user, and Windows per-user service artifact generation in `src/integration/service.rs`
- [X] T075 [US4] Wire `daemon` and `service install|uninstall|start|stop|status` commands in `src/cli.rs`
- [X] T076 [US4] Run supervisor crash and durable-session live tests, review the diff, and record evidence in `specs/001-long-command-execution/implementation-log.md`

**Checkpoint**: Durable execution and recovery are idempotent and never race an
active hook.

---

## Phase 8: User Story 5 - Install and Manage Longrun (Priority: P2)

**Goal**: Users can install a verified Longrun binary without a Rust toolchain,
then install, diagnose, repair, and remove the Codex integration without
changing unrelated configuration.

**Independent Test**: Install a release artifact on a clean supported machine,
then install into an isolated Codex home, inspect generated files, repair after
moving the binary, and uninstall while preserving a sentinel plugin and config
value.

### Tests for User Story 5

- [X] T077 [P] [US5] Add failing plugin, hook, skill, and marketplace rendering snapshot tests in `tests/integration_codex.rs`
- [X] T078 [P] [US5] Add failing idempotent init, moved-binary repair, and preservation-focused uninstall tests in `tests/integration_codex.rs`
- [X] T079 [P] [US5] Add failing doctor checks for Codex version, plugin commands, hooks, sandbox profile, store, and supervisor in `tests/integration_codex.rs`

### Implementation for User Story 5

- [X] T080 [P] [US5] Add the Longrun plugin manifest template in `assets/codex/plugin.json`
- [X] T081 [P] [US5] Add absolute-path SessionStart, PreToolUse, and PostToolUse hook templates in `assets/codex/hooks.json`
- [X] T082 [P] [US5] Add the agent workflow and no-polling rules in `assets/codex/skills/longrun/SKILL.md`
- [X] T083 [P] [US5] Add the local `longrun-local` marketplace template in `assets/codex/marketplace.json`
- [X] T084 [US5] Implement atomic template rendering, owned-file inventory, and manifest hashing in `src/integration/codex.rs`
- [X] T085 [US5] Implement idempotent `codex plugin marketplace add` and `codex plugin add longrun@longrun-local` orchestration in `src/integration/codex.rs`
- [X] T086 [US5] Implement repair and preservation-safe plugin/marketplace uninstall in `src/integration/codex.rs`
- [X] T087 [US5] Implement doctor diagnostics and human/JSON reports in `src/integration/codex.rs`
- [X] T088 [US5] Wire `init --codex`, `uninstall --codex`, and `doctor` commands in `src/cli.rs`
- [X] T089 [US5] Run timed isolated-Codex-home install/doctor/repair/uninstall live tests proving SC-009, review the diff, and record evidence in `specs/001-long-command-execution/implementation-log.md`
- [X] T090 [P] [US5] Add failing platform selection, archive layout, and checksum verification tests in `tests/install.rs`
- [X] T091 [P] [US5] Add failing Cargo package metadata and release-artifact manifest checks in `tests/install.rs`
- [X] T092 [US5] Implement checksummed GitHub Release binary installation for macOS and Linux in `install.sh`
- [X] T093 [US5] Add the Homebrew formula using published release checksums in `Formula/longrun.rb`
- [X] T094 [US5] Add Cargo publishing metadata plus macOS/Linux/Windows archive, checksum, clean-install, and `longrun doctor` automation in `Cargo.toml` and `.github/workflows/release.yml`

**Checkpoint**: Integration lifecycle is repeatable and preserves unrelated
user state.

---

## Phase 9: Cross-Cutting Completion

**Purpose**: Prove the final architecture across interfaces and platforms.

- [X] T095 [P] Add a structured MCP status/wait/logs/cancel adapter and `longrun mcp` dispatch that delegate only to supervisor IPC in `src/mcp.rs` and `src/cli.rs`
- [X] T096 [P] Add MCP adapter contract tests proving no independent spawn path in `tests/mcp.rs`
- [X] T097 [P] Add macOS, Linux, and Windows compile/test matrix configuration in `.github/workflows/ci.yml`
- [X] T098 [P] Add user installation, configuration, security, recovery, and command documentation in `README.md`
- [X] T099 Add performance tests for submit p95, hook no-op p95, status p95, completion latency, and model byte bounds in `tests/performance.rs`
- [X] T100 Add a 100-iteration execution/replay/delivery stress test covering SC-002 and SC-006 in `tests/recovery.rs`
- [X] T101 Run every quickstart scenario and record exact commands and outcomes in `specs/001-long-command-execution/implementation-log.md`
- [X] T102 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, and platform compile checks; record results in `specs/001-long-command-execution/implementation-log.md`
- [X] T103 Review all source and generated integration assets for constitution compliance, unnecessary dependencies, duplicate execution paths, and secret exposure in `specs/001-long-command-execution/implementation-log.md`

**Checkpoint**: All functional requirements, success criteria, constitution
gates, live scenarios, and platform contracts have evidence.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup**: no dependencies.
- **Foundational Runtime**: depends on Setup and blocks all stories.
- **US1**: receipt and PreToolUse routing depend on Foundational Runtime;
  PostToolUse acceptance, waiting, and its live checkpoint additionally depend
  on T036-T037's single sandboxed worker.
- **US2**: depends on Foundational Runtime; may proceed in parallel with US1.
- **US6**: depends on the US1 receipt path and US2 runner path.
- **US3**: depends on the shared store and runner; may begin after US2.
- **US4**: depends on job operations, process ownership, and delivery state from
  US1, US3, and US6.
- **US5**: depends on stable hook and command contracts from US1 and US4.
- **Cross-Cutting Completion**: depends on all user stories.

### User Story Dependencies

```text
Foundational
├── US1 ─────────┬────────────> US4 -> US5
│                └-> US6 ─────> US4
└── US2 ─┬────────> US3 ──────> US4
         └────────> US6
```

US1 and US2 are independently demonstrable after Foundational. Later stories
reuse their stable runtime contracts rather than creating alternate paths.

### Within Each User Story

1. Add the smallest failing contract or integration test.
2. Implement the minimum shared behavior that makes the test pass.
3. Run focused tests and a live scenario where the story crosses process or
   Codex boundaries.
4. Review the diff for constitution and security compliance.
5. Record evidence and commit the logical story slice.

### Parallel Opportunities

- T003-T006 can run in parallel after T001-T002.
- T008-T010 can run in parallel.
- T015-T016 can run in parallel after domain contracts exist.
- US1 and US2 test scaffolding can run in parallel after Foundational.
- Tests marked `[P]` within each story touch separate files or fixture groups.
- Codex asset templates T080-T083 can run in parallel.
- T090-T091 can run in parallel after the integration contract stabilizes.
- T095-T099 can run in parallel after all story contracts stabilize.

## Parallel Example: User Story 1

```text
T020 receipts tests
T021 PreToolUse tests
T022 PostToolUse tests
T023 active-session live harness
```

After those tests fail for the expected missing behavior, implement
T024-T032 in dependency order.

## Implementation Strategy

### First Independently Testable Slice

1. Complete Setup and Foundational Runtime.
2. Complete US1 and its live active-session check.
3. Keep the complete final scope in tasks.md; do not treat this checkpoint as
   the product boundary.

### Complete Final Architecture Incrementally

1. Establish the shared domain, store, output, and CLI contracts.
2. Deliver active-session zero-poll execution and direct CLI execution.
3. Harden the single runner and receipt path.
4. Add local job operations.
5. Add the durable supervisor and ordered recovery.
6. Add installable Codex integration and diagnostics.
7. Add the structured adapter, cross-platform matrix, stress tests, docs, and
   full live validation.

## Notes

- `[P]` means different files and no dependency on incomplete work.
- `[US#]` maps directly to spec.md.
- No task may introduce a second command-execution backend.
- Every significant implementation slice ends with focused tests, a diff
  review, evidence, and a commit.
- Full completion requires all 103 tasks, not only the first checkpoint.

## Requirement Coverage

This table is the authoritative task mapping used by cross-artifact analysis.

| Coverage key | Tasks |
|--------------|-------|
| FR-001 | T001, T018, T038, T094, T098 |
| FR-002 | T003, T034, T036-T040 |
| FR-003 | T020, T024, T028-T029 |
| FR-004 | T009, T013, T020, T030, T037, T100 |
| FR-005 | T012, T034, T037 |
| FR-006 | T023, T025, T031-T033 |
| FR-007 | T022-T023, T031, T033 |
| FR-008 | T004, T014-T015, T035, T037 |
| FR-009 | T015, T022, T031, T099 |
| FR-010 | T015, T026, T031 |
| FR-011 | T052-T059 |
| FR-012 | T034, T044, T047-T049 |
| FR-013 | T009, T011, T013, T037 |
| FR-014 | T009, T011, T013, T063, T071 |
| FR-015 | T022, T063, T071, T100 |
| FR-016 | T020, T024, T030, T043, T050 |
| FR-017 | T020-T021, T024, T028, T030 |
| FR-018 | T008, T011, T017, T027, T034, T038 |
| FR-019 | T017, T027, T029, T038 |
| FR-020 | T036, T042, T046 |
| FR-021 | T010, T041, T045 |
| FR-022 | T023, T031, T037 |
| FR-023 | T061-T070 |
| FR-024 | T074-T075, T078, T088-T089 |
| FR-025 | T004, T013-T014, T058, T064, T069, T071 |
| FR-026 | T063, T071-T072 |
| FR-027 | T063, T072-T073 |
| FR-028 | T012, T063, T073 |
| FR-029 | T077-T089, T098 |
| FR-030 | T004, T021, T027, T078, T081, T084 |
| FR-031 | T021, T027-T028 |
| FR-032 | T095-T096 |
| FR-033 | T008, T011, T034, T036 |
| FR-034 | T009, T013-T014, T064, T069, T071 |
| FR-035 | T044, T047-T049, T062, T067-T068, T074, T097 |
| FR-036 | T010, T012, T098 |
| FR-037 | T004, T016, T079, T087, T089, T094, T098 |
| FR-038 | T078, T086, T089 |
| FR-039 | T090, T092-T094, T098 |
| FR-040 | T002, T091, T094, T098 |
| SC-001 | T023, T033, T101 |
| SC-002 | T020, T022, T063, T100 |
| SC-003 | T023, T031, T099 |
| SC-004 | T015, T031, T039, T099 |
| SC-005 | T044, T047-T049 |
| SC-006 | T020, T043, T050, T100 |
| SC-007 | T063-T065, T071-T073, T076 |
| SC-008 | T041-T043, T045-T046, T051 |
| SC-009 | T077-T079, T084-T089 |
| SC-010 | T003, T034, T036-T040, T097 |
| SC-011 | T052-T060, T070 |
| SC-012 | T078, T086, T089 |
| SC-013 | T090-T094 |
| Constitution quality gates | T005-T007, T019, T033, T040, T051, T060, T076, T089, T101-T103 |

## Phase 10: Convergence

- [X] T104 Re-run the trusted native Codex 90-second acceptance against the opaque receipt-handle path and record JSONL and SQLite proof in `specs/001-long-command-execution/implementation-log.md` per SC-001 and SC-003 (partial)
