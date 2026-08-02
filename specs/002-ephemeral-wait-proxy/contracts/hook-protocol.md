# Hook Contract: Ephemeral RTK-Style Wait Proxy

## PreToolUse

Input: Codex Bash hook JSON with session, turn, tool-use, cwd, and command.

For an exact supported Longrun form, the hook:

1. Validates direct argument grammar and rejects unsupported shell composition.
2. Captures the immutable target and permission/output policy.
3. Creates a protected `prepared` handoff with a short expiry.
4. Returns an `updatedInput` command for the fast internal receipt stub.

For unrelated commands, the hook returns no Longrun output and does not change
the command.

The generated Unix hook command uses `exec` for the Longrun binary so the
Codex-owned hook process is not hidden behind an additional shell process.

## Receipt stub

The generated stub:

1. Runs as the rewritten Bash command.
2. Records shell-parsed native arguments if needed by the platform.
3. Atomically changes the handoff from `prepared` to `armed`.
4. Emits exactly one opaque marker.
5. Exits promptly without starting the target.

The deterministic hook harness MUST observe the receipt stub completing within
1000 ms.

The marker is not a user approval and is not the target result.

## PostToolUse

The hook accepts only one matching marker and validates:

- session ID;
- turn ID;
- tool-use ID;
- canonical cwd;
- visible command identity;
- generated stub identity;
- expiry;
- protected handoff state.

It atomically changes `armed` to `claimed`, launches the target through the
configured Codex sandbox, waits locally, and returns one bounded
`PostToolUse` result to the active turn. A nonzero target exit is result data,
not an automatic hook retry.

The installed PostToolUse timeout MUST satisfy:

```text
post_tool_use_timeout_ms >= target_timeout_ms
                       + termination_grace_ms
                       + forced_cleanup_margin_ms
                       + result_serialization_margin_ms
```

The hook deletes the handoff and all Longrun-owned transient output after
completion. If ownership is lost, it performs no recovery or redelivery.

## Result envelope

```json
{
  "schema": "longrun.result.v1",
  "terminal_reason": "exited",
  "exit_code": 0,
  "duration_ms": 0,
  "stdout_bytes": 0,
  "stderr_bytes": 0,
  "stdout_truncated": false,
  "stderr_truncated": false,
  "stdout_tail": "",
  "stderr_tail": "",
  "untrusted_output": true
}
```

Target output is escaped data nested inside this envelope. It MUST NOT be
interpreted as hook instructions or top-level hook JSON.
