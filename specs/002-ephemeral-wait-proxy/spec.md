# Feature Specification: Ephemeral RTK-Style Wait Proxy

**Feature Branch**: `002-ephemeral-wait-proxy`

**Created**: 2026-08-01

**Status**: Draft

**Input**: User description: "Redesign Longrun as an RTK-style transparent wait
proxy for Codex. Users and agents invoke one generic command such as longrun gh
run watch RUN_ID --repo OWNER/REPO --exit-status or longrun oracle ...; the
active PostToolUse hook waits locally so Codex does not poll for status and the
final target exit code plus bounded untrusted output return to the same active
turn. Use only an ephemeral PreToolUse/PostToolUse handoff. Do not use a
durable supervisor, per-job worker process, completed-result persistence,
recovery after a dead session, automatic retry, or a second Longrun approval
prompt. When the hook owner ends on handled or observable shutdown, terminate
the owned process tree and require manual rerun. Codex remains responsible for
command approval and sandbox policy; Longrun must not reproduce that policy,
add a second permission gate, or launch a second sandbox. Preserve
direct-argument safety, inherited hook environment, bounded output, and
human/CI synchronous execution. Support both longrun PROGRAM ARG... and rtk
longrun PROGRAM ARG... without special CI or Oracle subcommands."

## User Scenarios & Testing

### User Story 1 - Wait for Long Commands Without Model Polling (Priority: P1)

As a Codex user, I can invoke one finite command that may run for minutes or
hours and have Codex continue the same active turn only after the command
finishes, without periodic status questions or model polling.

**Why this priority**: Eliminating wasted model turns and tokens is the primary
reason Longrun exists.

**Independent Test**: Invoke a controlled command that runs longer than the
normal shell-tool wait threshold, observe the Codex event stream for the whole
wait, and verify that one final result returns to the same active turn with no
status request, `write_stdin`, or second model continuation.

**Acceptance Scenarios**:

1. **Given** Longrun integration is installed and trusted, **when** an agent
   invokes `longrun` with a finite command, **then** the command starts through
   the local wait path and the agent makes no progress-polling request.
2. **Given** a command is still running, **when** several minutes pass, **then**
   the active Codex turn remains pending locally and no model request is made
   solely to check status.
3. **Given** the originating Codex session and active hook remain available,
   **when** the target exits, **then** the target result is delivered to that
   same active turn and the agent can continue its original work.
4. **Given** the target exits nonzero, **when** completion is delivered, **then**
   the agent receives the exact target exit status, terminal reason, bounded
   diagnostics, and no automatic retry.

### User Story 2 - Use One Transparent Command Surface (Priority: P1)

As a developer, CI operator, or coding agent, I can use the same generic
Longrun command surface for GitHub Actions waits, Oracle reviews, tests, and
other finite commands without learning a submission protocol or special
subcommands.

**Why this priority**: A transparent proxy is easier to learn and keeps the
Codex integration as a thin adapter.

**Independent Test**: Run successful and failing commands through both direct
terminal execution and the Codex integration, using `longrun PROGRAM ARG...`
and `rtk longrun PROGRAM ARG...`, and verify that arguments and final statuses
are preserved.

**Acceptance Scenarios**:

1. **Given** Longrun is installed, **when** a user runs
   `longrun PROGRAM ARG...` in a terminal or CI job, **then** Longrun waits for
   the target and returns the target's final status.
2. **Given** a repository uses RTK, **when** an agent invokes
   `rtk longrun PROGRAM ARG...`, **then** Longrun receives the same target
   program and arguments without an extra `submit` or shell wrapper.
3. **Given** a user watches a GitHub Actions run, **when** the wrapped wait
   command completes, **then** the final CI status and bounded output are
   returned without requiring a special Longrun CI command.
4. **Given** a user runs an Oracle browser review, **when** the wrapped Oracle
   command completes, **then** its final status and bounded output are returned
   without Longrun reattaching, retrying, or starting another Oracle run.

### User Story 3 - Keep Execution Ephemeral and Owned (Priority: P1)

As a user, I want a command to belong to the active wait invocation rather than
become a background job that survives a dead Codex session.

**Why this priority**: The product intentionally avoids durable job management,
recovery behavior, and stale results that no longer have a useful destination.

**Independent Test**: Start a long-running command, end the active hook through
each handled shutdown path, and verify that the owned process tree is
terminated, no result is later recovered, and a manual rerun is required.

