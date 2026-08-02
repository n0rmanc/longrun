# CLI Contract: `longrun gain`

## Human-readable report

```text
longrun gain
```

The command exits successfully and prints:

- the number of valid recorded target executions;
- total elapsed waiting time in a human-readable minute/second form;
- average elapsed duration;
- counts for `completed`, `failed`, `timed_out`, `cancelled`, and
  `owner_shutdown`;
- a per-program table with count, total duration, and average duration.

An empty history is valid and reports zero values and an empty per-program
section.

## JSON report

```text
longrun gain --json
```

The command exits successfully and writes one JSON object to stdout. Its
top-level shape is:

```json
{
  "recorded_executions": 0,
  "total_duration_ms": 0,
  "average_duration_ms": 0,
  "outcomes": {
    "completed": 0,
    "failed": 0,
    "timed_out": 0,
    "cancelled": 0,
    "owner_shutdown": 0
  },
  "by_program": []
}
```

The JSON values are the same aggregate values represented by the human
report. Durations are integer milliseconds so scripts can aggregate without
floating-point parsing.

## Clear

```text
longrun gain --clear
```

The command exits successfully after removing only the local execution
metrics. It does not execute, cancel, or alter a target command. With
`--json`, it writes:

```json
{"cleared":true}
```

## Global JSON flag

The existing global `--json` flag remains accepted before the subcommand:

```text
longrun --json gain
```

`longrun gain --json` is the documented form. If both forms are supplied, the
result remains JSON rather than producing duplicate output.

## Error and privacy behavior

- Metrics read/write failures are reported as diagnostics without changing a
  target's already-determined exit status or causing a rerun.
- Invalid metric records are ignored and never treated as successful
  executions.
- The report never prints raw command arguments, command output, working
  directories, prompts, credentials, request bodies, or token-savings
  estimates.

## Management-command collision

`gain` is a Longrun management command, not an executable target:

```text
longrun gain
rtk longrun gain
```

An external executable named `gain` must use the explicit separator:

```text
longrun -- gain
```

The same distinction applies when the Codex hooks inspect the command: the
management form is passed through as a short CLI operation, while the
explicit-separator form is treated as a Longrun target.
