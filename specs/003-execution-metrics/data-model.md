# Data Model: Local Execution Metrics

## ExecutionMetric

A minimal local record for one Longrun target execution that reached a terminal
result.

| Field | Type | Required | Rules |
|---|---|---:|---|
| `schema_version` | positive integer | yes | Allows future readers to reject unsupported records safely. |
| `program` | string | yes | Executable basename only; never the full command line. |
| `duration_ms` | non-negative integer | yes | Copied from the shared runner result. |
| `outcome` | enum string | yes | `completed`, `failed`, `timed_out`, `cancelled`, or `owner_shutdown`. |
| `exit_code` | integer or null | yes | Present when the terminal result provides one; null otherwise. |
| `mode` | enum string | yes | `direct` or `codex_hook`. |
| `completed_at_ms` | signed integer | yes | Local completion timestamp in milliseconds. |

The `failed` outcome is the report label for an exited target with a nonzero
or unavailable exit code. It is distinct from `timed_out`, `cancelled`, and
`owner_shutdown`.

### Validation and privacy rules

- Unknown or missing required fields make a record unreadable; the report
  ignores it rather than treating it as a successful execution.
- `program` must be non-empty and must not contain raw arguments.
- `duration_ms` must be a valid non-negative integer.
- `outcome` and `mode` must be one of the defined enum values.
- Records must not contain command arguments, output, working directory,
  prompts, credentials, request bodies, or arbitrary extension fields.

## GainReport

An aggregate returned by `longrun gain`.

| Field | Type | Meaning |
|---|---|---|
| `recorded_executions` | non-negative integer | Number of valid execution metrics. |
| `total_duration_ms` | non-negative integer | Sum of all recorded durations. |
| `average_duration_ms` | non-negative integer | Integer average; zero for an empty report. |
| `outcomes` | `OutcomeCounts` | Count for each terminal outcome. |
| `by_program` | list of `ProgramSummary` | One summary per executable basename. |

`recorded_executions` equals the sum of all `outcomes` counts and the sum of
all `by_program.count` values.

## OutcomeCounts

| Field | Type |
|---|---:|
| `completed` | non-negative integer |
| `failed` | non-negative integer |
| `timed_out` | non-negative integer |
| `cancelled` | non-negative integer |
| `owner_shutdown` | non-negative integer |

## ProgramSummary

| Field | Type | Meaning |
|---|---|---|
| `program` | string | Executable basename. |
| `count` | non-negative integer | Number of valid metrics for the program. |
| `total_duration_ms` | non-negative integer | Sum of the program's durations. |
| `average_duration_ms` | non-negative integer | Integer average for the program. |

## Relationships and lifecycle

- One terminal target execution creates at most one `ExecutionMetric`.
- Many `ExecutionMetric` records contribute to one `GainReport`.
- `longrun gain --clear` removes all `ExecutionMetric` records but does not
  alter configuration, Codex hook installation, or in-flight execution state.
- A new terminal result after a clear creates a new metric under the normal
  recording rules.