**Acceptance Scenarios**:

1. **Given** a target is running under the active wait invocation, **when** the
   hook receives an orderly shutdown, timeout, or cancellation, **then** the
   entire owned process tree is terminated and no recovery is scheduled.
2. **Given** a target has completed, **when** the active delivery cannot return
   its result, **then** Longrun does not persist or redeliver the completed
   result and the user must rerun the command.
3. **Given** two hook events refer to the same handoff, **when** the first event
   claims it, **then** the second event starts no target.
4. **Given** a handoff is missing, expired, malformed, mismatched, or already
   claimed, **when** a hook receives it, **then** no target starts.

### User Story 4 - Respect Codex Boundaries and Bound Context (Priority: P1)

As a security-conscious user, I want Longrun to wait for commands without
creating a second approval system, sandbox, or environment policy.

**Why this priority**: Avoiding polling is not useful if the waiting proxy
weakens Codex's execution boundary or floods the model context with output.

**Independent Test**: Run an approved hook command with inherited environment
and shell composition expressed as explicit target arguments, then generate
output larger than the configured result budget; verify that Longrun does not
add a sandbox or filter and still returns bounded untrusted result data.

**Acceptance Scenarios**:

1. **Given** Codex has approved and invoked a Longrun command, **when**
   PostToolUse starts the target, **then** Longrun launches the target directly
   with the captured argv, cwd, and hook-inherited environment.
2. **Given** a target needs filesystem, network, or credential access, **when**
   it runs, **then** Codex's already-selected execution boundary and the hook
   environment remain the only external policy inputs; Longrun adds no
   escalation, filtering, or second prompt.
3. **Given** the hook environment contains a variable, **when** the target
   starts, **then** Longrun does not remove or selectively pass that variable.
4. **Given** the target emits very large, binary, or instruction-like output,
   **when** completion is delivered, **then** the model receives bounded escaped
   tails, byte counts, truncation metadata, and an untrusted-output marker.

### User Story 5 - Install and Upgrade the Thin Integration (Priority: P2)

As a user, I can install or repair Longrun's Codex integration without
installing a background service or leaving obsolete durable hooks active.

**Why this priority**: The new runtime contract must be reflected in installed
assets, upgrades, and documentation rather than only in Rust internals.

**Independent Test**: Install the integration into an isolated Codex home,
inspect its hooks and skill, repair it after replacing the binary, and verify
that obsolete recovery/service assets are removed without touching unrelated
configuration.

**Acceptance Scenarios**:

1. **Given** Longrun is installed, **when** the user initializes Codex
   integration, **then** only the thin PreToolUse/PostToolUse wait integration
   and its skill are installed using the absolute executable path.
2. **Given** an older Longrun integration is present, **when** the user runs
   repair, **then** obsolete SessionStart, supervisor, service, and submit-only
   guidance are removed or replaced with the new command surface.
3. **Given** the user never explicitly requests a service, **when** integration
   installation completes, **then** no operating-system service is installed or
   started.
4. **Given** the user invokes a removed durable command, **when** Longrun
   parses it, **then** it returns a clear migration error instead of silently
   creating a durable job.

## Edge Cases

- The target exits before `PostToolUse` begins waiting.
- The target exits with zero, one, two, four, or another nonzero status.
- The receipt marker is truncated, duplicated, altered, or mixed with unrelated
  shell output.
- Two different sessions create handoffs at nearly the same time.
- A duplicate `PostToolUse` invocation arrives after the handoff was claimed.
- The target emits invalid UTF-8, binary bytes, continuous output, or fake hook
  JSON and instructions.
- The target spawns children or grandchildren that ignore graceful termination.
- The target leader exits while descendants remain.
- The hook receives timeout, SIGINT, SIGTERM, SIGHUP, or orderly cancellation.
- The hook process is forcibly killed on Unix/macOS and cannot perform cleanup.
- Windows process containment assignment fails before the target is resumed.
- The configured timeout leaves no margin for cleanup and result serialization.
- The working directory disappears before the target starts.
- The hook environment lacks credentials required by `gh` or Oracle.
- Codex approval is missing or the installed hook is not trusted.
- Oracle creates its own browser/session artifacts outside Longrun's state.
- An old SQLite database, completed result, service, or recovery record remains
  from a prior Longrun version.
