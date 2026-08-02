# Research: Ephemeral RTK-Style Wait Proxy

## Decision 1: Make PostToolUse the only active wait owner

**Decision**: Keep one ephemeral execution path. PreToolUse prepares a short-lived
handoff; the receipt stub exits promptly; PostToolUse claims the handoff,
executes the target locally, waits, and returns the final result to the active
turn.

**Rationale**:

- The existing hook skill already identifies the useful behavior: PostToolUse
  waits locally and returns a bounded result without model polling.
- The user does not need durable jobs, recovery, or result delivery after the
  originating session is gone.
- Removing the supervisor, per-job worker, delivery leases, and recovery paths
  makes the execution owner equal to the active wait owner.

**Alternatives considered**:

- A durable supervisor with per-job workers: rejected because it intentionally
  keeps commands alive after the session and creates results with no required
  destination.
- Running the target directly inside the original Bash tool: rejected because
  the original tool wait can end before a multi-minute command completes.
- A second Codex process for recovery: rejected because normal completion must
  continue the original active turn, not create another model process.

## Decision 2: Use an RTK-style transparent command surface

**Decision**: The canonical command is `longrun PROGRAM ARG...`; the Codex
integration also accepts `rtk longrun PROGRAM ARG...`. Longrun does not expose
`submit`, `submit-shell`, CI-specific commands, or Oracle-specific commands in
the normal user workflow.

**Rationale**:

- The RTK repository describes its integration as a transparent Bash rewrite:
  the hook changes a command to an `rtk` equivalent, then the agent executes the
  rewritten command and receives compact output.
- RTK hook artifacts are thin delegates; rewrite decisions live in the Rust
  binary rather than in each integration.
- A generic passthrough keeps GitHub Actions, Oracle, tests, builds, and future
  finite commands on one path.

**Alternatives considered**:

- `longrun submit -- PROGRAM ARG...`: rejected because the internal handoff
  protocol leaked into the user-facing interface.
- `longrun gh watch cicd` and `longrun oracle review`: rejected because special
  subcommands duplicate existing CLIs and add command surface without value.

**Evidence**:

- GitHub source inspected: `rtk-ai/rtk`, branch `develop`, current README and
  `hooks/README.md`.
- The inspected RTK `develop` head was `e0ffd40ef7c450489aca4a50c0ab1358e4375691`;
  its technical guide keeps hook logic thin and delegates rewrite behavior to
  the Rust binary.
- RTK repository facts used in planning: transparent PreToolUse rewrite, thin
  hook delegates, and output filtering before model context.

## Decision 3: Keep only a short-lived, atomically claimed handoff

**Decision**: Replace durable job, execution, result, delivery, and receipt
  persistence with one protected ephemeral handoff. Its lifecycle is:
  `prepared -> armed -> claimed -> deleted`.

**Rationale**:

- PreToolUse, the rewritten Bash stub, and PostToolUse are separate processes;
  some cross-process state is unavoidable.
- A one-time opaque handle plus origin binding prevents two concurrent hooks
  from claiming the same command without retaining a durable job system.
- A crash after claim is intentionally lost and requires a manual rerun; it
  must not trigger an automatic retry.

**Alternatives considered**:

- Self-contained signed receipts with no local state: rejected because
  concurrent or replayed PostToolUse calls could both validate and execute.
- SQLite durable state: rejected because completed jobs, recovery, leases, and
  retention are explicitly out of scope.
- In-memory state only: rejected because PreToolUse and PostToolUse are
  separate processes and need a short-lived handoff.

## Decision 4: Execute through the configured Codex sandbox without a second approval

**Decision**: Codex-hook execution must use an explicitly configured named
permission profile, include managed requirements when the sandbox launcher
supports them, fail closed if the profile is unavailable or disallowed, and
never widen it automatically. Direct terminal/CI execution does not require a
Codex installation. Longrun must not install a PermissionRequest hook or
present a second approval prompt.

**Rationale**:

- The trusted Codex hook and configured profile are the authorization boundary;
  Longrun owns only the local wait and process lifecycle and does not claim a
  separate transient approval for the hidden target.
- Hook input does not expose enough information to claim exact inheritance of a
  transient approval decision or every active sandbox rule.
- The Codex runner must not turn a normal approved wrapper into an implicit
  `:danger-full-access` execution.

**Alternatives considered**:

- Automatically selecting `:danger-full-access` for GitHub and Oracle:
  rejected because it violates the repository security boundary.
