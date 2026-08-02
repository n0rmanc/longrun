# Implementation Log: Ephemeral RTK-Style Wait Proxy

## Migration inventory

| Existing surface | New surface | Action |
| --- | --- | --- |
| `longrun run -- PROGRAM ARG...` | `longrun PROGRAM ARG...` | Replace with the generic synchronous target surface. |
| `longrun run-shell` / `submit-shell` | `longrun /bin/sh -c SCRIPT` | Keep shell semantics explicit in target argv; do not add a Longrun shell parser. |
| `longrun submit -- PROGRAM ARG...` | `longrun PROGRAM ARG...` | Remove the hook-token submission protocol. |
| `rtk longrun submit -- PROGRAM ARG...` | `rtk longrun PROGRAM ARG...` | Normalize the transparent RTK wrapper to the same target argv. |
| SQLite jobs/results/deliveries | One protected ephemeral handoff | Delete completed Longrun-owned state after the active wait. |
| worker/supervisor/daemon/service | PostToolUse local wait | Execute one owned process tree in the active hook. |
| SessionStart recovery | Manual rerun after owner loss | Do not recover or redeliver stale results. |
| `status`, `wait`, `logs`, `cancel`, `gc`, `mcp` | No replacement | Return a clear migration error. |

## Evidence

This file is append-only during implementation. Each checkpoint records the
commands and observed result before its phase is committed.

## Checkpoint: active wait path and cleanup

Date: 2026-08-02

- Fixed the focused PostToolUse test. The failure was test-only: PreToolUse
  created the handoff at synthetic epoch `1000`, while PostToolUse validated it
  with the real current epoch, so the handoff was correctly treated as expired.
  The test now uses one real timestamp for prepare/arm/post.
- Removed the unused durable log-chunk/tail helpers so Longrun no longer
  exposes a file-log reader in the ephemeral output module.
- Increased the generated PostToolUse timeout from `86400` to `86410` seconds;
  the Rust configuration formula remains target timeout plus termination,
  forced-cleanup, and serialization margins.
- Removed durable-only dependencies (`anyhow`, `hmac`, `rmcp`, `rusqlite`,
  `schemars`, `time`, and `zeroize`) and kept the shared native runner.

Validation:

```text
rtk cargo test --locked --test hooks post_tool_use_claims_once_waits_and_returns_same_turn_result -- --nocapture --test-threads=1
1 passed

rtk cargo fmt --check
pass

rtk cargo clippy --all-targets -- -D warnings
No issues found

rtk cargo test --locked
50 passed, 1 ignored

LONGRUN_ACTIVE_SESSION_SECONDS=1 rtk cargo test --locked --test active_session active_hook_waits_once_and_delivers_to_the_same_turn -- --ignored --nocapture
1 passed

LONGRUN_ACTIVE_SESSION_SECONDS=125 target/debug/longrun --env-pass LONGRUN_ACTIVE_SESSION_SECONDS --timeout 5m -- cargo test --locked --test active_session active_hook_waits_once_and_delivers_to_the_same_turn -- --ignored --nocapture
1 passed; active target duration 125.87s

rtk ccc index
91 files listed; 4 added, 20 deleted, 36 reprocessed; errors: 0
```

The 125-second harness observed one receipt-stub path, one PostToolUse wait,
one target start, and one same-turn bounded result. No worker, supervisor,
result database, or Longrun-owned output log was created.

## Remaining release-gated validation

The deterministic implementation and active-session acceptance path pass.
Release-gated GitHub Actions and Oracle browser scenarios still need explicit
credentials, a controlled target, and live verification before claiming the
complete quickstart matrix. Process cancellation/leader-exit coverage and
old-state/manual-rerun inspection also remain listed in `tasks.md`.
