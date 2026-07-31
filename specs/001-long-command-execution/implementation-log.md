# Implementation Evidence: Longrun

This log records the exact focused checks, live scenarios, review notes, and
commits required by the constitution.

## Setup

- 2026-07-31: created the Rust 2024 `longrun` package, shared library,
  cross-platform paths, error exit mapping, ignore rules, and portable fixture
  sources.
- Focused checks: `cargo fmt --check`; `cargo check`; `cargo clippy
  --all-targets -- -D warnings`; `cargo test --locked`; compiled all command
  fixtures and live-ran the success, failure, output, and sleep fixtures.
- Review: one package and one binary only; fixture sources use the standard
  library; no execution backend or runtime behavior was added in setup.

## Foundational Runtime

- 2026-07-31: added versioned native strings, job/delivery/IPC state types,
  safe configuration defaults, SQLite WAL migrations, immutable JSON writes,
  bounded untrusted output, and full CLI parsing.
- Focused checks: `cargo fmt --check`; `cargo test --locked` (17 passing);
  `cargo clippy --all-targets -- -D warnings`; live `longrun --help`,
  `longrun --version`, and fail-closed `longrun doctor`.
- Review: direct argv remains an `OsString` vector; all transition validation
  sits in the store; temporary JSON is fully synced before the no-replace link;
  no command spawn path exists yet.

## Codex Receipt and PreToolUse Routing

- 2026-07-31: added exact-byte HMAC receipts with expiry/context verification,
  one-time nonce consumption, hook-owned token persistence, strict direct
  wrapper parsing, and receipt-only `submit`/`submit-shell`.
- Focused checks: receipt, hook, and CLI tests; `cargo clippy --all-targets
  -- -D warnings`; live JSON PreToolUse rewrite followed by direct execution of
  the rewritten `submit` and `submit-shell` wrappers in an isolated home.
- Review: the rewritten wrapper is non-executing; the hook token is generated
  from OS entropy, stored only as a hash, consumed transactionally, and the
  submit stdout contains exactly one receipt line. PostToolUse waiting remains
  gated on the shared sandbox worker rather than adding a second spawn path.

## Shared Sandbox Runner Base

- 2026-07-31: added the single Tokio runner that constructs `codex sandbox -P
  PROFILE -C CWD -- PROGRAM ARG...`, clears inherited environment, preserves
  separate full logs, bounds result tails, and returns the child exit state.
- Focused check: a fake sandbox executable verified exact command invocation,
  separate stdout/stderr logs, and child exit status; `cargo clippy
  --all-targets -- -D warnings` passed.
- Review: no CLI or hook path invokes the requested program directly. The
  internal worker will own claims and result persistence before any user-facing
  execution route is enabled.

## Worker-Owned Direct Execution

- 2026-07-31: added transactional execution claims, hidden internal workers,
  terminal result persistence, and direct `longrun run` / `run-shell`
  dispatch through that one worker.
- Focused checks: worker replay test; runner log test; full lint; live
  `longrun run -- /bin/sh -c 'printf out; printf err >&2; exit 7'`, which
  preserved separated streams and propagated exit status 7.
- Review: a second worker cannot claim an accepted job. The runner remains the
  sole command-spawn authority; durable routing, process-tree cleanup, and
  PostToolUse waiting remain pending rather than falling back to direct spawn.

## Active Codex Completion

- 2026-07-31: added PostToolUse receipt extraction, exact HMAC/context
  validation, transactional pending consumption and job creation, local worker
  wait, and `continue: false` bounded untrusted result delivery.
- Focused checks: hook fixture includes successful active completion and replay
  rejection; isolated live flow ran PreToolUse → rewritten submit → PostToolUse
  through a fake `codex sandbox`, returning one same-turn completion object.
- Review: PostToolUse never starts a command itself; only the claimed worker
  does. A replay fails before another job can be created or executed.

## Unix Process-Tree Timeout

- 2026-07-31: configured the sandbox child as a Unix process-group leader and
  route timeout through SIGTERM then SIGKILL after the configured grace period.
