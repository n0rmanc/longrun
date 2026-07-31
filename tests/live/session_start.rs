#![cfg(unix)]

use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use longrun::{
    protocol::{
        DeliveryState, EnvironmentPolicy, ExecutionMode, ExecutionState, JobResult,
        JobSpecification, NativeString, ShellMode,
    },
    store::Store,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[test]
#[ignore = "live SessionStart hook harness; run with --ignored"]
fn session_start_delivers_a_completed_result_once_through_the_real_hook_cli() {
    let root = std::env::temp_dir().join(format!("longrun-session-start-live-{}", Uuid::now_v7()));
    fs::create_dir_all(&root).expect("root");
    let input = json!({
        "session_id": "recovered-session",
        "transcript_path": null,
        "cwd": std::env::current_dir().expect("cwd"),
        "hook_event_name": "SessionStart",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "source": "resume"
    });
    let first = run_hook(&root, &input);
    assert!(first.status.success(), "initial hook failed: {first:?}");
    assert!(first.stdout.is_empty(), "initial hook must be a no-op");

    let database = find_database(&root);
    let job = JobSpecification {
        protocol_version: 1,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string(OsString::from("echo")),
        args: vec![NativeString::from_os_string(OsString::from("done"))],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Durable,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:session-start-live".into(),
    };
    let mut store = Store::open(&database).expect("store");
    store
        .create_job_for_session(&job, Some("recovered-session"))
        .expect("job");
    store.claim_execution(job.job_id, "worker").expect("claim");
    store.mark_running(job.job_id, "worker").expect("running");
    store
        .finish_execution(
            &JobResult {
                job_id: job.job_id,
                terminal_state: ExecutionState::Succeeded,
                exit_code: Some(0),
                signal: None,
                duration_ms: 1,
                stdout_log: NativeString::from_os_string("/tmp/stdout".into()),
                stderr_log: NativeString::from_os_string("/tmp/stderr".into()),
                stdout_tail: "done".into(),
                stderr_tail: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                result_hash: "sha256:session-start-live-result".into(),
                completed_at_ms: 2,
            },
            "worker",
        )
        .expect("complete");
    drop(store);

    let recovered = run_hook(&root, &input);
    assert!(
        recovered.status.success(),
        "recovery hook failed: {recovered:?}"
    );
    let output: Value = serde_json::from_slice(&recovered.stdout)
        .unwrap_or_else(|error| panic!("invalid hook JSON ({error}): {recovered:?}"));
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("recovery context");
    assert!(context.contains("Longrun is active at "));
    assert!(context.contains("delivery idempotency key:"));
    assert!(context.contains("Bounded stdout (base64url):"));
    assert_eq!(
        Store::open(&database)
            .expect("reopen")
            .status(job.job_id)
            .expect("status")
            .delivery_state,
        DeliveryState::DeliveredOnStart
    );

    let duplicate = run_hook(&root, &input);
    assert!(
        duplicate.status.success(),
        "duplicate hook failed: {duplicate:?}"
    );
    assert!(
        duplicate.stdout.is_empty(),
        "delivered results must not repeat"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

fn run_hook(root: &Path, input: &Value) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_longrun"))
        .args(["hook", "codex", "session-start"])
        .current_dir(std::env::current_dir().expect("cwd"))
        .env("HOME", root.join("home"))
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

fn find_database(root: &Path) -> PathBuf {
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
    panic!("Longrun state database was not created");
}
