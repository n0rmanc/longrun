#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use longrun::{
    protocol::{DeliveryState, ExecutionState},
    store::Store,
};
use serde_json::{Value, json};
use uuid::Uuid;

const DEFAULT_DURATION_SECONDS: u64 = 90;
const ACCEPTANCE_DURATION_SECONDS: u64 = 30 * 60;

#[test]
#[ignore = "live durable Codex termination/restart harness; run with --ignored"]
fn durable_job_survives_origin_termination_and_recovers_once_on_session_restart() {
    let duration_seconds = configured_duration_seconds();
    let root = std::env::temp_dir().join(format!("longrun-durable-live-{}", Uuid::now_v7()));
    let runtime_id = Uuid::now_v7().simple().to_string();
    let runtime_dir = std::env::temp_dir().join(format!("lr-{}", &runtime_id[..8]));
    fs::create_dir_all(root.join("bin")).expect("root");
    fs::create_dir_all(&runtime_dir).expect("runtime");
    let starts = root.join("starts.log");
    let codex_log = root.join("codex.log");
    let fixture = root.join("durable-command");
    fs::write(
        &fixture,
        "#!/bin/sh\nprintf x >> \"$2\"\nsleep \"$1\"\nprintf durable-complete\n",
    )
    .expect("fixture");
    fs::set_permissions(&fixture, fs::Permissions::from_mode(0o755)).expect("fixture mode");
    let codex = root.join("bin/codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\nif [ \"$1\" != sandbox ]; then exit 0; fi\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
            shell_quote_path(&codex_log)
        ),
    )
    .expect("codex");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("codex mode");

    let mut daemon = command(&root, &runtime_dir, env!("CARGO_BIN_EXE_longrun"))
        .args(["daemon", "--foreground"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start daemon");
    wait_for_socket(&runtime_dir, &mut daemon);

    let cwd = std::env::current_dir().expect("cwd");
    let session_id = "durable-session";
    let pre = json!({
        "session_id": session_id,
        "turn_id": "durable-turn",
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": "PreToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {
            "command": format!(
                "\"{}\" submit --mode durable -- \"{}\" {} \"{}\"",
                env!("CARGO_BIN_EXE_longrun"),
                fixture.display(),
                duration_seconds,
                starts.display(),
            )
        },
        "tool_use_id": "durable-tool"
    });
    let pre = run_hook(&root, &runtime_dir, "pre-tool-use", &pre);
    assert!(pre.status.success(), "pre hook failed: {pre:?}");
    let rewritten = json_output(&pre)["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .expect("rewritten command")
        .to_owned();
    let receipt = command(&root, &runtime_dir, "/bin/sh")
        .args(["-c", &rewritten])
        .current_dir(&cwd)
        .output()
        .expect("submit");
    assert!(receipt.status.success(), "submit failed: {receipt:?}");

    let post = json!({
        "session_id": session_id,
        "turn_id": "durable-turn",
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": "PostToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {"command": "ignored"},
        "tool_response": {"output": String::from_utf8(receipt.stdout).expect("receipt")},
        "tool_use_id": "durable-tool"
    });
    let mut origin = start_hook(&root, &runtime_dir, "post-tool-use", &post);
    wait_for_text(&starts, "x");
    origin.kill().expect("terminate originating Codex hook");
    origin.wait().expect("reap originating hook");

    let database = find_database(&root);
    let job_id = wait_for_terminal(&database);
    let mut store = Store::open(&database).expect("store");
    assert_eq!(
        store.status(job_id).expect("status").delivery_state,
        DeliveryState::HookLeased,
        "the terminated origin must retain delivery ownership until expiry"
    );
    store
        .expire_delivery_leases(i64::MAX)
        .expect("expire terminated-origin lease");
    drop(store);

    let restart = json!({
        "session_id": session_id,
        "transcript_path": null,
        "cwd": cwd,
        "hook_event_name": "SessionStart",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "source": "resume"
    });
    let recovered = run_hook(&root, &runtime_dir, "session-start", &restart);
    assert!(recovered.status.success(), "recovery failed: {recovered:?}");
    let recovered_json = json_output(&recovered);
    let context = recovered_json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("recovery context");
    assert!(context.contains("Recovered result (delivery idempotency key:"));
    assert!(context.contains("State: Succeeded"));
    assert_eq!(
        Store::open(&database)
            .expect("reopen")
            .status(job_id)
            .expect("status")
            .delivery_state,
        DeliveryState::DeliveredOnStart
    );

    let duplicate = run_hook(&root, &runtime_dir, "session-start", &restart);
    assert!(
        duplicate.status.success(),
        "duplicate restart failed: {duplicate:?}"
    );
    assert!(
        duplicate.stdout.is_empty(),
        "one completed job must produce one recovery delivery"
    );
    assert_eq!(fs::read_to_string(&starts).expect("starts"), "x");
    assert_eq!(
        fs::read_to_string(&codex_log)
            .expect("codex log")
            .lines()
            .collect::<Vec<_>>(),
        vec!["sandbox"],
        "recovery must not re-execute the command or start disabled automatic resume"
    );

    daemon.kill().expect("stop daemon");
    daemon.wait().expect("reap daemon");
    fs::remove_dir_all(root).expect("cleanup");
    fs::remove_dir_all(runtime_dir).expect("cleanup runtime");
}

fn configured_duration_seconds() -> u64 {
    match std::env::var("LONGRUN_DURABLE_SESSION_SECONDS") {
        Ok(value) => value
            .parse()
            .expect("LONGRUN_DURABLE_SESSION_SECONDS must be an integer"),
        Err(_) => DEFAULT_DURATION_SECONDS,
    }
}

fn command(root: &Path, runtime_dir: &Path, program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("TMPDIR", runtime_dir)
        .env(
            "PATH",
            format!(
                "{}:{}",
                root.join("bin").display(),
                std::env::var("PATH").expect("PATH")
            ),
        );
    command
}

fn run_hook(root: &Path, runtime_dir: &Path, hook: &str, input: &Value) -> std::process::Output {
    let child = start_hook(root, runtime_dir, hook, input);
    child.wait_with_output().expect("wait hook")
}

fn start_hook(root: &Path, runtime_dir: &Path, hook: &str, input: &Value) -> Child {
    let mut child = command(root, runtime_dir, env!("CARGO_BIN_EXE_longrun"))
        .args(["hook", "codex", hook])
        .current_dir(std::env::current_dir().expect("cwd"))
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
    child
}

fn json_output(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid hook output ({error}): {output:?}"))
}

fn wait_for_socket(runtime_dir: &Path, daemon: &mut Child) {
    for _ in 0..200 {
        if find_socket(runtime_dir).is_some() {
            return;
        }
        if let Some(status) = daemon.try_wait().expect("check daemon") {
            let mut stderr = String::new();
            daemon
                .stderr
                .take()
                .expect("daemon stderr")
                .read_to_string(&mut stderr)
                .expect("read daemon stderr");
            panic!("supervisor exited before binding ({status}): {stderr}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("supervisor socket was not created");
}

fn find_socket(root: &Path) -> Option<PathBuf> {
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            let file_type = entry.file_type().ok()?;
            if file_type.is_socket() {
                return Some(path);
            }
            if file_type.is_dir() {
                directories.push(path);
            }
        }
    }
    None
}

fn find_database(root: &Path) -> PathBuf {
    for _ in 0..200 {
        let mut directories = vec![root.to_path_buf()];
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory).expect("read state directory") {
                let entry = entry.expect("entry");
                let path = entry.path();
                if entry.file_type().expect("type").is_dir() {
                    directories.push(path);
                } else if entry.file_name() == "longrun.sqlite" {
                    return path;
                }
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("Longrun state database was not created");
}

fn wait_for_text(path: &Path, expected: &str) {
    for _ in 0..400 {
        if fs::read_to_string(path).is_ok_and(|contents| contents.contains(expected)) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {expected:?} in {}", path.display());
}

fn wait_for_terminal(database: &Path) -> Uuid {
    for _ in 0..400 {
        let statuses = Store::open(database)
            .expect("store")
            .list(None)
            .expect("jobs");
        if let Some(status) = statuses.first()
            && status.execution_state.is_terminal()
        {
            assert_eq!(status.execution_state, ExecutionState::Succeeded);
            return status.job_id;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for durable result");
}

fn shell_quote_path(path: &Path) -> String {
    path.display().to_string().replace('\'', "'\"'\"'")
}

#[test]
fn live_duration_defaults_to_ninety_seconds_and_acceptance_is_thirty_minutes() {
    assert_eq!(DEFAULT_DURATION_SECONDS, 90);
    assert_eq!(ACCEPTANCE_DURATION_SECONDS, 30 * 60);
}
