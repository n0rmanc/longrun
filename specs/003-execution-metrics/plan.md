# Implementation Plan: Local Execution Metrics

**Branch**: `003-execution-metrics` | **Date**: 2026-08-02 | **Spec**:
[spec.md](spec.md)

**Input**: Feature specification from
`/specs/003-execution-metrics/spec.md`

## Summary

Add a small local metrics module and a public `longrun gain` subcommand.
Longrun will record one minimal metric after each direct or Codex-hook target
returns a terminal `ResultEnvelope`. `gain` will aggregate those records into
human-readable or JSON totals, outcome counts, and per-executable summaries.
`gain --clear` will delete only the metrics history.

The implementation reuses the existing `Runner`, `ResultEnvelope`,
`ExecutionMode`, `AppPaths`, `serde_json`, and `uuid` dependencies. It adds no
worker, daemon, queue, database, retry, recovery, provider integration, or
token-savings estimate.

## Technical Context

**Language/Version**: Rust 1.88, edition 2024

**Primary Dependencies**: Existing `clap`, `serde`, `serde_json`, `uuid`,
`tokio`, and `directories`; no new dependency

**Storage**: Private local metric records below `AppPaths.data_dir`; existing
configuration, handoff, and integration directories remain separate

**Testing**: Existing `cargo test --locked` integration/unit tests plus focused
metrics, CLI, and hook tests; repository quality gates are `cargo fmt --check`
and `cargo clippy --all-targets -- -D warnings`

**Target Platform**: Existing Longrun macOS, Linux, and Windows CLI targets;
storage operations must use cross-platform standard-library filesystem APIs

**Project Type**: Standalone Rust CLI with a thin Codex `PreToolUse` /
`PostToolUse` adapter

**Performance Goals**: On the development machine used for implementation,
`longrun gain` scans 10,000 valid local records in under one second; recording
adds one small local write after terminal completion and makes no network or
model request

**Constraints**: Preserve the target's actual exit status and hook result if
metrics I/O fails; never store raw arguments, output, prompts, credentials,
request bodies, or working directories; do not add durable execution/result
infrastructure; do not count the internal receipt

**Scale/Scope**: One local user's history, up to at least 10,000 records for
the first version; no repository, session, or remote-service synchronization

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evidence |
|---|---|---|
| I. Eliminate Model Polling | PASS | Metrics are written after the existing local runner returns; `gain` performs no model or network polling. |
| II. CLI Is the Product | PASS | `longrun gain` is a direct public CLI command; direct and hook paths reuse the existing runner. |
| III. Continue the Same Work | PASS | No worker, background recovery, or second Codex process is introduced. |
| IV. One Handoff, One Owned Execution | PASS | Recording occurs only after the already-claimed target has one terminal result; metrics never retry or re-execute it. |
| V. Preserve Security Boundaries | PASS | Only an executable basename and terminal metadata are stored; no permission profile widening, shell evaluation, or secret inheritance is added. |
| VI. Keep Context Small and Evidence Local | PASS | Metric records contain no result output, prompt, arguments, or delivery state. They are aggregate observability metadata, not recoverable completed-result rows. |
| Quality Gates | PASS | Focused tests cover direct recording, hook recording, clear, JSON, invalid records, and privacy; repository gates remain mandatory. |

No constitutional exception is required.

## Research Summary

Research decisions are recorded in [research.md](research.md):

1. Use the existing terminal `ResultEnvelope` as the only timing/outcome
   source.
2. Call one metrics module from both direct dispatch and `PostToolUse`.
3. Use private per-record JSON files with atomic temporary-file rename, avoiding
   a new cross-process lock or dependency.
4. Group by executable basename and never by arguments.
5. Use exact integer milliseconds internally and in JSON; format human output.
6. Keep metrics failures diagnostic and never alter or rerun the target.
7. Do not estimate provider request or token savings.

## Design

### Command surface and parsing

