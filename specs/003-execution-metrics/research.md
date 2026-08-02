# Research: Local Execution Metrics

## Decision 1: Reuse the existing runner result as the metric source

- **Decision**: Record metrics only after `Runner::execute` returns a terminal
  `ResultEnvelope`.
- **Rationale**: The runner already owns duration, exit status, timeout,
  cancellation, owner-shutdown, output draining, and process cleanup. A
  second timer or execution path would create conflicting truth.
- **Alternatives considered**:
  - Start and stop a timer in the CLI and hook layers: rejected because it
    duplicates runner timing and would diverge between direct and hook modes.
  - Record at command submission: rejected because incomplete or lost-owner
    executions must not appear in the report.

## Decision 2: Record both execution entry points through one metrics module

- **Decision**: Add one small metrics module and call it from direct target
  dispatch and `PostToolUse` after the shared runner returns.
- **Rationale**: Direct CLI and Codex hooks are the two supported execution
  paths, and the constitution requires them to share one runner. One recorder
  keeps the metric schema and terminal classification identical.
- **Alternatives considered**:
  - Add metrics only to the CLI: rejected because Codex hook executions would
    be missing from the global report.
  - Add a worker or supervisor to observe executions: rejected by the
    constitution and unnecessary for a terminal-result metric.

## Decision 3: Use local per-record files with atomic publication

- **Decision**: Store one minimal JSON metric record per completed execution in
  a private metrics directory below `AppPaths.data_dir`. Write a temporary
  file and rename it to its final record name; scan final record files for
  `gain`.
- **Rationale**: Unique record names plus atomic rename avoid a cross-process
  lock and prevent concurrent direct and hook completions from interleaving
  JSON. The existing `uuid` and `serde_json` dependencies are sufficient.
  Temporary files are ignored by readers, and `--clear` removes only the
  metrics directory.
- **Alternatives considered**:
  - Append-only JSONL: smaller steady-state format, but concurrent writers can
    produce partial or interleaved records without a portable file lock.
  - SQLite or a job database: durable infrastructure that conflicts with the
    project's ephemeral architecture and adds a dependency.
  - In-memory counters: cannot provide a global report after a Longrun process
    exits.

## Decision 4: Aggregate by executable basename only

- **Decision**: Use the invoked program's basename as the per-program key.
- **Rationale**: The user needs to know whether `gh`, `cargo`, or `oracle`
  consumes the wait. Excluding arguments and directory prefixes keeps the
  report compact and avoids storing sensitive command details.
- **Alternatives considered**:
  - Group by the complete executable path: rejected because equivalent
    programs launched through different paths would fragment the report and
    expose local path details.
  - Group by the complete command line: rejected because it stores arguments
    and produces noisy, high-cardinality output.

## Decision 5: Keep output values exact in milliseconds and format human output

- **Decision**: Use integer millisecond totals and averages in the internal
  report and JSON output; format the same values as readable minutes and
  seconds in the human report.
- **Rationale**: Integer milliseconds avoid floating-point rounding in scripts,
  while the human report answers the user's "how many minutes" question.
- **Alternatives considered**:
  - Store only rounded minutes: rejected because short commands disappear and
    totals lose precision.
  - Use floating-point seconds everywhere: rejected because exact aggregation
    is less predictable for machine consumers.

## Decision 6: Classify outcomes from the existing terminal reason and exit code

- **Decision**: Classify an exited process with exit code zero as `completed`,
  an exited process with a nonzero or missing exit code as `failed`, and retain
  `timed_out`, `cancelled`, and `owner_shutdown` as separate outcomes.
- **Rationale**: This answers the requested completion, timeout, and
  cancellation counts without inventing a second result taxonomy. A nonzero
  process exit is materially different from a timeout or owner shutdown.
- **Alternatives considered**:
  - Count every `Exited` result as completed: rejected because failed target
    commands would look successful.
  - Record only a success boolean: rejected because it hides useful terminal
    distinctions.

## Decision 7: Metrics failures never rerun or change the target result

- **Decision**: Treat a local metrics write/read failure as a diagnostic
  warning; preserve the target's actual exit code and hook result.
- **Rationale**: Metrics are observability, not execution control. A full or
  unwritable local disk must not cause a long-running command to be started
  twice or make a successful target appear failed.
- **Alternatives considered**:
  - Fail the target command when metrics cannot be written: rejected because
    it changes the product's primary execution contract.
  - Retry writes in a background worker: rejected by the constitution and
    outside the feature.

## Decision 8: Do not estimate token savings

- **Decision**: `gain` reports only locally measured Longrun executions and
  elapsed time.
- **Rationale**: Longrun does not receive Codex/provider request counters or
  token usage. A savings percentage would be fabricated and misleading.
- **Alternatives considered**:
  - Estimate saved model requests from command duration: rejected because
    command duration does not prove how many model requests would otherwise
    have occurred.
