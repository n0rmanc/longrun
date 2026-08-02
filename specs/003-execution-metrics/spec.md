# Feature Specification: Local Execution Metrics

**Feature Branch**: `003-execution-metrics`

**Created**: 2026-08-02

**Status**: Draft

**Input**: User description: "Add a local execution metrics and gain command for Longrun, similar to rtk gain. Provide a global local longrun gain report showing the number of recorded target command executions, total and average wait time, completion status counts, and basic per-program counts and durations. Support --json output and --clear to reset local metrics. Record both direct CLI and Codex hook executions after terminal completion without storing raw arguments, output, prompts, credentials, or request bodies. Do not add token-savings estimates, workers, daemons, SQLite job queues, retries, or recovery."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See Longrun Usage at a Glance (Priority: P1)

As a Longrun user, I want a global `longrun gain` report that tells me how many
target commands Longrun has recorded and how long they took, so I can see the
actual waiting workload instead of guessing from session history.

**Why this priority**: The primary value is visibility into command count and
elapsed time. Without a basic report, the feature does not answer the user's
core question.

**Independent Test**: Run several direct Longrun commands that reach terminal
results, then run `longrun gain` and verify that the report contains the
recorded execution count, total duration, average duration, and outcome counts.

**Acceptance Scenarios**:

1. **Given** recorded successful and unsuccessful target executions, **When** a
   user runs `longrun gain`, **Then** the report shows the total recorded
   executions, total waiting time, average duration, and a count for each
   recorded terminal outcome.
2. **Given** no recorded executions, **When** a user runs `longrun gain`,
   **Then** the command succeeds and shows zero values rather than failing or
   inventing data.
3. **Given** a target command is still running or its owning session ends
   before a terminal result is delivered, **When** a user runs `longrun gain`,
   **Then** that incomplete execution is not included in the report.

---

### User Story 2 - Understand Which Programs Consume the Wait (Priority: P2)

As a Longrun user, I want the report grouped by executable name, so I can see
which programs account for the most executions and elapsed time without
exposing command arguments or output.

**Why this priority**: The global totals answer "how much"; a per-program
breakdown makes the result actionable while keeping the stored data minimal.

**Independent Test**: Run targets from at least two executable names, then run
`longrun gain` and verify that each executable has its own count and duration
summary.

**Acceptance Scenarios**:

1. **Given** recorded executions for `gh`, `cargo`, and another executable,
   **When** a user runs `longrun gain`, **Then** the report lists each
   executable separately with its execution count and total and average
   duration.
2. **Given** the same executable is invoked with different arguments, **When**
   the report is generated, **Then** those executions are grouped under the
   executable name and their raw arguments are not displayed.

---

### User Story 3 - Automate and Reset the Report (Priority: P2)

As a Longrun user, I want machine-readable output and a way to clear local
history, so I can use the metrics in scripts and start a fresh measurement
period without deleting unrelated Longrun state.

**Why this priority**: JSON output supports automation, while clearing local
history keeps the report useful over time. Neither should change how target
commands execute.

**Independent Test**: Run `longrun gain --json`, validate the output with a
JSON parser, run `longrun gain --clear`, and verify that a subsequent report
contains zero recorded executions.

**Acceptance Scenarios**:

1. **Given** recorded metrics, **When** a user runs `longrun gain --json`,
   **Then** the output contains the same totals, outcome counts, and
   per-program data as the human-readable report in a machine-readable form.
2. **Given** recorded metrics, **When** a user runs `longrun gain --clear`,
   **Then** local metrics are reset, no target command is started, and a later
   `longrun gain` reports zero recorded executions.
3. **Given** a target command is running while the user clears metrics, **When**
   the clear operation completes before that target publishes its terminal
   metric, **Then** the clear operation does not stop the target and the later
   terminal result is recorded in the new measurement period.

### Edge Cases

- The report starts at zero when no local metrics history exists.
- A target that exits nonzero is counted as a terminal execution and is
  identified as unsuccessful; its exit code is retained in the recorded
  metadata when available.
- A timeout, cancellation, or handled owner shutdown is counted with its
  terminal outcome and duration.
- The internal receipt/stub used by the Codex hook must never appear as a
  target execution in the report.
- The `gain` management command itself must not be recorded as a target or
  routed through the long-command wait adapter; an external executable named
  `gain` remains invokable with the explicit `longrun -- gain` separator.
- Direct CLI executions and Codex hook executions contribute to the same local
  report.