- Claiming to inherit the exact active Codex permission state: rejected because
  the hook input contract does not expose that state.

## Decision 5: Bound output in memory and return the target status as data

**Decision**: Drain stdout and stderr concurrently into rolling bounded buffers,
  return byte counts, truncation, terminal reason, duration, exact target exit
  status, bounded tails, and an untrusted-output marker. Do not retain
  Longrun-owned completed logs or result records.

**Rationale**:

- The receipt stub exits before PostToolUse runs the target, so the target exit
  code cannot retroactively become the Bash stub's process exit code.
- Bounded capture prevents progress output from consuming model context.
- Codex may spill oversized hook `additionalContext` to a temporary file; the
  generated Longrun hook sets `additionalContextLimit` to `0`, while Longrun's
  own result cap remains the bound.
- Wrapped tools such as Oracle may retain their own artifacts; Longrun does not
  treat those artifacts as its recoverable state.

**Alternatives considered**:

- Persisting complete stdout/stderr and returning log paths: rejected because
  the redesigned product has no post-session result consumer.
- Returning only a success/failure sentence: rejected because CI and Oracle
  failures need the exact status and bounded diagnostics.

## Decision 6: Make process ownership explicit and platform-specific

**Decision**: The PostToolUse owner must terminate the target process tree on
  timeout, cancellation, handled signal, and observable owner shutdown.
  Unix uses a dedicated process group; Windows uses a Job Object with
  kill-on-close and starts the child suspended until containment is assigned.

**Rationale**:

- The current Unix code creates a process group and can terminate it, but
  process-group creation alone does not make children die when a parent is
  uncatchably killed.
- Context7's Tokio documentation confirms that `Child::kill_on_drop(true)`
  kills the direct child handle when it is dropped; it does not replace
  process-group or Job Object cleanup for descendants.
- The installed Codex CLI reports `--permission-profile`,
  `--include-managed-config`, and `--cd` on `codex sandbox`; the plan therefore
  requires managed-profile resolution and fails closed when the required
  launcher option is unavailable.
- The current Codex hooks contract supports synchronous command hooks with an
  explicit timeout; PostToolUse feedback can replace the completed tool result
  and continue the active turn, while asynchronous command hooks are not a
  substitute for this wait path.
- macOS cannot guarantee zero descendants after an uncatchable `SIGKILL` of the
  sole owner without an external owner. That is documented as best effort.

**Alternatives considered**:

- Keep the detached worker/supervisor: rejected because it contradicts the
  session-owned product contract.
- Promise zero-orphan cleanup after every macOS hard kill: rejected as
  impossible under the no-external-owner constraint.
- Rely only on Tokio child drop: rejected because descendant cleanup still
  needs platform process-tree ownership.

## Decision 7: Use the real CLI contracts for GitHub and Oracle acceptance

**Decision**: Validate generic passthrough with `gh run watch RUN_ID --repo
OWNER/REPO --exit-status` and the existing Oracle browser CLI. Do not add
Longrun-specific GitHub or Oracle protocols.

**Rationale**:

- GitHub CLI already owns run waiting and exit-status behavior.
- Oracle already owns browser sessions and any provider-side artifacts.
- Longrun's responsibility is one local wait, bounded result delivery, and
  cleanup; it must not poll, reattach, or start a second target.

**Evidence and constraints**:

- GitHub CLI repository and `gh run watch` documentation were checked for the
  existing command and `--exit-status` contract.
- Oracle browser review was run against the current source during planning;
  the review confirmed that Longrun should not own Oracle session reattachment.
- The retained Oracle review artifact is
  `/Users/norman/.oracle/sessions/review-this-rust-repository-and/artifacts/transcript.md`.
- Google Search was used as a discovery pass for current Codex/PostToolUse
  material; no authoritative result was used in place of repository evidence or
  official documentation.

## Resolved risks

- **Target exit code vs receipt exit code**: the plan treats the exact target
  status as model-visible result data and does not promise to mutate the
  already-completed receipt stub status.
- **Transient permission inheritance**: the plan requires an explicitly
  configured named profile and documents that exact per-call inheritance is not
  claimed.
- **macOS hard death**: the plan guarantees handled and observable cleanup and
  documents the uncatchable-owner limitation.
- **Old durable state**: the plan makes old jobs inert; repair removes old
  SessionStart/service guidance, and the new runtime never resumes old jobs.
