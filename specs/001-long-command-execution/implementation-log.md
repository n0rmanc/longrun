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
