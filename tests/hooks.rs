use std::path::Path;

use longrun::{
    hook::{
        input::{CodexCommonInput, PreToolUseInput},
        pre_tool_use::handle_pre_tool_use,
    },
    store::Store,
};

fn input(command: &str) -> PreToolUseInput {
    PreToolUseInput {
        common: common("PreToolUse"),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({ "command": command }),
    }
}

fn common(hook_event_name: &str) -> CodexCommonInput {
    CodexCommonInput {
        session_id: "session".into(),
        agent_id: None,
        agent_type: None,
        transcript_path: None,
        cwd: std::env::current_dir().expect("cwd"),
        hook_event_name: hook_event_name.into(),
        model: "gpt-test".into(),
        permission_mode: "default".into(),
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
            DeliveryState, EnvironmentPolicy, ExecutionMode, JobSpecification, NativeEncoding,
            NativeString, PendingState, PendingSubmission, ShellMode,
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
    let job_id = job.job_id;
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
        common: common("PostToolUse"),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({ "command": "ignored" }),
        tool_response: serde_json::json!({ "output": line }),
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
    assert!(!output.universal.continue_processing);
    assert!(
        output
            .hook_specific_output
            .additional_context
            .contains("Job ID:")
    );
    assert_eq!(
        longrun::store::Store::open(&database)
            .expect("reopen store")
            .status(job_id)
            .expect("status")
            .delivery_state,
        DeliveryState::DeliveredInTurn
    );
    assert!(
        handle_post_tool_use(&input, &paths, &Config::default(), &Runner::new())
            .await
            .is_err()
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn codex_hook_inputs_accept_current_wire_fields_and_ignore_unknown_ones() {
    use longrun::hook::input::{PostToolUseInput, SessionStartInput};

    let pre: PreToolUseInput = serde_json::from_value(serde_json::json!({
        "session_id": "session",
        "turn_id": "turn",
        "agent_id": "agent",
        "agent_type": "worker",
        "transcript_path": null,
        "cwd": "/tmp",
        "hook_event_name": "PreToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {"command": "echo ok"},
        "tool_use_id": "tool",
        "future_field": true
    }))
    .expect("pre input");
    assert_eq!(pre.common.agent_id.as_deref(), Some("agent"));
    assert_eq!(pre.bash_command(), Some("echo ok"));

    let post: PostToolUseInput = serde_json::from_value(serde_json::json!({
        "session_id": "session",
        "turn_id": "turn",
        "transcript_path": "/tmp/transcript.jsonl",
        "cwd": "/tmp",
        "hook_event_name": "PostToolUse",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "tool_name": "Bash",
        "tool_input": {"command": "echo ok"},
        "tool_response": {"output": "LONGRUN_RECEIPT_V1 ignored"},
        "tool_use_id": "tool"
    }))
    .expect("post input");
    assert_eq!(
        post.common.transcript_path.as_deref(),
        Some(std::path::Path::new("/tmp/transcript.jsonl"))
    );
    assert!(post.tool_response.is_object());

    let session: SessionStartInput = serde_json::from_value(serde_json::json!({
        "session_id": "session",
        "transcript_path": null,
        "cwd": "/tmp",
        "hook_event_name": "SessionStart",
        "model": "gpt-test",
        "permission_mode": "workspace-write",
        "source": "resume"
    }))
    .expect("session input");
    assert_eq!(session.source, "resume");
    assert!(serde_json::from_value::<PreToolUseInput>(serde_json::json!({})).is_err());
}

#[test]
fn codex_hook_outputs_use_current_wire_shape() {
    use longrun::hook::output::{PostToolUseOutput, PreToolUseOutput, SessionStartOutput};

    assert_eq!(
        serde_json::to_value(PreToolUseOutput::allow("longrun submit".into()))
            .expect("allow output"),
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": {"command": "longrun submit"}
            }
        })
    );
    assert_eq!(
        serde_json::to_value(PreToolUseOutput::deny("no")).expect("deny output"),
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": "no"
            }
        })
    );
    assert_eq!(
        serde_json::to_value(PostToolUseOutput::completed("result".into())).expect("post output"),
        serde_json::json!({
            "continue": false,
            "systemMessage": "Longrun completed the submitted command.",
            "hookSpecificOutput": {
                "hookEventName": "PostToolUse",
                "additionalContext": "result"
            }
        })
    );
    assert_eq!(
        serde_json::to_value(SessionStartOutput::context("recovered".into()))
            .expect("session output"),
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": "recovered"
            }
        })
    );
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
