# CLI Contract: Ephemeral RTK-Style Wait Proxy

## Canonical target form

```text
longrun PROGRAM ARG...
longrun -- PROGRAM ARG...
rtk longrun PROGRAM ARG...
rtk longrun -- PROGRAM ARG...
```

`--` disambiguates a target whose program name collides with a reserved
management command. Target arguments are passed literally and are not evaluated
as shell syntax.

## Reserved management commands

The implementation MAY retain only integration and diagnostic commands needed
to install and verify the thin adapter:

```text
longrun --version
longrun init --codex
longrun doctor
longrun uninstall --codex
```

The following legacy and durable commands are removed from the public contract:

```text
run
run-shell
submit
submit-shell
status
wait
list
logs
cancel
gc
service
daemon
mcp
```

Removed commands MUST return a clear migration error and MUST NOT create a
durable job.

The removed names remain reserved for this migration error. A real target whose
program name is one of those names MUST use the explicit separator, for example:

```text
longrun -- submit --help
```

## Direct execution result

Direct terminal and CI execution returns the target exit status. Normal output
and diagnostics remain separate. Timeout, cancellation, and owner shutdown
return a nonzero Longrun process status and identify the terminal reason in the
rendered result; no particular platform-independent numeric code is promised
for those runner-owned terminal reasons.

## Codex result limitation

The receipt stub exits before the target runs. The target's exact exit status is
returned in the model-visible result envelope; it cannot retroactively change
the receipt stub's process exit status.
