# Feature Specification: Long-Running Command Execution

**Feature Branch**: `main`

**Created**: 2026-07-31

**Status**: Draft

**Input**: Build Longrun as a standalone command-line product and Codex
integration that runs finite long-running commands without repeated model
polling, preserves security boundaries, and safely continues the originating
work when execution completes.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Run Long Commands Without Model Polling (Priority: P1)

As a Codex user, I can submit a finite, non-interactive command that may run for
minutes or hours and have Codex continue the same work only after the command
finishes, without periodic model requests while it is running.

**Why this priority**: Eliminating repeated polling and token waste is the
primary reason Longrun exists.

**Independent Test**: Submit a command that runs for at least five minutes,
observe that it starts once and causes no periodic model activity, and verify
that the originating Codex work continues with the command result after
completion.

**Acceptance Scenarios**:

1. **Given** Longrun is installed and Codex integration is active, **When** an
   agent submits a finite long-running command, **Then** the command starts once
   and the agent does not poll for progress.
2. **Given** a submitted command is still running, **When** several minutes
   pass, **Then** no model continuation request is made solely to check status.
3. **Given** the originating Codex process remains active, **When** the command
   finishes, **Then** its bounded result is delivered to the same active work
   and the agent continues the original task.
4. **Given** a command exits unsuccessfully, **When** completion is delivered,
   **Then** the agent receives the exit status, bounded diagnostics, and local
   log locations without an automatic retry.

---

### User Story 2 - Use Longrun Directly from a Terminal (Priority: P1)

As a developer or CI operator, I can use Longrun directly to execute a command,
wait for it, observe its output, and receive the same final success or failure
status as the child command.

**Why this priority**: Longrun is a standalone product; Codex integration is an
adapter rather than the only way to use it.

**Independent Test**: Run successful and failing commands through
`longrun run`, verify output behavior, and verify that Longrun returns the same
exit status as each command.

**Acceptance Scenarios**:

1. **Given** Longrun is installed, **When** a user runs
   `longrun run -- PROGRAM ARG...`, **Then** Longrun executes the command,
   waits for completion, and exits with the child command's status.
2. **Given** a command writes to both output streams, **When** it completes,
   **Then** the user can distinguish normal output from diagnostics.
3. **Given** a command exceeds its configured timeout, **When** the timeout
   elapses, **Then** Longrun terminates the owned process tree and reports a
   timed-out result.

---

### User Story 3 - Inspect and Control Jobs (Priority: P2)

As a user, I can inspect job state, read or follow logs, wait for completion,
list jobs, cancel a running job, and remove expired job data.

**Why this priority**: Long-running work must remain observable and controllable
without asking an agent to poll it.

**Independent Test**: Start a durable job, inspect its state and logs from a
second terminal, cancel it, and verify that the job and its descendants stop
and reach one terminal state.

**Acceptance Scenarios**:

1. **Given** a known job identifier, **When** a user requests status, **Then**
   Longrun returns its current execution and delivery states.
2. **Given** a running job, **When** a user follows its logs, **Then** new output
   is visible without changing the job state or model context.
3. **Given** a running job, **When** a user cancels it, **Then** Longrun
   terminates the entire owned process tree and records a cancelled result.
4. **Given** completed jobs older than the configured retention period,
   **When** garbage collection runs, **Then** eligible state and logs are
   removed without affecting active or undelivered jobs.

---

### User Story 4 - Recover Completed Work Safely (Priority: P2)

As a Codex user, I can choose durable execution so a command survives the
originating Codex process, and its result is delivered effectively once when
the session is available again.

**Why this priority**: Commands lasting hours must not be lost when a terminal
or Codex process closes, but recovery must never duplicate execution or result
delivery.

**Independent Test**: Start a durable job, terminate the originating Codex
process, let the job complete, reopen the same session, and verify one stable
recovery identity with no second command execution or duplicate resume process.

**Acceptance Scenarios**:

1. **Given** durable mode is enabled, **When** the originating Codex process
   exits while a job runs, **Then** the job continues under the local
   supervisor.
2. **Given** a durable job completed while its original delivery owner was
   unavailable, **When** the same session starts again, **Then** Longrun
   delivers the result with a stable idempotency identity.
3. **Given** an active delivery owner still holds the job, **When** another
   recovery path checks the same result, **Then** it does not compete for or
   duplicate delivery.
4. **Given** optional automatic session resume is disabled, **When** a durable
   job completes without an active session, **Then** Longrun stores the result
   without starting a new Codex process.

---

### User Story 5 - Install and Manage Longrun (Priority: P2)

As a user, I can install the Longrun CLI without first installing a Rust
toolchain, then install, diagnose, repair, and uninstall its Codex integration.

**Why this priority**: Integration must be reproducible and must not depend on a
particular shell environment or silently install privileged services.

**Independent Test**: Install a verified platform binary on a machine without a
Rust toolchain, install the integration, start a new Codex session, verify that
Longrun is discoverable and healthy, repair it after moving the executable, and
uninstall it without removing unrelated Codex configuration.

**Acceptance Scenarios**:

