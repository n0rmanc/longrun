use std::path::Path;

use longrun::{
    hook::{
        input::{BashInput, PreToolUseInput},
        pre_tool_use::handle_pre_tool_use,
    },
    store::Store,
};

fn input(command: &str) -> PreToolUseInput {
    PreToolUseInput {
        session_id: "session".into(),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        cwd: std::env::current_dir().expect("cwd"),
        hook_event_name: "PreToolUse".into(),
        tool_name: "Bash".into(),
        tool_input: BashInput {
            command: command.into(),
        },
    }
}

#[cfg(unix)]
#[tokio::test]
async fn post_tool_use_consumes_a_receipt_waits_locally_and_rejects_replay() {
    use std::{fs, os::unix::fs::PermissionsExt};

    use longrun::{
        config::Config,
        hook::{input::PostToolUseInput, post_tool_use::handle_post_tool_use},
        paths::AppPaths,
        protocol::{
            EnvironmentPolicy, ExecutionMode, JobSpecification, NativeEncoding, NativeString,
            PendingState, PendingSubmission, ShellMode,
        },
        receipt::{ReceiptPayload, ReceiptSigner},
        runner::Runner,
    };
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!("longrun-post-{}", std::process::id()));
    fs::create_dir_all(&root).expect("root");
    let sandbox = root.join("codex");
    fs::write(
        &sandbox,
        "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
    )
    .expect("script");
    fs::set_permissions(&sandbox, fs::Permissions::from_mode(0o755)).expect("mode");
    let paths = AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        log_dir: root.join("logs"),
        jobs_dir: root.join("jobs"),
        integration_dir: root.join("integration"),
        socket_path: root.join("socket"),
    };
    paths.ensure_private_state().expect("state");
    fs::write(paths.state_dir.join("receipt.key"), [9; 32]).expect("secret");
    let cwd = NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string());
    let job = JobSpecification {
        protocol_version: 1,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string("/bin/echo".into()),
        args: vec![NativeString::from_os_string("done".into())],
        cwd: cwd.clone(),
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:test".into(),
    };
    let pending = PendingSubmission {
        session_id: "session".into(),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        cwd: cwd.clone(),
        binary_path: NativeString {
            encoding: NativeEncoding::Utf8,
            value: "/opt/longrun".into(),
        },
        expected_program: job.program.clone(),
        expected_args: job.args.clone(),
        command_hash: job.command_hash.clone(),
        hook_token_hash: "sha256:token".into(),
        created_at_ms: 1,
        expires_at_ms: i64::MAX,
        state: PendingState::Claimed,
    };
    let database = paths.state_dir.join("longrun.sqlite");
    longrun::store::Store::open(&database)
        .expect("store")
        .save_pending(&pending)
        .expect("pending");
    let now = OffsetDateTime::now_utc();
    let payload = ReceiptPayload::from_job(
        job,
        "session",
        "turn",
        "tool",
        now.format(&Rfc3339).expect("time"),
        (now + time::Duration::minutes(5))
            .format(&Rfc3339)
            .expect("expiry"),
        "nonce",
    );
    let line = ReceiptSigner::new([9; 32])
        .issue(&payload)
        .expect("receipt")
        .to_line();
    let input = PostToolUseInput {
        session_id: "session".into(),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        cwd: std::env::current_dir().expect("cwd"),
        hook_event_name: "PostToolUse".into(),
        tool_name: "Bash".into(),
        tool_input: BashInput {
            command: "ignored".into(),
        },
        tool_response: serde_json::Value::String(line),
    };
    let output = handle_post_tool_use(
        &input,
        &paths,
        &Config::default(),
        &Runner::with_sandbox_binary(sandbox),
    )
    .await
    .expect("post")
    .expect("output");
    assert!(!output.continue_processing);
    assert!(
        output
            .hook_specific_output
            .additional_context
            .contains("Job ID:")
    );
    assert!(
        handle_post_tool_use(&input, &paths, &Config::default(), &Runner::new())
            .await
            .is_err()
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn pre_tool_use_ignores_unrelated_or_wrong_binary_commands() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = Path::new("/opt/longrun");

    assert!(
        handle_pre_tool_use(&input("echo hello"), binary, &mut store, 1)
            .expect("hook")
            .is_none()
    );
    assert!(
        handle_pre_tool_use(
            &input("\"/other/longrun\" submit -- echo hello"),
            binary,
            &mut store,
            1
        )
        .expect("hook")
        .is_none()
    );
}

#[test]
fn pre_tool_use_rewrites_only_verified_submit_wrapper() {
    let mut store = Store::open_in_memory().expect("store");
    let output = handle_pre_tool_use(
        &input("\"/opt/longrun\" submit -- echo --literal"),
        Path::new("/opt/longrun"),
        &mut store,
        1,
    )
    .expect("hook")
    .expect("allow output");

    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("allow")
    );
    let command = output
        .hook_specific_output
        .updated_input
        .expect("rewrite")
        .command;
    assert!(command.contains("--hook-token"));
    assert!(command.contains("'--literal'"));
}

#[test]
fn pre_tool_use_rejects_outer_shell_composition() {
    let mut store = Store::open_in_memory().expect("store");
    let output = handle_pre_tool_use(
        &input("\"/opt/longrun\" submit -- echo ok; rm -rf /"),
        Path::new("/opt/longrun"),
        &mut store,
        1,
    )
    .expect("hook")
    .expect("deny output");

    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("deny")
    );
}

#[test]
fn pre_tool_use_accepts_explicit_submit_shell_only_with_a_script() {
    let mut store = Store::open_in_memory().expect("store");
    let output = handle_pre_tool_use(
        &input("\"/opt/longrun\" submit-shell --script 'echo ok'"),
        Path::new("/opt/longrun"),
        &mut store,
        1,
    )
    .expect("hook")
    .expect("allow output");
    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("allow")
    );
    assert!(
        handle_pre_tool_use(
            &input("\"/opt/longrun\" submit-shell"),
            Path::new("/opt/longrun"),
            &mut store,
            1
        )
        .expect("hook")
        .is_some()
    );
}