- If a metrics entry is incomplete or unreadable, the report must not crash or
  fabricate a successful execution; it must ignore the invalid entry or
  surface a clear local-data warning.
- Clearing history must not remove unrelated Longrun configuration, hook
  installation state, or handoff state.
- Concurrent terminal completions must not make the report double-count or
  lose a completed execution.
- Clear has a defined ordering point: records published before the clear
  completes may be removed, while records published after it completes belong
  to the new measurement period.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Longrun MUST record one local metrics entry for each direct CLI
  target execution that reaches a terminal result.
- **FR-002**: Longrun MUST record one local metrics entry for each Codex hook
  target execution that reaches a terminal result, including nonzero exits,
  timeouts, cancellations, and handled owner shutdowns.
- **FR-003**: Longrun MUST NOT record the internal receipt/stub as a target
  execution and MUST NOT record an invocation that ends before Longrun receives
  a terminal result.
- **FR-004**: Each recorded entry MUST contain only a schema version, the
  executable name, elapsed duration, terminal outcome, optional exit code,
  execution mode, and completion time.
- **FR-005**: Longrun MUST NOT store raw command arguments, command output,
  working directory, prompts, credentials, request bodies, or other secret
  material in the metrics history.
- **FR-006**: `longrun gain` MUST report the total number of recorded target
  executions, total elapsed waiting time, average elapsed duration, and counts
  for successful completion, nonzero exit, timeout, cancellation, and handled
  owner shutdown when present.
- **FR-007**: `longrun gain` MUST provide a per-executable summary containing
  execution count, total elapsed duration, and average duration.
- **FR-008**: `longrun gain --json` MUST return valid machine-readable JSON with
  the same metric values and outcome categories as the human-readable report.
- **FR-009**: `longrun gain --clear` MUST remove all recorded metrics, MUST NOT
  start or stop a target command, and MUST leave unrelated Longrun state
  unchanged.
- **FR-010**: Metrics MUST remain local to the current machine; this feature
  MUST NOT synchronize execution history with a remote service.
- **FR-011**: Concurrent metric writes and reads MUST preserve valid records
  and MUST NOT double-count a terminal execution.
- **FR-012**: Longrun MUST NOT estimate or claim Codex/provider token savings
  from these metrics.
- **FR-013**: `longrun gain` and `rtk longrun gain` MUST remain management
  commands; a target executable whose name collides with `gain` MUST remain
  invokable through the explicit `longrun -- gain` separator.

### Key Entities

- **Execution Metric**: A local record of one target command that reached a
  terminal result. It identifies the executable, elapsed duration, terminal
  outcome, optional exit code, execution mode, and completion time.
- **Gain Report**: An aggregate view of execution metrics containing global
  totals, terminal-outcome counts, and per-executable summaries.
- **Metrics History**: The local collection of execution metrics that can be
  read by `longrun gain` and reset by `longrun gain --clear`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After 100 target executions reach terminal results, `longrun gain`
  reports exactly 100 recorded executions and the correct total and average
  durations.
- **SC-002**: The report contains separate outcome counts whose sum equals the
  total recorded execution count.
- **SC-003**: For a history containing at least three executable names, the
  report contains one per-executable summary for each name, and the
  per-executable counts sum to the global total.
- **SC-004**: `longrun gain --json` can be parsed by a standard JSON parser and
  expresses the same values as `longrun gain`.
- **SC-005**: After `longrun gain --clear` succeeds, the next `longrun gain`
  reports zero recorded executions while unrelated Longrun state remains
  usable.
- **SC-006**: Every direct or Codex-hook target execution that reaches a
  terminal result is represented exactly once, and no incomplete execution is
  represented.
- **SC-007**: The feature provides no token-savings percentage or provider
  request estimate; users can distinguish measured local wait metrics from
  unavailable provider usage data.

## Assumptions

- The report is global for the current user's local Longrun data directory, not
  scoped to one repository or one Codex session.
- The first version reports recorded executions, not every process ever
  started outside Longrun and not executions that terminate with owner loss
  before result delivery.
- Executable names are grouped by the invoked program name; arguments are never
  part of the grouping key or report.
- Local metric history may be retained until the user clears it; automatic
  retention windows and remote synchronization are out of scope.
- Existing runner and hook terminal results provide the authoritative duration,
  terminal outcome, and exit status.
- Token usage and request counts belong to the Codex/provider layer and are
  outside the data available to this feature.
