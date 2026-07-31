# Quickstart Validation: Long-Running Command Execution

This guide validates the completed product; it is not an implementation script.

## Prerequisites

- A supported Rust toolchain for building from source.
- A current Codex CLI with plugin, hooks, sandbox, and exec-resume commands.
- A disposable test repository.
- On Windows, a shell that preserves native argument boundaries.

## Build and baseline checks

```bash
cargo build --locked
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

Expected: all commands succeed.

## Direct CLI execution

```bash
target/debug/longrun run -- sh -c 'printf "out\n"; printf "err\n" >&2; exit 7'
```

Expected:

- stdout and stderr remain distinct;
- Longrun exits with status 7;
- local logs and result metadata exist.

On Windows, use the equivalent fixture executable rather than shell syntax.

## Codex integration

```bash
target/debug/longrun init --codex
target/debug/longrun doctor
```

Expected:

- local marketplace and plugin are generated;
- hooks use the absolute `target/debug/longrun` path;
- `longrun@longrun-local` is installed;
- doctor reports that hook trust may require user action;
- no durable service is installed.

Open a new Codex session, review Longrun hooks with `/hooks`, and trust the
generated definitions.

## Zero-poll active-session test

Ask Codex:

```text
Use Longrun to execute a command that waits 90 seconds, prints DONE, and then
continue by reporting the exit code and output.
```

Expected:

- Codex invokes `longrun submit`, not `longrun run`;
- the command starts exactly once;
- no `write_stdin` or periodic model turn occurs during the wait;
- the same session and turn continue after completion;
- the result contains a bounded tail and local log paths.

## Receipt and replay tests

Run the hook fixture suite:

```bash
cargo test --test hooks --test receipts
```

Expected:

- forged, expired, mismatched, replayed, and consumed receipts execute nothing;
- unrelated Bash calls are no-ops;
- outer shell composition is denied;
- retrying delivery does not re-execute a job.

## Timeout and process-tree test

```bash
cargo test --test process_tree process_tree_timeout -- --exact --nocapture
```

Expected: the parent and all descendants are gone after the configured grace
period, and the result is `timed_out`.

## Durable recovery

```bash
target/debug/longrun service install
target/debug/longrun service start
target/debug/longrun doctor
```

Submit a durable 90-second fixture, close the originating Codex process, wait
for completion, then resume the same session.

Expected:

- the job survives Codex termination;
- `SessionStart` delivers one logical result identity;
- the job is not executed again;
- uncertain recovery retries retain the same idempotency key;
- no automatic `codex exec resume` occurs unless explicitly enabled.

## Sandbox denial

Run a fixture that attempts a write outside the selected permission profile.

Expected:

- the command fails;
- Longrun does not retry with broader permissions;
- the denial is recorded in the bounded result and full local logs.

## Uninstall preservation

```bash
target/debug/longrun uninstall --codex
```

Expected:

- Longrun's plugin and marketplace are removed;
- unrelated Codex configuration and plugins remain unchanged;
- job data remains unless explicit purge was requested.