1. **Given** a supported machine without a Rust toolchain, **When** a user
   selects a published binary installation method, **Then** Longrun installs
   from a verifiable platform artifact and checksum.
2. **Given** the Longrun CLI is installed, **When** a user initializes Codex
   integration, **Then** the required plugin, skill, and lifecycle integration
   are installed using the actual executable location.
3. **Given** Codex integration is installed, **When** the executable moves,
   **Then** repair updates Longrun-owned integration without rewriting unrelated
   user configuration.
4. **Given** a user requests integration removal, **When** uninstall completes,
   **Then** only Longrun-owned integration artifacts are removed.
5. **Given** durable mode has never been explicitly enabled, **When** Codex
   integration is installed, **Then** no operating-system background service is
   installed or started.

---

### User Story 6 - Preserve Execution Security (Priority: P1)

As a security-conscious user, I can trust that moving a command into Longrun
does not silently grant it more filesystem, network, environment, or shell
permissions than configured.

**Why this priority**: A wrapper that bypasses the originating security boundary
would be unsafe even if it eliminated polling.

**Independent Test**: Attempt workspace-external writes, denied network access,
secret reads, forged submissions, and command replay; verify that all are
rejected without running with elevated permissions.

**Acceptance Scenarios**:

1. **Given** a command requests access outside its permission profile, **When**
   it executes, **Then** it fails without automatic escalation.
2. **Given** protected credentials exist in the parent environment, **When** a
   job starts without explicit permission to inherit them, **Then** they are not
   present in the child environment.
3. **Given** a forged, expired, mismatched, replayed, or consumed submission
   receipt, **When** Longrun validates it, **Then** no command starts.
4. **Given** direct argument mode, **When** arguments contain shell
   metacharacters, **Then** they are passed as literal arguments rather than
   evaluated as shell syntax.

### Edge Cases

- The submitted command completes before completion waiting begins.
- The submission command succeeds but its receipt is truncated, malformed, or
  mixed with unrelated output.
- The same completion hook or recovery action is retried after a crash.
- The process creates children or grandchildren that ignore graceful shutdown.
- The process exits because of a platform signal or forced termination.
- Output is empty, binary, non-UTF-8, extremely large, or continuously written
  to both output streams.
- The working directory is removed or becomes inaccessible before execution.
- The local clock changes between submission, execution, and receipt expiry.
- The state store fills the disk or becomes temporarily unwritable.
- Two sessions submit jobs concurrently and complete at nearly the same time.
- The supervisor stops after command completion but before result persistence.
- The original Codex process closes while embedded execution is active.
- A user moves or replaces the Longrun executable after installing integration.
- A user requests shell execution without explicitly enabling shell mode.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Longrun MUST provide a standalone command-line interface usable
  independently of Codex.
- **FR-002**: Longrun MUST provide a direct execution mode that waits for a
  command and returns its final status.
- **FR-003**: Longrun MUST provide an agent submission mode that records a job
  request and returns quickly without executing the requested command directly.
- **FR-004**: Each accepted submission MUST start its requested command at most
  once.
- **FR-005**: Longrun MUST support finite, non-interactive commands with
  configurable timeouts.
- **FR-006**: Codex integration MUST wait locally for submitted command
  completion without periodic model polling.
- **FR-007**: When the originating Codex work remains active, completion MUST be
  delivered back to that same work.
- **FR-008**: Longrun MUST retain complete normal output and diagnostics in
  separate local job logs.
- **FR-009**: Model-visible completion results MUST be bounded by a configurable
  byte limit and include truncation status and log locations.
- **FR-010**: Command output delivered to an agent MUST be identified as
  untrusted data.
- **FR-011**: Longrun MUST expose job wait, status, list, logs, cancel, and
  garbage-collection operations.
- **FR-012**: Timeout and cancellation MUST terminate the entire owned process
  tree within a bounded grace period.
- **FR-013**: Every job MUST reach exactly one terminal execution state:
  succeeded, failed, timed out, or cancelled.
- **FR-014**: Execution state and result-delivery state MUST be recorded
  separately.
- **FR-015**: Retrying result delivery MUST NOT re-execute the command.
- **FR-016**: Longrun MUST reject forged, expired, mismatched, replayed, and
  already-consumed submission receipts without executing a command.
- **FR-017**: Submission validation MUST bind a request to its originating
  session, turn, tool invocation, working directory, command, and freshness.
- **FR-018**: Longrun MUST default to direct program-and-argument execution
  without shell evaluation.
- **FR-019**: Compound shell execution MUST require a distinct, explicit
  shell-enabled operation.
- **FR-020**: Longrun MUST execute jobs under an explicit permission profile and
  MUST fail rather than automatically escalate denied access.
- **FR-021**: Protected credentials and secret-like environment values MUST be
  excluded from jobs unless explicitly allowed by the user.
- **FR-022**: Longrun MUST support an embedded mode in which the active
  completion waiter owns the process.
- **FR-023**: Longrun MUST support a durable mode in which an explicitly enabled
  local supervisor owns jobs independently of the originating Codex process.