- Add a visible `Gain(GainArgs)` variant to `src/cli.rs`.
- `GainArgs` supports `--json` and `--clear`.
- Dispatch `longrun gain` before the external target fallback.
- Update the hook command recognizer so management commands, including
  `gain`, are passed through normally instead of being rewritten as target
  programs. Preserve the explicit-separator rule: `longrun -- gain` remains
  an external target and is eligible for the wait adapter.
- Accept both documented `longrun gain --json` and the existing global
  `longrun --json gain`; combine the flags so JSON is emitted once.
- `--clear` performs only metrics deletion. With JSON enabled it returns the
  `{"cleared":true}` contract; without JSON it prints a short confirmation.
- The hidden `internal receipt` command remains unchanged and is never routed
  through metrics.

### Shared metric recording

Add `src/metrics.rs` and export it from `src/lib.rs`.

The module owns:

- `ExecutionMetric`, `GainReport`, `OutcomeCounts`, and `ProgramSummary`;
- terminal outcome classification from `TerminalReason` and `exit_code`;
- executable-basename extraction from `TargetSpec.program`;
- atomic record publication below `paths.data_dir`;
- invalid-record skipping;
- report aggregation and human/JSON serialization helpers;
- metrics-only clear.

The record path is:

1. Build the minimal record from `TargetSpec`, `ExecutionMode`, terminal
   `ResultEnvelope`, and the completion timestamp.
2. Serialize it with the existing JSON dependency.
3. Write a uniquely named temporary file in the metrics directory.
4. Rename it to a final record filename so readers see either no record or one
   complete record.

Readers scan only final record files, validate required fields, aggregate valid
records, and ignore incomplete/invalid files. No stdout/stderr or target
arguments are serialized.

### Direct execution path

In `src/cli.rs`, after `Runner::execute` returns in `execute_target`, call the
metrics recorder with `ExecutionMode::Direct`. Emit a diagnostic on recorder
failure but return the target-derived `ExitCode` unchanged. JSON target output
continues to contain the existing `ResultEnvelope`, not metric storage data.

### Codex hook execution path

In `src/hook/post_tool_use.rs`, after the shared runner returns its terminal
result and the claimed handoff is removed, call the same metrics recorder with
`ExecutionMode::CodexHook`. Return the existing bounded untrusted result to the
same turn. A duplicate or forged `PostToolUse` still claims nothing and starts
no target, so it also records nothing.

### Aggregation and output

`GainReport` contains:

- `recorded_executions`;
- `total_duration_ms`;
- integer `average_duration_ms`;
- fixed outcome counters for completed, failed, timed out, cancelled, and
  owner shutdown;
- sorted per-program summaries with count, total, and average duration.

Human output formats the same millisecond values into readable duration text
and keeps the report compact. JSON follows
[contracts/cli.md](contracts/cli.md) and contains exact integer milliseconds.
The report does not include a token-savings percentage, request estimate,
arguments, output, or local paths.

### Clear and failure behavior

- `longrun gain --clear` removes only the metrics directory and succeeds when
  it is already absent.
- Configuration, handoff, runtime, and Codex integration state are untouched.
- Clear has a linearization point when its deletion succeeds: records
  published before that point may be removed, while records published after it
  are retained in the new measurement period. Clear never stops a target.
- Metrics read/write errors are diagnostics. They must not rerun a command,
  change its exit status, or replace the hook's terminal result.

### Testing strategy

Add focused tests without a new test framework:

- `tests/metrics.rs`: record classification, basename grouping, JSON
  aggregation, invalid-record skipping, atomic record visibility, clear
  ordering, and privacy fields.
- `tests/cli.rs`: parse `gain`, `gain --json`, global JSON, and `gain --clear`;
  verify the management/target collision separator, and run direct commands
  in isolated XDG directories to verify totals/exit status.
- `tests/hooks.rs`: verify direct and hook executions both record once and
  duplicate `PostToolUse` does not add a second metric; verify `longrun gain`
  is not rewritten while `longrun -- gain` is.
