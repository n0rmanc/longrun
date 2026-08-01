# Data Model: Long-Running Command Execution

## JobSpecification

Immutable description of a requested command.

| Field | Type | Rules |
|-------|------|-------|
| protocol_version | integer | Must equal a supported version |
| job_id | UUIDv7 | Unique and immutable |
| program | NativeString | Non-empty; preserved without shell reconstruction |
| args | ordered NativeString list | May be empty; order preserved |
| cwd | absolute native path | Must exist when execution starts |
| execution_mode | embedded or durable | Immutable after acceptance |
| shell_mode | direct or explicit-shell | Direct by default |
| timeout_ms | integer | Positive and within configured maximum |
| permission_profile | string | Defaults to `:workspace` |
| environment_policy | reference | Explicit allow and deny rules |
| created_at | timestamp | UTC |
| command_hash | digest | Covers execution-relevant fields |

Relationships:

- Has one `JobExecution`.
- Has one current `DeliveryRecord`.
- Is referenced by one consumed `SubmissionReceipt`.
- Produces zero or one `JobResult`.

## NativeString

Lossless JSON representation of an operating-system string.

| Field | Type | Rules |
|-------|------|-------|
| encoding | `utf8`, `unix_bytes`, or `windows_utf16le` | Matches source platform |
| value | string | Text for UTF-8; base64url bytes otherwise |

UTF-8 values use readable text. Non-UTF-8 Unix bytes and Windows UTF-16 code
units use base64url so receipts, IPC, and durable state never lose argument
identity.

## PendingSubmission

Hook-owned record created before the short submit command runs.

| Field | Type | Rules |
|-------|------|-------|
| session_id | string | Required |
| turn_id | string | Required |
| tool_use_id | string | Primary lookup key |
| cwd | native path | Must match hook input |
| binary_path | native path | Must match installed executable |
| command_hash | digest | Covers exact submit request |
| hook_token_hash | digest | Hash of opaque hook-owned token retained in the rewritten wrapper |
| signed_receipt | string | HMAC-signed receipt retained only in trusted pending state |
| created_at | timestamp | UTC |
| expires_at | timestamp | Short bounded lifetime |
| state | pending, claimed, consumed, rejected | Monotonic |

## SubmissionReceipt

Single-use versioned proof emitted by `longrun submit`.

| Field | Type | Rules |
|-------|------|-------|
| receipt_version | integer | Must equal 1 |
| job_specification | object | Must match pending command and context |
| session_id | string | Must match pending submission |
| turn_id | string | Must match pending submission |
| tool_use_id | string | Must match pending submission |
| nonce | random bytes | Unique |
| issued_at | timestamp | UTC |
| expires_at | timestamp | Must be in the future |
| signature | bytes | HMAC over canonical receipt payload |

Validation transition:

```text
issued -> consumed
       -> rejected
       -> expired
```

Only `issued -> consumed` permits execution. The transition and job creation
occur in one transaction.

## JobExecution

Authoritative process lifecycle.

```text
accepted -> starting -> running
running  -> succeeded
running  -> failed
running  -> timed_out
running  -> cancelled
starting -> failed
```

Terminal states are immutable. A job has at most one process identity and one
terminal state.

| Field | Type | Rules |
|-------|------|-------|
| job_id | UUIDv7 | Primary key |
| state | enum | Follows transition graph |
| owner_mode | embedded or supervisor | One owner at a time |
| owner_id | string | Hook or supervisor instance |
| execution_claim | random token or null | Exactly one worker may hold it |
| worker_id | UUID or null | Set before requested-command spawn |
| pid | integer or null | Present after spawn |
| process_group | platform identifier or null | Present when supported |
| started_at | timestamp or null | Set once |
| finished_at | timestamp or null | Set only with terminal state |
| exit_code | integer or null | Mutually compatible with signal |
| signal | string or null | Platform termination reason |
| failure_kind | string or null | Spawn, sandbox, storage, timeout, cancel |

## JobResult

Immutable completion evidence.

| Field | Type | Rules |
|-------|------|-------|
| job_id | UUIDv7 | Unique |
| terminal_state | enum | Matches execution |
| exit_code | integer or null | Preserved |
| signal | string or null | Preserved |
| duration_ms | integer | Non-negative |
| stdout_log | native path | Local file |
| stderr_log | native path | Local file |
| stdout_tail | bytes | Bounded |
| stderr_tail | bytes | Bounded |
| stdout_truncated | boolean | Accurate |
| stderr_truncated | boolean | Accurate |
| result_hash | digest | Covers immutable result metadata |
| completed_at | timestamp | UTC |

## DeliveryRecord

Independent completion-delivery lifecycle.

```text
undelivered -> hook_leased -> delivered_in_turn
undelivered -> session_start_leased -> delivered_on_start
undelivered -> resume_leased -> resume_started -> delivered_by_resume
any leased state -> undelivered (lease expiry or failed delivery)
```

Execution state never moves backward when delivery is retried.

| Field | Type | Rules |
|-------|------|-------|
| job_id | UUIDv7 | Primary key |
| session_id | string | Delivery target |
| state | enum | Follows transition graph |
| lease_id | UUID | Unique per attempt |
| lease_owner | string or null | Hook, session start, or resume process |
| lease_expires_at | timestamp or null | Required while leased |
| attempt_count | integer | Monotonic |
| delivered_at | timestamp or null | Set once |
| idempotency_key | string | Unique per final delivery |

Repeated uncertain delivery attempts retain the same idempotency key so the
receiver can identify one logical result even when the hook protocol cannot
acknowledge consumption.

## SupervisorInstance

Durable runtime owner.

| Field | Type | Rules |
|-------|------|-------|
| instance_id | UUID | Unique per process start |
| pid | integer | Current process |
| endpoint | native local IPC address | Per-user |
| started_at | timestamp | UTC |
| heartbeat_at | timestamp | Updated while healthy |
| protocol_version | integer | Compatibility gate |

## IntegrationInstallation

Tracks files and Codex selectors owned by Longrun.

| Field | Type | Rules |
|-------|------|-------|
| integration | `codex` | Initial integration |
| binary_path | absolute native path | From `current_exe()` |
| marketplace_name | string | `longrun-local` |
| plugin_selector | string | `longrun@longrun-local` |
| generated_root | native path | Longrun data directory |
| manifest_hash | digest | Detects repair need |
| installed_at | timestamp | UTC |

## Configuration

Validated user policy grouped into:

- execution: mode, timeout, concurrency, permission profile, shell and
  danger-full-access gates;
- output: tail lines, model byte limit, redaction;
- environment: safe inheritance, explicit pass-through, deny patterns;
- recovery: session-start delivery, automatic resume, retry budget;
- retention: age and total log-byte limits;
- diagnostics: log level and telemetry opt-in.
