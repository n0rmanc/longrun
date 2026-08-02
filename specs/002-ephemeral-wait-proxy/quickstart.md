# Quickstart: Ephemeral RTK-Style Wait Proxy

This guide validates the feature from source after implementation.

## Prerequisites

- Rust 1.88 or newer.
- For Codex-hook tests, a trusted Codex installation with the Longrun
  PreToolUse and PostToolUse hooks installed.
- For Codex-hook tests, a configured named Codex permission profile that
  permits the test command. Direct terminal/CI tests do not require Codex.
- `gh` authentication for the live GitHub Actions check.
- A pre-authenticated Oracle browser profile for the live Oracle check.

## Static validation

From the repository root:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

Expected result: all checks pass and the new runtime creates no durable
supervisor, worker, IPC, SessionStart recovery, or runtime SQLite state.

## Direct terminal validation

Run a successful and failing finite command:

```sh
cargo run -- -- /bin/sh -c 'printf "ok\n"; exit 0'
cargo run -- -- /bin/sh -c 'printf "failure\n" >&2; exit 7'
```

Expected result:

- The target receives literal arguments.
- The direct invocation returns the target exit status.
- stdout and stderr remain distinguishable.

## Same-turn no-polling validation

Use the active hook harness with a command longer than the normal shell wait
threshold:

```sh
LONGRUN_ACTIVE_SESSION_SECONDS=125 \
  cargo test --locked --test active_session \
  active_hook_waits_once_and_delivers_to_the_same_turn \
  -- --ignored --nocapture
```

The acceptance harness must additionally prove:

- no `write_stdin` or status polling occurs;
- the receipt stub completes within 1000 ms;
- the long wait occurs inside PostToolUse;
- PreToolUse and PostToolUse share session, turn, and tool identity;
- the final result is delivered once to the active turn.

## Generic RTK-style command validation

Use both forms with a deterministic fixture:

```sh
longrun -- /bin/sh -c 'sleep 3; printf "longrun-ok\n"'
rtk longrun -- /bin/sh -c 'sleep 3; printf "rtk-longrun-ok\n"'
```

Expected result: both forms run the same target with the same arguments and do
not require `submit`, hook tokens, shell wrappers, or special CI commands.

## GitHub Actions live validation

Use a known run ID:

```sh
longrun gh run watch RUN_ID \
  --repo OWNER/REPO \
  --exit-status
```

Expected result:

- Longrun waits locally until the run completes.
- The active Codex turn receives the final run status and bounded output.
- There is no model polling, `gh run watch` reattachment, or second invocation.
- A failed run preserves the target's nonzero result.

Use a dedicated test repository and avoid relying only on a currently completed
run; include success, failure, cancellation, and authentication-denied cases.

## Oracle browser live validation

Run a small controlled review with the already authenticated browser profile:

```sh
longrun oracle \
  --engine browser \
  --model gpt-5-pro \
  -p "Review the controlled fixture and return one short sentence." \
  --file "tests/fixtures/**"
```

Expected result:

- The active turn waits without model polling.
- Oracle runs exactly once.
- Longrun returns the final exit status, duration, and bounded untrusted tails.
- Longrun does not call Oracle session reattachment or automatic recovery.
- Browser artifacts created by Oracle itself are outside Longrun's persistence
  contract.

## Lifecycle and security validation

The test suite must cover:

- duplicate, forged, expired, mismatched, and malformed handoffs;
- no completed Longrun state after success, failure, timeout, cancellation, or
  lost delivery;
- environment secret filtering and explicit authentication-variable passing;
- denied filesystem/network access without profile widening;
- bounded hook output with `additionalContextLimit: 0` and no Codex spill file;
- PostToolUse timeout arithmetic covering target, termination, forced-cleanup,
  and serialization margins;
- Unix process-group cleanup and documented macOS hard-kill limitation;
- Windows suspended Job Object assignment and kill-on-close cleanup;
- no obsolete SessionStart hook, service artifact, or submit-only skill after
  `longrun init --codex --repair`.