- Focused check: spawned a background `sleep` descendant, timed out the owner,
  and proved `kill -0` could no longer find the recorded descendant PID.
- Review: this is the shared timeout path used by the runner; Windows Job
  Objects and cancellation wiring remain separate unfinished platform work.

## Direct CLI Contract

- 2026-07-31: added binary-level direct-run checks for successful/failing
  mixed streams, timeout exit 124, and a non-UTF-8 argument.
- Focused check: the binary ran under an isolated HOME with a fake Codex
  sandbox; all direct CLI contract tests passed without relying on ambient
  Longrun state.
- Review: `run` remains direct argv by default; `run-shell` stays explicitly
  configuration-gated and no CLI path bypasses the sandbox worker.

## Runner Contract Completion

- 2026-07-31: completed the runner contract checks: the fake sandbox now
  records and verifies the exact `codex sandbox -P PROFILE -C CWD -- ...`
  invocation, worker execution verifies result persistence, and the runner
  verifies separate full stdout/stderr logs and child exit status.
- Focused check: `cargo test --locked --test runner`; full `cargo test
  --locked`; and `cargo clippy --all-targets -- -D warnings`.
- Review: the test observes the actual runner-to-sandbox process boundary; it
  does not duplicate command construction or rely on implementation internals.

## Explicit Environment Policy

- 2026-07-31: completed the job-owned environment allowlist: the runner starts
  with a safe fixed platform baseline and inherits only names recorded in the
  immutable job policy. `--env-pass` may explicitly opt in a secret-like name;
  an unlisted secret remains absent.
- Focused check: the binary-level security fixture injects a `TOKEN` and a
  `SECRET` into Longrun's parent environment, passes only the former, and
  observes `allowed|missing` in the sandboxed child.
- Review: the runner consumes the job's signed policy rather than mutable
  ambient configuration, so hook-receipted and direct jobs share one exact
  inheritance rule.

## Sandbox Fail-Closed Policy

- 2026-07-31: centralized permission-profile policy in configuration. The
  `:danger-full-access` profile requires both its explicit command request and
  `execution.allow_danger_full_access = true`; other profiles retain the
  existing safe default.
- Focused checks: a sandbox fixture exits 42 and proves Longrun does not run
  the requested command directly; the default configuration rejects a
  danger-full-access direct request before sandbox spawn.
- Review: failure propagates as the sandbox status, with no direct-execution
  fallback or permission-profile substitution.

## Argument, Path, and Replay Boundaries

- 2026-07-31: added security coverage for absolute installed-binary matching,
  PATH-shadow rejection, direct-argument shell-metacharacter preservation, and
  consumed receipt nonce replay rejection.
- Focused checks: a fake sandbox executes `/usr/bin/printf` with a literal
  semicolon-containing argument and never creates the attempted marker; hook
  matching ignores relative and alternate binary paths.
- Review: command authority stays with the exact installed binary and direct
  argv reaches the sandbox without shell reparsing.

## Cross-Platform Process-Tree Ownership

- 2026-07-31: completed the shared process-tree ownership boundary. Unix jobs
  use their own process group and terminate it with SIGTERM then SIGKILL.
  Windows jobs create a Job Object, assign the spawned sandbox process, set
  kill-on-close, attempt CTRL_BREAK during the configured grace period, and
  terminate the whole Job Object if it remains alive.
- Focused checks: the Unix fixture proves its background descendant is gone
  after timeout; `cargo check --locked --target x86_64-pc-windows-gnu`
  compiles the Job Object implementation and the Windows timeout fixture.
- Review: the runner retains the cleanup owner until process exit and kills
  the just-spawned child if Job Object assignment fails; no timeout path
  bypasses platform cleanup.

## Receipt Expiry and Replay Cleanup

- 2026-07-31: completed receipt cleanup enforcement. HMAC verification stays
  on RustCrypto's constant-time `verify_slice` path, consumed nonces remain
  uniquely persisted, and expired pending submissions are deleted before a
  verified hook creates new state.
