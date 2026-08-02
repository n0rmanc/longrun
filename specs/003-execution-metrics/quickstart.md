# Quickstart: Local Execution Metrics

This guide validates the `longrun gain` contract without inspecting the
metrics storage format.

## Prerequisites

- A built `longrun` binary on `PATH`, or the repository binary from
  `cargo build`.
- A temporary home/config/data environment so the check does not alter the
  user's existing metrics.

Example setup:

```sh
ROOT="$(mktemp -d)"
export HOME="$ROOT/home"
export XDG_CONFIG_HOME="$ROOT/config"
export XDG_DATA_HOME="$ROOT/data"
export XDG_RUNTIME_DIR="$ROOT/runtime"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
```

## Scenario 1: Empty report

```sh
longrun gain
longrun gain --json
```

Expected:

- both commands succeed;
- the report shows zero recorded executions;
- the JSON object contains zero totals and an empty `by_program` list.

## Scenario 2: Direct terminal results are counted once

```sh
longrun -- /bin/sh -c 'sleep 1; exit 0'
longrun -- /bin/sh -c 'exit 7'
longrun gain --json
```

Expected:

- `recorded_executions` is `2`;
- `outcomes.completed` is `1`;
- `outcomes.failed` is `1`;
- `total_duration_ms` is at least 1000;
- one program summary exists for `sh`;
- the command arguments and output do not appear in the JSON.

## Scenario 3: Timeout is recorded

```sh
longrun --timeout 50 -- /bin/sh -c 'sleep 1'
longrun gain --json
```

Expected:

- the target command exits with Longrun's timeout status;
- `outcomes.timed_out` increases by one;
- the timeout duration is included in the total.

## Scenario 4: JSON and human reports agree

```sh
longrun gain > human.txt
longrun gain --json > report.json
```

Parse `report.json` with a standard JSON parser and verify that its totals and
per-program values match `human.txt`.

## Scenario 5: Clear only metrics

```sh
longrun gain --clear
longrun gain --json
```

Expected:

- clear succeeds without starting a target;
- the next report has `recorded_executions: 0`;
- Longrun configuration and Codex integration remain usable.

Then run one new target and report again:

```sh
longrun -- /bin/echo after-clear
longrun gain --json
```

Expected: the new terminal result is counted in the new measurement period.

## Scenario 6: Codex hook execution

With the Codex hooks installed and trusted, run one supported Longrun command
from a Codex turn and wait for the single `PostToolUse` result. Then run:

```sh
longrun gain --json
```

Expected:

- the hook target appears once in the global totals and under its executable
  basename;
- the internal receipt/stub does not appear as a separate program;
- no model polling is required while the target runs.

## Automated quality gates

Run the repository gates after implementation:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```
