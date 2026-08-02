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

## Checkpoint: generic GitHub and Oracle targets

Validation on 2026-08-02 used the canonical generic command surface:

```text
target/debug/longrun -- gh run watch 30700238163 --repo n0rmanc/longrun --exit-status
Run CI (30700238163) has already completed with 'success'

target/debug/longrun --timeout 10m -- oracle --engine browser \
  --browser-manual-login \
  --browser-manual-login-profile-dir /Users/norman/.oracle/browser-profile \
  --browser-auto-reattach-delay 5s \
  --browser-auto-reattach-interval 3s \
  --browser-auto-reattach-timeout 60s \
  --browser-keep-browser \
  --model gpt-5-pro \
  -p 'Reply exactly ORACLE_LONGRUN_OK'
Answer: ORACLE_LONGRUN_OK
23.0s; resolved model gpt-5-pro; browser run completed once
```

These runs used no Longrun-specific GitHub or Oracle subcommand and no model
polling loop in the caller. The wrapped tools performed only their own normal
polling/session behavior.

The RTK wrapper was also validated against the new binary on `PATH`:

```text
PATH=target/debug:$PATH rtk longrun gh run watch 30700238163 --repo n0rmanc/longrun --exit-status
Run CI (30700238163) has already completed with 'success'

PATH=target/debug:$PATH rtk longrun -- /bin/sh -c 'sleep 1; printf "rtk-longrun-ok\n"'
rtk-longrun-ok
```

An un-upgraded `longrun` earlier in `PATH` still exposes the removed legacy
subcommands; `rtk longrun` delegates to that binary. Upgrade the binary and
run `longrun init --codex --repair` before testing the new transparent form.

## Validation status before convergence

The deterministic implementation, the 125-second active-session acceptance
path, direct generic GitHub/Oracle targets, Codex-hook GitHub/Oracle scenarios
(including failure/auth cases), process cancellation/leader-exit coverage, and
old-state/manual-rerun inspection all pass. The remaining work is the final
quickstart matrix, repository-wide static checks, and the final convergence
review.

## Checkpoint: owned process-tree lifecycle

Date: 2026-08-02

- Normal leader exit now performs a best-effort process-group cleanup so
  background descendants do not survive a successful shell leader.
- Added Unix tests for timeout, future cancellation/child drop, leader exit,
  descendant cleanup, and SIGTERM owner shutdown.

Validation:

```text
rtk cargo test --locked --test process_tree -- --nocapture --test-threads=1
4 passed

rtk cargo clippy --all-targets -- -D warnings
No issues found
```

## Checkpoint: convergence lifecycle, security, and live hook evidence

Date: 2026-08-02

- Fixed the ignored Codex-hook live helper to write configuration at the
  platform-specific `ProjectDirs` path. On macOS this is
  `~/Library/Application Support/dev.longrun.Longrun`; the previous fixture
  wrote only XDG-style paths, so the hook correctly denied
  `:danger-full-access` before starting a target.
- Added explicit lost-delivery/no-recovery assertions, fake hook-JSON
  rejection, no-spill-field coverage, and doctor guidance coverage for old
  `longrun.sqlite` state. Legacy state remains untouched and is optional to
  remove with `longrun uninstall --codex --purge-data`.
- Added an ignored real-Codex sandbox test for denied outside-home writes and
  network access. It passed with the configured `:workspace` profile.

Focused checks:

