#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use uuid::Uuid;

const DEFAULT_DURATION_SECONDS: u64 = 90;
const ACCEPTANCE_DURATION_SECONDS: u64 = 30 * 60;

#[test]
#[ignore = "live hook harness; run with --ignored"]
fn active_hook_waits_once_and_delivers_to_the_same_turn() {
    let duration_seconds = configured_duration_seconds();
    let root = std::env::temp_dir().join(format!("longrun-active-{}", Uuid::now_v7()));
    fs::create_dir_all(root.join("bin")).expect("root");
    let sandbox_log = root.join("sandbox.log");
    let starts = root.join("starts.log");
    let fixture = root.join("active-command");
    fs::write(
        &fixture,
        "#!/bin/sh\nprintf x >> \"$2\"\nsleep \"$1\"\nprintf DONE\n",
    )
    .expect("fixture");
    fs::set_permissions(&fixture, fs::Permissions::from_mode(0o755)).expect("fixture mode");
    let codex = root.join("bin/codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
            shell_quote_path(&sandbox_log)
        ),
    )
    .expect("sandbox");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("sandbox mode");

    let cwd = std::env::current_dir().expect("cwd");
    let pre = json!({
        "session_id": "active-session",
        "turn_id": "active-turn",
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": "PreToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {
            "command": format!(
                "\"{}\" -- \"{}\" {} \"{}\"",
                env!("CARGO_BIN_EXE_longrun"),
                fixture.display(),
                duration_seconds,
                starts.display(),
            )
        },
        "tool_use_id": "active-tool"
    });
    let pre = run_hook(&root, "pre-tool-use", &pre);
    assert!(pre.status.success(), "pre hook failed: {pre:?}");
    let rewritten = json_output(&pre)["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .expect("rewritten command")
        .to_owned();

    let issued = Instant::now();
    let receipt = Command::new("/bin/sh")
        .arg("-c")
        .arg(rewritten)
        .current_dir(&cwd)
        .env("HOME", root.join("home"))
        .env("PATH", command_path(&root))
        .output()
        .expect("run rewritten receipt");
    assert!(receipt.status.success(), "receipt failed: {receipt:?}");

    let post = json!({
        "session_id": "active-session",
        "turn_id": "active-turn",
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": "PostToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {"command": "ignored"},
        "tool_response": {"output": String::from_utf8(receipt.stdout).expect("receipt text")},
        "tool_use_id": "active-tool"
    });
    let post = run_hook(&root, "post-tool-use", &post);
    assert!(post.status.success(), "post hook failed: {post:?}");
    assert!(
        issued.elapsed() >= Duration::from_secs(duration_seconds.saturating_sub(1)),
        "PostToolUse returned before the target command finished"
    );
    assert_eq!(fs::read_to_string(&starts).expect("start count"), "x");
    assert_eq!(
        fs::read_to_string(&sandbox_log)
            .expect("sandbox log")
            .lines()
            .count(),
        1,
        "Longrun must make one sandbox invocation, not poll through a second execution path"
    );

    let output = json_output(&post);
    assert_eq!(output["continue"], false);
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("completion context");
    assert!(context.contains("Terminal reason: Exited"));
    assert_eq!(bounded_stdout(context), b"DONE");

    fs::remove_dir_all(root).expect("cleanup");
}

fn configured_duration_seconds() -> u64 {
    match std::env::var("LONGRUN_ACTIVE_SESSION_SECONDS") {
        Ok(value) => value
            .parse()
            .expect("LONGRUN_ACTIVE_SESSION_SECONDS must be an integer"),
        Err(_) => DEFAULT_DURATION_SECONDS,
    }
}

fn command_path(root: &std::path::Path) -> String {
    format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").expect("PATH")
    )
}

fn run_hook(root: &std::path::Path, hook: &str, input: &Value) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_longrun"))
        .args(["hook", "codex", hook])
        .current_dir(std::env::current_dir().expect("cwd"))
        .env("HOME", root.join("home"))
        .env("PATH", command_path(root))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start hook");
    child
        .stdin
        .take()
        .expect("hook stdin")
        .write_all(serde_json::to_string(input).expect("input JSON").as_bytes())
        .expect("write hook input");
    child.wait_with_output().expect("wait hook")
}

fn json_output(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid hook output ({error}): {output:?}"))
}

fn bounded_stdout(context: &str) -> Vec<u8> {
    let encoded = context
        .split("Bounded stdout (base64url):\n")
        .nth(1)
        .and_then(|value| value.split("\n\n").next())
        .expect("stdout tail");
    URL_SAFE_NO_PAD.decode(encoded).expect("base64 stdout")
}

fn shell_quote_path(path: &std::path::Path) -> String {
    path.display().to_string().replace('\'', "'\"'\"'")
}

#[test]
fn live_duration_defaults_to_ninety_seconds_and_acceptance_is_thirty_minutes() {
    assert_eq!(DEFAULT_DURATION_SECONDS, 90);
    assert_eq!(ACCEPTANCE_DURATION_SECONDS, 30 * 60);
}
