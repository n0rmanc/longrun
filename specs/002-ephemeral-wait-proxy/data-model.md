# Data Model: Ephemeral RTK-Style Wait Proxy

## EphemeralHandoff

Represents one visible Longrun invocation while it crosses from PreToolUse to
PostToolUse.

| Field | Type | Rules |
| --- | --- | --- |
| `schema_version` | integer | Required; reject unsupported versions. |
| `handle_hash` | protected bytes | Hash of a high-entropy opaque marker; never persist the raw marker. |
| `session_id` | string | Must match the active Codex hook input. |
| `turn_id` | string | Must match the originating active turn. |
| `tool_use_id` | string | Primary one-shot invocation identity. |
| `cwd` | native path | Canonical path captured at preparation. |
| `visible_command_hash` | digest | Binds the original user-visible Longrun command. |
| `stub_command_hash` | digest | Binds the generated fast receipt stub. |
| `program` | native string | Target program to execute. |
| `args` | list of native strings | Passed literally; no shell reconstruction. |
| `permission_profile` | optional string | Required and immutable for Codex-hook execution; absent for direct terminal/CI execution. |
| `environment_policy` | policy value | Allowlist and deny patterns captured for the handoff. |
| `timeout_ms` | unsigned integer | Must leave margin for cleanup and result finalization. |
| `termination_grace_ms` | unsigned integer | Used before forced tree termination. |
| `forced_cleanup_margin_ms` | unsigned integer | Bounds the post-grace forced-cleanup window. |
| `result_serialization_margin_ms` | unsigned integer | Bounds final result construction and hook serialization. |
| `post_tool_use_timeout_ms` | unsigned integer | MUST be at least `timeout_ms + termination_grace_ms + forced_cleanup_margin_ms + result_serialization_margin_ms`. |
| `output_limit_bytes` | unsigned integer | Bounds model-visible rolling tails. |
| `created_at` | timestamp | Used for expiry and diagnostics. |
| `expires_at` | timestamp | Derived from `handoff_ttl_ms`; default 300000 ms and maximum 900000 ms; expired handoffs execute nothing. |
| `state` | enum | `prepared`, `armed`, or `claimed`; only forward transitions. |

### Handoff State Transitions

```text
prepared --receipt stub arms--> armed
armed --PostToolUse atomically claims--> claimed
claimed --execution finalizes--> deleted

prepared/armed --expiry or invalidation--> deleted
claimed --crash--> inert until TTL cleanup, never retried
```

## TargetExecution

An in-memory execution owned by the active PostToolUse process.

| Field | Type | Rules |
| --- | --- | --- |
| `program` | native string | Comes only from the claimed handoff. |
| `args` | list of native strings | Passed literally to the sandbox launcher. |
| `started_at` | timestamp | Set after the target is spawned. |
| `terminal_reason` | enum | `exited`, `signaled`, `timed_out`, `cancelled`, `owner_shutdown`, or `spawn_failed`. |
| `exit_code` | optional integer | Exact OS exit status when available; otherwise absent. |
| `stdout_bytes` | unsigned integer | Total bytes drained. |
| `stderr_bytes` | unsigned integer | Total bytes drained. |
| `stdout_tail` | bounded bytes | Rolling tail only. |
| `stderr_tail` | bounded bytes | Rolling tail only. |
| `stdout_truncated` | boolean | True when older stdout was discarded. |
| `stderr_truncated` | boolean | True when older stderr was discarded. |
| `duration_ms` | unsigned integer | Monotonic elapsed duration. |

No TargetExecution is persisted after the active invocation finishes.

## ResultEnvelope

Model-visible or direct-terminal representation of one TargetExecution.

| Field | Type | Rules |
| --- | --- | --- |
| `schema` | string | Versioned result schema identifier. |
| `terminal_reason` | string | Required and stable. |
| `exit_code` | optional integer | Exact target value when supplied by the OS. |
| `duration_ms` | integer | Required. |
| `stdout_bytes` / `stderr_bytes` | integer | Required total counts. |
| `stdout_truncated` / `stderr_truncated` | boolean | Required. |
| `stdout_tail` / `stderr_tail` | escaped text | Bounded, escaped, and labeled untrusted data. |
| `untrusted_output` | boolean | Always true for target output. |

The ResultEnvelope is returned to the active turn or direct terminal only. It
is not a recovery record.

## Relationships

```text
one visible invocation
  └── one EphemeralHandoff
        └── at most one TargetExecution
              └── one ResultEnvelope
```