```text
rtk cargo fmt --check
pass

rtk cargo test --locked --test hooks --test output --test integration_codex --test security
21 passed, 1 ignored

rtk cargo test --locked --test security live_workspace_profile_denies_outside_write_and_network -- --ignored --nocapture --test-threads=1
1 passed

LONGRUN_GITHUB_RUN_ID=30700238163 LONGRUN_GITHUB_REPO=n0rmanc/longrun \
  GH_TOKEN=<gh auth token> \
  rtk cargo test --locked --test github_watch \
  codex_hook_waits_for_a_github_actions_run_once \
  -- --ignored --nocapture --test-threads=1
1 passed

LONGRUN_GITHUB_FAILURE_RUN_ID=30699648987 LONGRUN_GITHUB_REPO=n0rmanc/longrun \
  GH_TOKEN=<gh auth token> \
  rtk cargo test --locked --test github_watch \
  codex_hook_returns_github_failure_without_retry \
  -- --ignored --nocapture --test-threads=1
1 passed

LONGRUN_GITHUB_RUN_ID=30700238163 LONGRUN_GITHUB_REPO=n0rmanc/longrun \
  GH_TOKEN=<gh auth token> \
  rtk cargo test --locked --test github_watch \
  codex_hook_reports_github_auth_failure_without_widening_access \
  -- --ignored --nocapture --test-threads=1
1 passed

LONGRUN_ORACLE_LIVE=1 ORACLE_BROWSER_PROFILE=/Users/norman/.oracle/browser-profile \
  rtk cargo test --locked --test oracle \
  codex_hook_runs_one_oracle_browser_review \
  -- --ignored --nocapture --test-threads=1
1 passed; 32.37s

LONGRUN_ORACLE_LIVE_FAILURE=1 ORACLE_BROWSER_PROFILE=/Users/norman/.oracle/browser-profile \
  rtk cargo test --locked --test oracle \
  codex_hook_returns_oracle_failure_without_reattachment \
  -- --ignored --nocapture --test-threads=1
1 passed
```

## Final quickstart matrix and requirement map

Date: 2026-08-02

| Quickstart scenario | Evidence |
| --- | --- |
| Static validation | `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test --locked` → 61 passed, 7 ignored |
| Direct success/failure | `longrun -- /bin/sh -c ...` → `0` and `7`, with separate `ok`/`failure` output |
| Same-turn wait | `target/debug/longrun ... active_session ...` → one target, 125.98s, same-turn result |
| Generic RTK surface | `PATH=target/debug:$PATH rtk longrun ...` → GitHub success and `rtk-longrun-ok` |
| GitHub live hook | success run `30700238163`, cancelled run `30699648987`, invalid-auth case → 1 passed each |
| Oracle live hook | browser success and failure/no-reattach cases → 1 passed each |
| Lifecycle | process-group timeout, child drop, leader exit, SIGTERM/SIGINT/SIGHUP owner shutdown → 5 passed |
| Security/output | focused security/output/hooks suites → 21 passed, 1 ignored; real Codex workspace denial → 1 passed |
| Install/repair/doctor | isolated Codex integration suite → 14 passed across integration, handoff, process-tree, and runner checks |

Requirement coverage:

| Requirement | Evidence |
| --- | --- |
| FR-001–FR-005, FR-020, FR-023 | generic target parser, RTK normalization, literal argv, shell-composition and migration tests |
| FR-006–FR-010 | handoff transition/race tests and active same-turn harness |
| FR-011–FR-013 | bounded output, fake receipt/JSON rejection, lost-delivery/manual-rerun tests |
| FR-014–FR-016 | Unix process-tree tests and Windows compile path |
| FR-017–FR-019 | named-profile fail-closed tests, real workspace write/network denial, environment filtering |
| FR-021–FR-024 | generated hook/skill snapshots, doctor legacy-state warning, timeout-margin checks |
| SC-001–SC-003 | 126-second wait, single active completion, 100 concurrent claim attempts |
| SC-004–SC-007 | no durable state, lifecycle cleanup, security/output suites |
| SC-008–SC-011 | direct/RTK status propagation, GitHub and Oracle hook runs |
| SC-012 | doctor and README document Unix/macOS hard owner-death limitation and manual rerun |

## Checkpoint: final contract corrections

Date: 2026-08-02

- Handoff claims now explicitly transition the in-memory and on-disk record to
  `claimed` before execution; the handoff test reads the claimed JSON state.
- Codex PreToolUse now rejects unsupported `env`, `sudo`, `command`, `nohup`,
  and `timeout` wrappers around Longrun instead of silently bypassing the
  wait adapter.
- Unix owner shutdown now handles `SIGINT`, `SIGTERM`, and `SIGHUP`; the
  process-tree suite covers all three plus timeout, child-drop, and leader-exit
  cleanup.
- README and the installed skill document the unavoidable macOS/Unix
  uncatchable-owner-death limitation and manual-rerun contract.

Validation:

```text
rtk cargo test --locked --test handoff --test hooks
10 passed

rtk cargo test --locked --test process_tree -- --nocapture --test-threads=1
5 passed

rtk cargo clippy --all-targets -- -D warnings
No issues found

rtk cargo test --locked
61 passed, 7 ignored

rtk ccc index
76 files listed; 0 added, 0 deleted, 10 reprocessed, 66 unchanged; errors: 0
```
