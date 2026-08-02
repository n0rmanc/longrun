# Longrun

Run finite, long-running commands without model polling.

Longrun is a small Rust CLI. Its Codex integration is a thin two-hook adapter:
`PreToolUse` replaces the requested command with a fast receipt, and
`PostToolUse` runs the original command locally until it finishes, then returns
the bounded result to the same turn. There is no worker, daemon, job database,
automatic retry, or persisted result.

## Install

Use a published, checksummed release on macOS or Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/n0rmanc/longrun/main/install.sh | sh
```

The installer selects the supported native archive, verifies its adjacent
SHA-256 file, and installs `longrun` to `~/.local/bin` by default. Add that
directory to `PATH` if necessary.

Or install the same published archive through Homebrew:

```sh
brew install --formula https://raw.githubusercontent.com/n0rmanc/longrun/main/Formula/longrun.rb
```

Developers can install from source:

```sh
cargo install --path .
```

## Upgrade

Re-run the release installer to replace an existing binary:

```sh
curl -fsSL https://raw.githubusercontent.com/n0rmanc/longrun/main/install.sh | sh
hash -r
longrun --version
# Expected: longrun 0.1.8
longrun init --codex --repair
longrun doctor --json
```

The generic command surface is available in `0.1.8`. If `longrun --help`
still shows legacy commands such as `submit` or `run`, the old binary is still
installed; rerun the installer, then run `hash -r` or start a new shell.

For Homebrew:

```sh
brew reinstall --formula https://raw.githubusercontent.com/n0rmanc/longrun/main/Formula/longrun.rb
longrun init --codex --repair
longrun doctor --json
```

For a source installation:

```sh
git pull --ff-only
cargo install --path . --locked --force --root ~/.local
hash -r
longrun init --codex --repair
longrun doctor --json
```

## Codex integration

```sh
longrun init --codex
longrun doctor
```

`init` installs the local `longrun` plugin through Codex, renders absolute
`PreToolUse` and `PostToolUse` hook commands, and installs the skill. Review
and trust the hooks in Codex with `/hooks`.

Use the same generic command surface from a terminal, CI, or Codex:

```sh
longrun PROGRAM ARG...
rtk longrun PROGRAM ARG...
```

Examples:

```sh
rtk longrun -- /bin/sh -c 'sleep 3; printf "longrun-ok\n"'

rtk longrun gh run watch RUN_ID --repo OWNER/REPO --exit-status

rtk longrun oracle --engine browser --model gpt-5-pro \
  -p "TASK" --file "src/**"

rtk longrun cargo test --locked
```

`gh run watch` and Oracle may perform their own network polling, but the Codex
model does not need to poll. The command returns once the target exits, times
out, or is cancelled. If the Codex turn is lost before delivery, rerun the
command manually; Longrun does not keep a background result.

## Execution metrics

Longrun records minimal local metadata after each direct or Codex-hook target
reaches a terminal result. View the global count, total wait, average duration,
outcome counts, and per-program breakdown:

```sh
longrun gain
```

Use JSON for scripts, or clear only the local metrics history:

```sh
longrun gain --json
longrun gain --clear
```

`gain` reports measured Longrun executions and elapsed time only. It does not
estimate Codex/provider request counts or token savings, and it never stores
command arguments, output, prompts, credentials, or request bodies. The
management command is distinct from an executable named `gain`; use
`longrun -- gain` for that target.

Use `--` when the target program could be confused with a Longrun option:

```sh
longrun --timeout 30m -- gh run watch RUN_ID --repo OWNER/REPO --exit-status
```

To invoke the installed skill explicitly in Codex:

```text
$longrun:longrun
```

Repair after moving or replacing the binary:

```sh
longrun init --codex --repair
```

Remove only Longrun-owned integration files and selectors:

```sh
longrun uninstall --codex
```

## Configuration

Longrun reads `config.toml` from its platform configuration directory, or a
path passed with `--config`.

```toml
[execution]
timeout_ms = 86400000
permission_profile = ":workspace"
allow_danger_full_access = false
include_managed_config = true
termination_grace_ms = 5000
forced_cleanup_margin_ms = 2000
result_serialization_margin_ms = 1000
post_tool_use_timeout_ms = 86408000

[handoff]
ttl_ms = 300000

[output]
model_max_bytes = 32768
tail_bytes = 65536

[environment]
pass = []
```

GitHub Actions waits and Oracle browser reviews need network access. The
default `:workspace` profile does not grant it. Opt in explicitly:

```toml
[execution]
allow_danger_full_access = true
```

Then:

```sh
longrun --permission-profile :danger-full-access -- \
  gh run watch RUN_ID --repo OWNER/REPO --exit-status
```

Longrun does not widen permissions automatically, and it does not support
shell composition such as `;`, `|`, or `&&` in the Codex command form. Pass a
program and native arguments instead.

## Security and lifecycle

- Codex-hook execution uses `codex sandbox` with the selected named profile.
- Protected environment variables are removed unless explicitly allowed.
- Output is captured with bounded tails, byte counts, hashes, and an
  untrusted-output marker before it is returned to the model.
- The ephemeral handoff is private, one-time, and removed after delivery.
- Handled timeout, cancellation, and owner shutdown terminate the owned
  process tree. An uncatchable Unix/macOS `SIGKILL`, crash, or power loss can
  still leave descendants; Longrun does not recover them, and the user must
  manually rerun the command after checking for any orphan.
- No worker, supervisor, service, SQLite job store, recovery, or automatic
  rerun is installed.

## Validate from source

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

For the feature contract and acceptance scenarios, see
[`specs/003-execution-metrics/quickstart.md`](specs/003-execution-metrics/quickstart.md).
