# CLI Contract

## General rules

- Binary name: `longrun`.
- Human-readable output goes to stdout; diagnostics go to stderr.
- `--json` emits one JSON value and suppresses decorative output.
- Commands preserve child exit status where documented.
- Program arguments follow `--` and are never reparsed as shell syntax.

## Execution

### `longrun run [OPTIONS] -- PROGRAM [ARGS...]`

Runs a command synchronously for a human, script, or CI process.

Options:

- `--timeout DURATION`
- `--permission-profile NAME`
- `--env-pass NAME` (repeatable)
- `--mode embedded|durable`
- `--json`

Exit behavior:

- Child exits normally: return the child exit code.
- Timeout or cancellation: return Longrun's documented non-zero control code.
- Spawn, sandbox, storage, or validation failure: return a distinct documented
  non-zero control code.

### `longrun submit [OPTIONS] -- PROGRAM [ARGS...]`

Creates a single-use Codex submission envelope and returns quickly.

The generated Codex hook adds a hidden `--hook-token TOKEN` option. Interactive
users MUST NOT supply it manually; tokens are single-use and expire quickly.

Stdout:

```text
LONGRUN_RECEIPT_V1 <base64url-payload>.<base64url-signature>
```

No other stdout is permitted.

### `longrun run-shell --script SCRIPT`

### `longrun submit-shell --script SCRIPT`

Explicit shell-evaluation variants. They are disabled unless configuration
allows shell mode.

## Job operations

### `longrun wait JOB_ID [--json]`

Blocks locally until the job reaches a terminal state.

### `longrun status JOB_ID [--json]`

Returns execution state, delivery state, owner, timing, and log locations.

### `longrun list [--state STATE] [--json]`

Lists jobs newest first.

### `longrun logs JOB_ID [--follow] [--stderr]`

Reads stdout by default. `--stderr` selects stderr. `--follow` waits for new
bytes until the job completes or the caller stops.

### `longrun cancel JOB_ID [--grace DURATION] [--json]`

Requests process-tree termination. Repeating cancellation is idempotent.

### `longrun gc [--dry-run] [--json]`

Removes only retention-eligible terminal, delivered jobs and their logs.

## Integration

### `longrun init --codex [--repair] [--json]`

Renders the local marketplace and plugin, installs `longrun@longrun-local`,
records owned paths, and reports that hooks require user trust.

### `longrun uninstall --codex [--json]`

Removes the installed plugin, Longrun marketplace source, and generated
Longrun-owned integration files. It preserves job data unless `--purge-data`
is explicitly supplied.

### `longrun doctor [--json]`

Checks:

- executable and version;
- state directory ownership and permissions;
- database migrations and integrity;
- Codex CLI version and required commands;
- generated plugin and absolute hook paths;
- hook trust status when observable;
- sandbox profile resolution;
- supervisor endpoint and protocol;
- platform process-control support.

## Supervisor service

### `longrun daemon [--foreground]`

Runs the durable supervisor event loop.

### `longrun service install|uninstall|start|stop|status`

Manages only Longrun's per-user service. `install` is explicit and idempotent.

## Hook entrypoints

All hook commands read one Codex hook JSON object from stdin and write either
no output or one Codex hook JSON object to stdout.

```text
longrun hook codex pre-tool-use
longrun hook codex post-tool-use
longrun hook codex session-start
```

## Optional structured interface

### `longrun mcp`

Runs a stdio MCP server exposing status, wait, logs, and cancel through the same
supervisor protocol. It MUST NOT provide an independent spawn path.