- Focused checks: a tampered receipt fails verification; an expired pending
  row is removed while an unexpired row remains; receipt issuance now shares
  the pending submission's original expiry instead of extending it.
- Review: PostToolUse rechecks expiry inside its transaction boundary, so an
  expired claimed submission cannot create a job even if it was read earlier.

## Local Cancellation Ownership

- 2026-07-31: added an idempotent persistent cancellation request to the
  execution record. The active worker checks that local request and sends its
  owned child through the same platform process-tree cleanup path used by
  timeouts.
- Focused checks: a worker starts a ten-second sandboxed command, observes a
  second connection's cancellation request, persists `cancelled`, and exits
  without leaving the command running.
- Live check: an isolated `longrun run --json -- /bin/sh -c 'sleep 10'`
  was discovered from `longrun list --json`, cancelled from a second terminal,
  returned exit 130, and reported `execution_state: cancelled`.
- Review: callers do not receive or act on child PIDs; only the owning runner
  consumes cancellation state, preserving a single tree-cleanup authority.

## Local Job Inspection and Retention

- 2026-07-31: completed local status/list/wait/log commands, byte-safe log
  reads and follow, and retention GC. Direct commands mark their result as
  delivered to their waiting caller; GC selects only delivered terminal jobs
  without an active lease, then applies age and total-log-byte limits.
- Focused checks: binary-level CLI tests cover status/list/wait JSON, stdout
  and stderr selection, `--follow`, and non-UTF-8 logs. Store tests prove
  undelivered jobs are retained while delivered jobs are selected by both age
  and byte budget.
- Live check: with `max_age_days = 0`, `gc --dry-run --json` and `gc --json`
  selected the same completed job and its later `status` exited 70.
- Two-terminal check: a running ten-second job was listed and inspected,
  `logs --follow` was attached, a second terminal cancelled it, and the
  original runner plus `wait` both returned 130 while the follower returned
  0 and final JSON reported `cancelled` / `delivered_in_turn`.
- Review: GC validates every stored log path remains inside Longrun's log
  directory before deletion, then deletes database records only after logs.

## Codex Hook Wire Compatibility

- 2026-07-31: completed the current Codex common, PreToolUse, PostToolUse, and
  SessionStart wire deserializers. Hook outputs now share Codex's universal
  `continue`, `systemMessage`, and `suppressOutput` shape while retaining
  typed allow/deny and additional-context envelopes. PostToolUse accepts only
  a string response or a structured string `output` member, then still
  requires one signed, context-matching receipt.
- Focused checks: `cargo test --locked --test hooks`; `cargo test --locked
  --test security`; receipt-shape unit test; `cargo fmt --check`; and `cargo
  clippy --all-targets -- -D warnings`. An isolated live flow ran
  PreToolUse → rewritten `submit` → structured PostToolUse through a fake
  `codex sandbox` and returned the same-turn completion envelope.
- Review: generic `tool_input` avoids failing unrelated tools, structured
  receipt extraction does not scan arbitrary object fields, and only the
  existing receipt/context verification can authorize worker execution.

## Active Hook Live Harness

- 2026-07-31: added `tests/live/active_session.rs`, exposed through
  `cargo test --test active_session -- --ignored`. Its default live smoke
  submits a 90-second command via the real PreToolUse rewrite and structured
  PostToolUse route, then asserts one command start, one sandbox invocation,
  same-turn `continue: false` delivery, and the bounded `DONE` output. Set
  `LONGRUN_ACTIVE_SESSION_SECONDS=1800` for the documented SC-001 duration.
- Live check: ran the default 90-second ignored harness successfully
  (`1 passed`, `90.84s`). The shorter one-second override also passed while
  iterating on the harness.
- Review: the test calls the compiled Longrun binary and the actual worker;
  only the `codex sandbox` boundary is a controlled pass-through fixture.
  The 30-minute mode still requires the external Codex event trace review
  documented in the quickstart to prove zero model-side polling.