- **FR-024**: Installing Codex integration MUST NOT install or start durable
  background services without explicit user action.
- **FR-025**: Durable completion results MUST remain available until delivered
  or removed according to retention policy.
- **FR-026**: Only one active owner MAY hold delivery rights for a job and
  session at a time.
- **FR-027**: Recovery MUST prefer the original active completion waiter, then
  session-start delivery, then optional automatic session resume.
- **FR-028**: Automatic session resume MUST be disabled by default and bounded
  by an explicit retry limit.
- **FR-029**: Longrun MUST provide Codex integration installation, diagnostics,
  repair, and uninstall operations.
- **FR-030**: Codex integration MUST use the installed executable's resolved
  location rather than depending on an ambient command search path.
- **FR-031**: Integration hooks MUST ignore unrelated command executions and
  MUST NOT auto-approve or rewrite them.
- **FR-032**: Any additional agent or structured-tool interface MUST reuse the
  same job execution and state authority.
- **FR-033**: Longrun MUST preserve native command arguments without lossy text
  or shell reconstruction.
- **FR-034**: Durable state updates MUST survive interruption without producing
  an impossible job or delivery state.
- **FR-035**: Longrun MUST support correct process-tree ownership and
  cancellation on macOS, Linux, and Windows.
- **FR-036**: Users MUST be able to configure execution timeout, permission
  profile, output bounds, environment allowlist, recovery behavior, concurrency,
  and retention.
- **FR-037**: Longrun MUST provide health diagnostics for executable access,
  state storage, integration configuration, supervisor availability, and
  supported platform behavior.
- **FR-038**: Installation and removal MUST preserve unrelated user files and
  Codex configuration.
- **FR-039**: Longrun MUST publish installable, checksummed binaries for every
  supported platform so ordinary users do not need a Rust toolchain.
- **FR-040**: Longrun MUST also support source installation through the standard
  Rust package workflow for developers.

### Key Entities

- **Job Specification**: Immutable requested command, arguments, working
  directory, timeout, permission profile, environment policy, creation time,
  and protocol version.
- **Submission Receipt**: Single-use proof linking a job specification to its
  originating Codex context and freshness constraints.
- **Job Execution**: Process ownership and state from acceptance through one
  terminal result.
- **Job Result**: Exit status, termination reason, duration, output metadata,
  log locations, and integrity information.
- **Delivery Record**: Independent state describing whether, how, and to which
  session a completed result has been leased or delivered.
- **Delivery Lease**: Time-bounded exclusive ownership preventing concurrent
  completion or recovery delivery.
- **Integration Installation**: Longrun-owned Codex plugin, skill, hook, and
  resolved executable-location metadata.
- **Supervisor**: Optional explicitly enabled local owner for durable jobs and
  completion events.
- **Configuration**: User policy for execution, output, environment, recovery,
  retention, and diagnostics.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: During a 30-minute submitted command, zero periodic model requests
  are made solely to check whether the command has completed.
- **SC-002**: In 100 repeated delivery, restart, and replay attempts, each
  accepted job starts no more than once.
- **SC-003**: When the originating Codex work remains active, completion becomes
  available to that work within two seconds of local process termination.
- **SC-004**: Model-visible completion data never exceeds the configured byte
  limit, while complete output remains available through local logs.
- **SC-005**: Cancellation and timeout tests leave zero owned descendant
  processes running after the configured termination grace period.
- **SC-006**: All forged, expired, mismatched, replayed, and consumed receipt
  tests result in zero command executions.
- **SC-007**: A durable job survives termination of the originating Codex
  process; every recovery attempt uses one stable delivery idempotency identity,
  starts at most one resume process, and never re-executes the job.
- **SC-008**: Denied filesystem, network, secret, and elevated-permission tests
  complete without silent permission escalation.
- **SC-009**: A new user with the CLI already installed can initialize and
  validate Codex integration in under five minutes.
- **SC-010**: Successful and failing direct commands return the child command's
  exit status in all supported-platform conformance tests.
- **SC-011**: Status, logs, wait, cancellation, and cleanup operations remain
  usable from a separate terminal while a durable job is running.
- **SC-012**: Reinstall, repair, and uninstall tests leave all unrelated user
  configuration unchanged.
- **SC-013**: A user on each supported platform can install a verified Longrun
  binary and run `longrun doctor` without installing a Rust toolchain.

## Assumptions

- Longrun is intended for finite, non-interactive commands such as builds, test
  suites, migrations, benchmarks, and deployments.
- Interactive prompts, terminal user interfaces, password entry, and indefinite
  servers require a different execution path and are not submitted as ordinary
  Longrun jobs.
- Users explicitly choose any permission profile, environment secret, shell
  mode, durable service, or automatic resume behavior beyond safe defaults.
- The machine has enough local storage for configured job retention; storage
  exhaustion is reported as a job or system failure rather than silently
  discarding evidence.
- Codex integration capabilities may vary by installed Codex version; health
  diagnostics report incompatibility before accepting integrated submissions.
- The command being executed and its repository output are untrusted.
