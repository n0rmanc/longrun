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