## Supervisor IPC Foundation

- 2026-07-31: added bounded (1 MiB), unsigned-32-bit big-endian,
  length-prefixed JSON framing for request, response, and event envelopes.
  Unix sockets use a mode-0600 endpoint; Windows uses first-instance named
  pipes plus a busy-pipe client retry, rejects remote clients, and prevents
  DACL rewriting. Both reject protocol-version mismatch and response IDs that
  do not match the caller's request.
- Focused checks: the Unix transport test bound a real local socket, checked
  its permissions, and completed a request/response round trip. Frame tests
  cover all envelope types, malformed JSON, oversize lengths, and unsupported
  versions. `cargo test --locked --test supervisor` passed (3 tests), and
  `cargo test --locked --test supervisor --no-run --target
  x86_64-pc-windows-gnu` compiled the Windows named-pipe test.
- Review: framing is transport-only and cannot spawn commands; it caps
  allocation before reading a payload. Supervisor ownership and dispatch are
  intentionally still pending, so no CLI or MCP path can bypass the worker.

## Delivery Lease Foundation

- 2026-07-31: migrated delivery state to schema version 3 with target session,
  lease owner/expiry, retry counter, durable idempotency key, and per-session
  recovery lock. Active PostToolUse now claims a timeout-bounded hook lease
  before worker execution and marks the result `delivered_in_turn` only after
  the owned worker has persisted a terminal result.
- Focused checks: migration coverage upgrades a version-2 delivery table;
  recovery tests cover exclusive SessionStart ownership, lease expiry, stable
  delivery identity, resume retry budget, and duplicate resume rejection. Hook
  tests assert successful active completion reaches
  `delivered_in_turn`; the isolated live hook harness also passed with a
  one-second command after the lease path was added.
- Review: expiry clears both delivery and session ownership atomically before
  another claimant can proceed. A lease cannot authorize command execution;
  the existing worker claim remains the sole spawn authority.

## SessionStart Hook Recovery

- 2026-07-31: completed the `longrun hook codex session-start` dispatch and
  ordered recovery step. It expires stale leases, finds only terminal
  undelivered results for the current session, claims a five-minute
  SessionStart lease, and returns the absolute Longrun submission path plus
  the stable delivery idempotency key and bounded untrusted result context.
- Focused checks: `cargo test --locked --test recovery` (3 passed),
  `cargo test --locked --test hooks` (7 passed), and the ignored
  `cargo test --locked --test session_start -- --ignored --nocapture`
  process-level harness (1 passed). The harness calls the compiled CLI with
  a fresh home, proves its empty-state no-op, persists a completed
  session-targeted result, observes one recovery envelope, confirms
  `delivered_on_start`, then confirms a second hook emits nothing.
- Review: the CLI flushes one valid hook envelope before it marks a delivery
  complete; a write failure or crash therefore leaves the lease retryable
  with the original idempotency key. SessionStart handles only recovery
  delivery and never receives command-spawn authority.

## Durable Supervisor Bootstrap

- 2026-07-31: added the first durable supervisor slice: local supervisor
  startup resumes accepted durable jobs, exposes protocol-versioned health,
  submit, wait, status, and cancellation IPC requests, and starts only hidden
  `longrun internal worker` processes. The supervisor never directly spawns a
  requested command.
- `longrun run --mode durable` now submits and waits through that socket;
  durable PostToolUse claims its active delivery lease, asks the supervisor to
  start the already transactionally accepted job, and waits for the same stored
  terminal result. Long Unix runtime paths use a stable short socket fallback.
- Focused checks: a real Unix socket supervisor test starts one persisted
  accepted job after supervisor startup, submits a second durable job over IPC,
  waits for both external workers to complete, and proves exactly two sandbox
  starts. It also verifies the health response and removes the socket on
  controlled shutdown. `cargo test --locked --test supervisor` passed (4
  tests), the Windows supervisor test target compiled, and the full suite
  passed (58 passed, 2 ignored).