- The target program name collides with a reserved Longrun management command.
- A user includes shell separators, redirection, command substitution, `sudo`,
  `env`, aliases, or another unsupported wrapper.

## Requirements

### Functional Requirements

- **FR-001**: Longrun MUST accept `longrun PROGRAM ARG...` as its canonical
  generic target command.
- **FR-002**: Codex integration MUST accept `rtk longrun PROGRAM ARG...` and
  preserve the target program and native arguments.
- **FR-003**: Longrun MUST provide synchronous human and CI execution that
  returns the target's final exit status.
- **FR-004**: Codex PreToolUse MUST recognize only the exact Longrun forms and
  MUST leave unrelated shell commands untouched.
- **FR-005**: Codex integration MUST reject shell composition, command
  substitution, redirection, background execution, unsupported wrappers, and
  multiple Longrun invocations in one shell command.
- **FR-006**: The integration MUST create only a protected handoff containing
  the origin identity, target arguments, execution limits, and expiry. The
  expiry MUST use a configured `handoff_ttl_ms` with a default of 300000 ms and
  a maximum of 900000 ms.
- **FR-007**: The handoff MUST transition once from prepared to armed to claimed
  and MUST be deleted after the active invocation finishes.
- **FR-008**: A missing, expired, malformed, mismatched, duplicated, or already
  claimed handoff MUST start no target.
- **FR-009**: The receipt stub MUST not start the target and MUST complete
  within 1000 ms in the deterministic hook harness. PostToolUse MUST claim the
  handoff and execute the target locally without issuing model status requests
  or starting a second Codex process.
- **FR-010**: The active wait path MUST return the final target result to the
  originating active Codex turn when that hook remains available.
- **FR-011**: The result envelope MUST include the exact target exit status,
  terminal reason, duration, stdout/stderr byte counts, truncation metadata,
  bounded escaped tails, and an explicit untrusted-output marker. The generated
  Codex hook MUST set `additionalContextLimit` to `0` because Longrun already
  bounds the envelope; Codex MUST NOT spill the result to a hook-output file.
- **FR-012**: Longrun MUST NOT persist completed result rows, delivery records,
  recovery state, or Longrun-owned stdout/stderr logs after the active
  invocation completes.
- **FR-013**: Longrun MUST NOT automatically retry, resume, or redeliver a
  command or result after hook/session loss; the user MUST manually rerun it.
- **FR-014**: On timeout, cancellation, handled signal, or observable owner
  shutdown, Longrun MUST terminate the entire owned process tree within the
  configured grace period or force-kill it.
- **FR-015**: Unix implementations MUST use process-group ownership and
  explicitly document the best-effort limitation for uncatchable macOS/Unix
  owner death.
- **FR-016**: Windows implementations MUST contain the target in a kill-on-close
  Job Object before resuming it.
- **FR-017**: Codex-hook execution MUST launch the captured target directly
  through the shared native runner in the hook's inherited environment.
  Longrun MUST NOT invoke `codex sandbox`, create a second sandbox, or require
  a Codex installation for direct terminal/CI execution.
- **FR-018**: Longrun MUST NOT add a second user approval prompt, permission
  gate, environment filter, or permission-escalation path. Codex approval and
  sandbox policy remain outside Longrun's execution logic.
- **FR-019**: Longrun MUST preserve the target's captured argv, canonical cwd,
  and hook-inherited environment without clearing, redacting, or selectively
  passing variables.
- **FR-020**: Direct argument mode MUST pass arguments literally and Longrun
  MUST NOT evaluate outer-shell syntax. Users who explicitly need shell
  semantics MUST invoke the shell as the target program, for example
  `longrun /bin/sh -c '...'`.
- **FR-021**: Codex integration MUST install only the active wait hooks and MUST
  not install SessionStart recovery hooks, durable services, or supervisors.
- **FR-022**: Repair MUST remove obsolete durable integration assets and old
  submit-only guidance without modifying unrelated Codex configuration.
- **FR-023**: Removed durable command names MUST remain reserved and return a
  clear migration error; they MUST NOT silently become target programs or
  create a durable job. A real target with one of those names MUST be
  invokable through the explicit separator.
- **FR-024**: The integration MUST configure a PostToolUse timeout using the
  explicit formula
  `post_tool_use_timeout_ms >= target_timeout_ms + termination_grace_ms +
  forced_cleanup_margin_ms + result_serialization_margin_ms`; configuration
  diagnostics MUST report all four component values and reject a timeout that
  does not satisfy the formula.

