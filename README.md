# Longrun

Run finite, long-running commands without model polling.

Longrun is a standalone Rust CLI. Its optional Codex integration is a thin
adapter over the same local runtime: it creates a receipt from a verified Bash
submission, waits locally in `PostToolUse`, and returns the final bounded
result to the originating turn.

## Install

Use a published, checksummed release on macOS or Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/n0rmanc/longrun/main/install.sh | sh
```

The installer selects the supported native archive, downloads its adjacent
SHA-256 file, verifies it, and installs `longrun` to `~/.local/bin` by
default. Add that directory to `PATH` if necessary.

Or install the same published archive through Homebrew:

```sh
brew install --formula https://raw.githubusercontent.com/n0rmanc/longrun/main/Formula/longrun.rb
```

Developers can install from source:

```sh
cargo install --path .
```

## Codex integration

```sh
longrun init --codex
longrun doctor
```

`init` renders a local `longrun-local` marketplace under Longrun's private
data directory, installs `longrun@longrun-local` through the Codex CLI, and
uses the resolved absolute path of the executable in every generated hook.
Review and trust the hooks in Codex with `/hooks`.

If project instructions require RTK, Codex may submit through the one supported
wrapper:

```sh
rtk longrun submit -- PROGRAM ARG...
```

Longrun's `PreToolUse` hook validates that exact form and rewrites it to the
installed absolute Longrun binary before issuing a receipt. Other wrappers are
not supported.

Repair after moving or replacing the binary:

```sh
longrun init --codex --repair
```

Remove only Longrun-owned integration files and selectors:

```sh
longrun uninstall --codex
```

No background service is installed by `init`. Durable execution requires the
explicit `longrun service install` action.

## Commands

```text
longrun run -- PROGRAM ARG...
longrun submit -- PROGRAM ARG...
longrun status JOB_ID
longrun wait JOB_ID
longrun list
longrun logs JOB_ID [--stderr] [--follow]
longrun cancel JOB_ID [--grace DURATION]
longrun gc [--dry-run]
```

Use `run` for human, script, and CI execution. Codex uses `submit`; only the
internal worker can run its verified requested command. Direct argument mode
is the default. `run-shell` and `submit-shell` require
`execution.allow_shell = true`.

## Configuration

Longrun reads `config.toml` from its platform configuration directory, or a
path passed with `--config`.

```toml
[execution]
timeout_ms = 86400000
permission_profile = ":workspace"
allow_shell = false
allow_danger_full_access = false
termination_grace_ms = 5000
concurrency = 32

[output]
model_max_bytes = 32768
tail_bytes = 65536

[environment]
pass = []

[recovery]
auto_resume = false
retry_budget = 3

[retention]
max_age_days = 30
max_log_bytes = 10737418240
```

`recovery.auto_resume` is disabled by default. When explicitly enabled, it
can invoke `codex exec resume` only to deliver a persisted result; it never
re-runs the requested command.

## Security and recovery

- Longrun runs direct program-and-argument jobs through `codex sandbox` with
  the selected profile. It never widens permissions automatically.
- Secret-like environment names are removed unless explicitly allowed in the
  immutable job policy.
- Full stdout and stderr remain local. Model-visible results contain bounded,
  explicitly untrusted tails and local log paths.
- Jobs execute at most once. Delivery can retry through the active hook,
  SessionStart, or optional guarded resume without re-executing the command.
- `longrun doctor` reports executable, local-state, Codex integration,
  sandbox, and supervisor health.

## Validate from source

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

For the complete validation scenarios and architecture contracts, see
[`specs/001-long-command-execution/quickstart.md`](specs/001-long-command-execution/quickstart.md).
