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