### Key Entities

- **Ephemeral Handoff**: A one-way record connecting one visible Longrun
  invocation to one PostToolUse execution. It is governed by the configured TTL
  and contains origin identity, native target arguments, execution snapshot,
  expiry, and claim state.
- **Target Execution**: The single locally owned process-tree attempt launched
  after a handoff is claimed. It has one terminal reason and one target exit
  status when the operating system provides one.
- **Result Envelope**: The bounded, escaped, untrusted data returned to the
  active Codex turn or printed for a direct terminal/CI invocation.
- **Execution Snapshot**: The target argv, canonical cwd, timeout,
  termination, forced-cleanup, result-serialization, and output limits captured
  for the handoff. Codex approval and sandbox policy are not Longrun fields.

## Success Criteria

### Measurable Outcomes

- **SC-001**: In a controlled run lasting at least 125 seconds, the active
  Codex turn receives no periodic status request, `write_stdin`, or model
  continuation before the target finishes.
- **SC-002**: For successful and failing controlled targets, the final result
  reaches the original active turn in the same PostToolUse lifecycle, with no
  second Codex process or automatic rerun.
- **SC-003**: Across at least 100 sequential and concurrent duplicate-hook
  attempts, each handoff starts its target no more than once and every rejected
  duplicate starts zero targets.
- **SC-004**: After success, failure, timeout, cancellation, and lost delivery,
  a Longrun state inspection finds no completed job, result, delivery lease,
  recovery record, supervisor endpoint, worker process, or Longrun-owned output
  log for that invocation.
- **SC-005**: In all handled timeout, cancellation, SIGINT, SIGTERM, SIGHUP,
  and orderly owner-shutdown tests, every owned descendant is terminated within
  `termination_grace_ms + forced_cleanup_margin_ms`, where both values come
  from the immutable execution policy.
- **SC-006**: For output at least 10 times larger than the configured model
  result budget, peak retained Longrun output remains bounded by the configured
  rolling-buffer limits, the model receives only bounded escaped tails, and no
  Codex hook-output spill file is created.
- **SC-007**: Across hook executions with present and absent credentials,
  filesystem/network operations, and varied Codex approval outcomes, Longrun
  never invokes a second sandbox, adds a permission gate, filters the hook
  environment, or retries with a broader policy.
- **SC-008**: Direct terminal/CI runs return the target's exact success or
  nonzero exit status for all supported normal exits; Codex result envelopes
  report the same status even though the receipt stub has already exited.
- **SC-009**: Both `longrun PROGRAM ARG...` and `rtk longrun PROGRAM ARG...`
  preserve target argv for GitHub Actions, Oracle, and generic fixture
  commands.
- **SC-010**: A repair in an isolated Codex home leaves no active Longrun
  SessionStart recovery hook, service artifact, or submit-only skill guidance.
- **SC-011**: A controlled GitHub Actions watch and a controlled Oracle browser
  run each return one final bounded result to the active turn without Longrun
  polling, reattachment, or a second target invocation.
- **SC-012**: Documentation and diagnostics explicitly state that uncatchable
  Unix/macOS owner death is best effort and that manual rerun is required
  after lost ownership.

## Assumptions

- The user has already trusted the generated Codex command hooks; Codex owns
  approval and sandbox decisions, while the trusted adapter launches the exact
  captured target directly in the hook environment.
- The Codex environment supports synchronous command hooks with a configured
  timeout long enough for the target and cleanup margin.
- Longrun does not infer or reproduce transient Codex approval state; it relies
  on Codex's hook execution boundary and does not add another one.
- GitHub Actions and Oracle are invoked through their normal CLIs; Longrun
  provides no special CI or Oracle protocol.
- A wrapped tool may retain its own artifacts; Longrun owns only the ephemeral
  handoff and active result.
- A lost hook/session does not trigger automatic recovery or retry; the user
  accepts manual rerun and any resulting duplicate side effects.
- Hard, uncatchable Unix/macOS process death cannot guarantee zero orphaned
  descendants without an external process owner, which is out of scope.
- The existing Rust command runner directly executes the target for both
  terminal/CI and Codex hooks; no second execution backend or sandbox wrapper
  is introduced.
