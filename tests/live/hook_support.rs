#![cfg(unix)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
use uuid::Uuid;

pub fn test_root(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("longrun-live-{prefix}-{}", Uuid::now_v7()));
    fs::create_dir_all(root.join("home")).expect("home");
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join("data")).expect("data");
    fs::create_dir_all(root.join("runtime")).expect("runtime");
    let config = "[execution]\nallow_danger_full_access = true\n";
    let config_dir = if cfg!(target_os = "macos") {
        root.join("home/Library/Application Support/dev.longrun.Longrun")
    } else {
        root.join("config/longrun")
    };
    fs::create_dir_all(&config_dir).expect("config");
    fs::write(config_dir.join("config.toml"), config).expect("config");
    root
}

pub fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn run_hook(root: &Path, hook: &str, input: &Value, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_longrun"));
    command
        .args(["hook", "codex", hook])
        .current_dir(std::env::current_dir().expect("cwd"))
        .env("HOME", root.join("home"))
        .env("PATH", command_path(root))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"));
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start hook");
    serde_json::to_writer(child.stdin.as_mut().expect("hook stdin"), input)
        .expect("write hook input");
    child.wait_with_output().expect("wait hook")
}

pub fn run_hooked_target(
    root: &Path,
    command: &str,
    env: &[(&str, &str)],
    session_id: &str,
    turn_id: &str,
    tool_use_id: &str,
) -> Value {
    let cwd = std::env::current_dir().expect("cwd");
    let pre_input = json!({
        "session_id": session_id,
        "turn_id": turn_id,
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": "PreToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_use_id": tool_use_id
    });
    let pre = run_hook(root, "pre-tool-use", &pre_input, env);
    assert!(
        pre.status.success(),
        "pre hook failed: {}",
        String::from_utf8_lossy(&pre.stderr)
    );
    let pre_json = json_output(&pre);
    let rewritten = pre_json["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap_or_else(|| panic!("missing rewritten command: {pre_json:?}"));
    let receipt = Command::new("/bin/sh")
        .args(["-c", rewritten])
        .current_dir(&cwd)
        .env("HOME", root.join("home"))
        .env("PATH", command_path(root))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .output()
        .expect("run receipt");
    assert!(
        receipt.status.success(),
        "receipt failed: {}",
        String::from_utf8_lossy(&receipt.stderr)
    );
    let post_input = json!({
        "session_id": session_id,
        "turn_id": turn_id,
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": "PostToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {"command": rewritten},
        "tool_response": {
            "output": String::from_utf8(receipt.stdout).expect("receipt text")
        },
        "tool_use_id": tool_use_id
    });
    let post = run_hook(root, "post-tool-use", &post_input, env);
    assert!(
        post.status.success(),
        "post hook failed: {}",
        String::from_utf8_lossy(&post.stderr)
    );
    json_output(&post)
}

fn command_path(root: &Path) -> String {
    format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").expect("PATH")
    )
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid hook output ({error}): {output:?}"))
}