- Existing runner/handoff/security tests remain unchanged except for the
  smallest assertions needed to prove metrics do not create result/output
  persistence.

## Project Structure

### Documentation (this feature)

```text
specs/003-execution-metrics/
├── spec.md
├── checklists/requirements.md
├── plan.md
├── research.md
├── data-model.md
├── contracts/cli.md
├── quickstart.md
└── tasks.md
```

### Source Code (repository root)

```text
src/
├── cli.rs                    # gain command, dispatch, direct recording
├── hook/post_tool_use.rs     # Codex-hook recording after terminal result
├── lib.rs                    # export metrics module
└── metrics.rs                # new local record and aggregation logic

tests/
├── cli.rs                    # command parsing and direct CLI integration
├── hooks.rs                  # hook recording and duplicate protection
└── metrics.rs                # new focused metrics tests

README.md                     # gain usage and measured-scope disclaimer
```

**Structure Decision**: Keep the feature in the existing single Rust CLI.
Metrics are a sibling module to the runner and are called at the two existing
terminal-result boundaries; no service, worker, database, or new process
architecture is added.

## Implementation Phases

### Phase 0 - Contract and data model

1. Add the metrics data types, terminal classification, basename normalization,
   and local record read/write/clear behavior.
2. Add focused metrics tests for valid, invalid, private, and concurrent-safe
   record handling.

### Phase 1 - CLI report

1. Add the public `gain` subcommand and its `--json` / `--clear` options.
2. Record direct target completions without changing target exit behavior.
3. Add CLI integration tests for empty, populated, timeout, JSON, and clear
   reports.

### Phase 2 - Codex hook integration

1. Record `PostToolUse` terminal results through the same metrics module.
2. Verify duplicate, forged, and incomplete hook paths do not record.
3. Preserve the existing same-turn bounded result contract.

### Phase 3 - Documentation and gates

1. Update `README.md` with `gain`, `--json`, `--clear`, and the explicit
   no-token-estimate boundary.
2. Run quickstart scenarios and all repository quality gates.

## Verification Matrix

| Requirement area | Evidence |
|---|---|
| Direct terminal executions | Isolated CLI test records success, nonzero exit, and timeout with the runner duration. |
| Codex hook executions | Hook test records one terminal result and confirms duplicate `PostToolUse` adds nothing. |
| Management/target collision | CLI and hook tests keep `longrun gain` as management and require `longrun -- gain` for an external target. |
| Global totals and minutes | Aggregation test verifies count, total milliseconds, average, and human formatting. |
| Outcome counts | Classification tests cover completed, failed, timed out, cancelled, and owner shutdown, and assert the counts sum to the total. |
| Per-program breakdown | Tests use multiple argument lists and verify basename grouping without arguments in output. |
| JSON contract | CLI test parses `gain --json` and compares values with human report semantics. |
| Clear scope | Integration test verifies metrics reset while config/handoff/integration paths remain. |
| Privacy | Serialized record/output assertions prove no args, output, cwd, prompt, credentials, or request body fields. |
| Concurrency and invalid data | Metrics tests publish distinct records concurrently and ignore incomplete/invalid files. |
| Performance | A 10,000-record aggregation check verifies the report meets the stated local scan goal. |
| No token claim | Contract, README, and JSON assertions contain no savings/request estimate field. |
| No model polling or new worker | Source review plus existing lifecycle tests show only post-terminal local writes. |

## Post-Design Constitution Re-check

All gates remain **PASS** after design:

- The design has one CLI command and one recorder, not a second execution
  backend.
- Per-record metric files are minimal observability metadata and exclude
  completed-result content, delivery state, and output logs.
- Atomic publication handles concurrent completion without a daemon, lock
  service, retry loop, or durable job queue.
- Clear only deletes metrics and never changes ownership, permissions, or
  target lifecycle.
- Direct and hook execution still use the same `Runner` and exact terminal
  result.

## Complexity Tracking

No constitution violations or complexity exceptions.