- Live check: started `target/debug/longrun daemon` with a disposable home and
  fake `codex sandbox`, then ran `target/debug/longrun run --mode durable
  --json -- /bin/sh -c 'printf durable'`. It returned the persisted succeeded
  result with a bounded `durable` tail and the sandbox start log contained
  exactly one entry.
- Review: worker execution claims remain the at-most-once boundary, so a
  restart may launch replacement internal workers but only one can reach the
  requested command. Store connections now use a bounded SQLite busy timeout
  and skip schema DDL after reaching schema version 3, avoiding spurious
  `SQLITE_BUSY` failures while a supervisor and worker share WAL state.
  Full log/list/GC routing, completion events, restart handling for
  in-progress workers, guarded resume, and service lifecycle remain pending.

## Explicit Service Artifacts

- 2026-07-31: added explicit launchd, systemd-user, and Windows Startup
  artifact renderers. `longrun service install` is the only command that
  writes and registers the matching artifact; `uninstall`, `start`, `stop`,
  and `status` now dispatch through the platform service manager.
- Focused checks: service-renderer unit tests cover absolute-path validation,
  launchd XML escaping, systemd command quoting, Windows batch invocation, and
  manifest sensitivity. macOS/Linux/Windows code paths pass compile checks.
  An isolated-home `target/debug/longrun service status` live CLI check
  returned `not installed` without writing or registering a service.
- Review: no integration, hook, or ordinary command path enables a background
  service implicitly. The macOS and Linux managers provide lifecycle control;
  Windows startup registration is generated, while graceful Windows stop
  remains tied to the supervisor shutdown endpoint work.

## Unified Supervisor Shutdown and Process Cleanup

- 2026-07-31: supervisor shutdown now stops accepting new work, requests
  cancellation only for its active durable workers, and waits for their owned
  runners to persist terminal results. Unix SIGINT/SIGTERM in either a hook
  runner or supervisor now enters the same cancellation route. The runner
  remains the only component that invokes the shared platform process-tree
  termination path.
- Focused checks: `cargo test --locked --test supervisor` (6 passed), including
  a running durable job whose supervisor shutdown returns a `cancelled` result
  to a concurrently waiting IPC client. `cargo test --locked --test
  process_tree`, `--test security`, and `--test worker` passed.
- Live checks: with a disposable home and fake `codex sandbox`, a one-second
  timed `longrun run` killed its recorded `sleep` descendant and exited 124.
  A running durable `longrun run --mode durable` then received exit 130 and a
  persisted `cancelled` result after the daemon received SIGTERM. Sending
  SIGTERM to an embedded runner likewise returned 130 and killed its recorded
  descendant.
- Review: the supervisor tracks worker job IDs, not requested-command PIDs;
  it requests durable cancellation through the store, and the claimed worker
  performs the one platform cleanup call. It drains wait handlers after the
  cancelled result is persisted, avoiding an exit-time lost IPC response.

## Durable Worker Restart and Persistence-Gap Recovery

- 2026-07-31: added execution heartbeats and schema version 4. A restarted
  supervisor adopts a fresh in-progress durable worker instead of launching a
  duplicate. An execution whose heartbeat becomes stale is terminally recorded
  as failed with an explicit persistence-gap result; Longrun never retries its
  requested command.
- Focused checks: `cargo test --locked --test recovery` (5 passed) proves an
  abruptly stopped supervisor can restart while the original worker completes
  exactly once, and proves a stale `running` execution becomes the
  `sha256:worker-persistence-gap` result without invoking the sandbox again.
  Store migration tests cover the version-2 to version-4 path.
- Live check: a durable `sleep 2; printf recovered` job continued after its
  first daemon received SIGKILL. A second daemon reclaimed the stale socket,
  observed the existing worker's heartbeat, and recorded `succeeded`; the fake
  sandbox start log contained exactly one entry.
- Review: worker heartbeats are only liveness evidence. They may prevent a
  premature failure record but never authorize re-execution; stale recovery
  records failure rather than risking a second requested-command spawn.
