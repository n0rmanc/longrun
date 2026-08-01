use std::path::{Path, PathBuf};

use longrun::{
    config::Config,
    hook::{
        input::{CodexCommonInput, PreToolUseInput},
        output::PreToolUseOutput,
        pre_tool_use::{
            handle_pre_tool_use as handle_pre_tool_use_with_receipt, parse_strict_shell_words,
        },
    },
    protocol::PendingState,
    receipt::{ReceiptExpectation, ReceiptSigner},
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

fn test_binary() -> PathBuf {
    std::env::temp_dir()
        .join("longrun-hook-tests")
        .join(if cfg!(windows) {
            "longrun.exe"
        } else {
            "longrun"
        })
}

fn command(binary: &Path, arguments: &str) -> String {
    format!("\"{}\" {arguments}", binary.display())
}

fn handle_pre_tool_use(
    input: &PreToolUseInput,
    binary: &Path,
    store: &mut Store,
    now_ms: i64,
) -> longrun::error::Result<Option<PreToolUseOutput>> {
    handle_pre_tool_use_with_receipt(
        input,
        binary,
        store,
        &ReceiptSigner::new([7; 32]),
        &Config::default(),
        now_ms,
    )
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
            NativeString, PendingState, PendingSubmission, ShellMode, sha256_hex,
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
    let now = OffsetDateTime::now_utc();
    let token = "token";
    let signed_receipt = ReceiptSigner::new([9; 32])
        .issue(&ReceiptPayload::from_job(
            job.clone(),
            "session",
            "turn",
            "tool",
            now.format(&Rfc3339).expect("time"),
            (now + time::Duration::minutes(5))
                .format(&Rfc3339)
                .expect("expiry"),
            "nonce",
        ))
        .expect("receipt")
        .to_line();
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
        hook_token_hash: sha256_hex(token.as_bytes()),
        signed_receipt: Some(signed_receipt),
        created_at_ms: 1,
        expires_at_ms: i64::MAX,
        state: PendingState::Claimed,
    };
    let database = paths.state_dir.join("longrun.sqlite");
    longrun::store::Store::open(&database)
        .expect("store")
        .save_pending(&pending)
        .expect("pending");
    let input = PostToolUseInput {
        common: common("PostToolUse"),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({ "command": "ignored" }),
        tool_response: serde_json::json!({ "output": "LONGRUN_RECEIPT_HANDLE_V1 token" }),
    };
    let output = handle_post_tool_use(
        &input,
        &paths,
        &Config::default(),
        &Runner::with_sandbox_binary(sandbox.clone()),
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

    let blocked_job = JobSpecification {
        protocol_version: 1,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string("/bin/echo".into()),
        args: vec![NativeString::from_os_string("blocked".into())],
        cwd: cwd.clone(),
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":danger-full-access".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:blocked".into(),
    };
    let blocked_job_id = blocked_job.job_id;
    let blocked_token = "blocked-token";
    let blocked_signed_receipt = ReceiptSigner::new([9; 32])
        .issue(&ReceiptPayload::from_job(
            blocked_job.clone(),
            "session",
            "turn",
            "blocked-tool",
            now.format(&Rfc3339).expect("time"),
            (now + time::Duration::minutes(5))
                .format(&Rfc3339)
                .expect("expiry"),
            "blocked-nonce",
        ))
        .expect("blocked receipt")
        .to_line();
    longrun::store::Store::open(&database)
        .expect("reopen store")
        .save_pending(&PendingSubmission {
            session_id: "session".into(),
            turn_id: "turn".into(),
            tool_use_id: "blocked-tool".into(),
            cwd: cwd.clone(),
            binary_path: NativeString {
                encoding: NativeEncoding::Utf8,
                value: "/opt/longrun".into(),
            },
            expected_program: blocked_job.program.clone(),
            expected_args: blocked_job.args.clone(),
            command_hash: blocked_job.command_hash.clone(),
            hook_token_hash: sha256_hex(blocked_token.as_bytes()),
            signed_receipt: Some(blocked_signed_receipt),
            created_at_ms: 1,
            expires_at_ms: i64::MAX,
            state: PendingState::Claimed,
        })
        .expect("blocked pending");
    let blocked_input = PostToolUseInput {
        common: common("PostToolUse"),
        turn_id: "turn".into(),
        tool_use_id: "blocked-tool".into(),
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({ "command": "ignored" }),
        tool_response: serde_json::json!({
            "output": "LONGRUN_RECEIPT_HANDLE_V1 blocked-token"
        }),
    };
    let error = handle_post_tool_use(
        &blocked_input,
        &paths,
        &Config::default(),
        &Runner::with_sandbox_binary(sandbox),
    )
    .await
    .expect_err("deny disallowed profile before consuming receipt");
    assert!(
        error
            .to_string()
            .contains("danger-full-access requires explicit configuration")
    );
    let store = longrun::store::Store::open(&database).expect("reopen store");
    assert_eq!(
        store
            .pending("blocked-tool")
            .expect("pending remains unconsumed")
            .state,
        PendingState::Claimed
    );
    assert!(store.status(blocked_job_id).is_err());
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
    let binary = test_binary();
    let other_binary = binary.with_file_name("other-longrun");

    assert!(
        handle_pre_tool_use(&input("echo hello"), &binary, &mut store, 1)
            .expect("hook")
            .is_none()
    );
    assert!(
        handle_pre_tool_use(
            &input(&command(&other_binary, "submit -- echo hello")),
            &binary,
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
    let binary = test_binary();
    let pre_tool_input = input(&command(&binary, "submit -- echo --literal"));
    let signer = ReceiptSigner::new([7; 32]);
    let now = longrun::hook::pre_tool_use::now_ms().expect("clock");
    let output = handle_pre_tool_use_with_receipt(
        &pre_tool_input,
        &binary,
        &mut store,
        &signer,
        &Config::default(),
        now,
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
    assert!(command.contains("--hook-receipt"));
    let words = parse_strict_shell_words(&command).expect("parse rewrite");
    assert_eq!(words[0], binary.display().to_string());
    assert_eq!(words[1], "submit");
    assert_eq!(words.last(), Some(&"longrun-hook-receipt".to_owned()));

    let pending = store
        .pending(&pre_tool_input.tool_use_id)
        .expect("pending submission");
    assert_eq!(pending.state, PendingState::Claimed);
    assert!(
        words
            .windows(2)
            .find_map(|pair| (pair[0] == "--hook-receipt").then(|| pair[1].as_str()))
            .expect("rewritten receipt")
            .starts_with("LONGRUN_RECEIPT_HANDLE_V1 ")
    );
    let receipt = signer
        .parse(
            pending
                .signed_receipt
                .as_deref()
                .expect("stored signed receipt"),
        )
        .expect("parse receipt");
    let payload = receipt
        .verify(
            &signer,
            &ReceiptExpectation {
                session_id: pre_tool_input.common.session_id.clone(),
                turn_id: pre_tool_input.turn_id.clone(),
                tool_use_id: pre_tool_input.tool_use_id.clone(),
                cwd: pending.cwd.clone(),
                command_hash: pending.command_hash.clone(),
            },
            time::OffsetDateTime::now_utc(),
        )
        .expect("verify receipt");
    assert_eq!(payload.program.value, "echo");
    assert_eq!(payload.args[0].value, "--literal");
}

#[cfg(unix)]
#[test]
fn pre_tool_use_receipt_handle_executes_with_a_large_direct_argument() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_longrun"));
    let argument = "a".repeat(100_000);
    let pre_tool_input = input(&command(
        &binary,
        &format!("submit -- /bin/echo {argument}"),
    ));
    let signer = ReceiptSigner::new([7; 32]);
    let output = handle_pre_tool_use_with_receipt(
        &pre_tool_input,
        &binary,
        &mut store,
        &signer,
        &Config::default(),
        longrun::hook::pre_tool_use::now_ms().expect("clock"),
    )
    .expect("hook")
    .expect("allow output");
    let command = output
        .hook_specific_output
        .updated_input
        .expect("rewrite")
        .command;
    assert!(command.len() < 10_000);
    let words = parse_strict_shell_words(&command).expect("parse rewrite");
    assert_eq!(words.last(), Some(&"longrun-hook-receipt".to_owned()));
    let handle = words
        .windows(2)
        .find_map(|pair| (pair[0] == "--hook-receipt").then(|| pair[1].as_str()))
        .expect("rewritten receipt");
    let run = std::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .output()
        .expect("run receipt relay");
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(run.stdout, format!("{handle}\n").as_bytes());
    let pending = store
        .pending(&pre_tool_input.tool_use_id)
        .expect("pending submission");
    let receipt = signer
        .parse(
            pending
                .signed_receipt
                .as_deref()
                .expect("stored signed receipt"),
        )
        .expect("parse receipt");
    let payload = receipt
        .verify(
            &signer,
            &ReceiptExpectation {
                session_id: pre_tool_input.common.session_id.clone(),
                turn_id: pre_tool_input.turn_id.clone(),
                tool_use_id: pre_tool_input.tool_use_id.clone(),
                cwd: pending.cwd.clone(),
                command_hash: pending.command_hash.clone(),
            },
            time::OffsetDateTime::now_utc(),
        )
        .expect("verify receipt");
    assert_eq!(payload.args[0].value, argument);
}

#[test]
fn pre_tool_use_rejects_outer_shell_composition() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();
    let output = handle_pre_tool_use(
        &input(&command(&binary, "submit -- echo ok; rm -rf /")),
        &binary,
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
fn pre_tool_use_rejects_hook_fields_with_equals() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();

    for argument in [
        "--hook-token=untrusted",
        "--hook-receipt=LONGRUN_RECEIPT_HANDLE_V1.untrusted",
    ] {
        let output = handle_pre_tool_use(
            &input(&command(
                &binary,
                &format!("submit {argument} -- /bin/echo ignored"),
            )),
            &binary,
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
}

#[test]
fn pre_tool_use_preserves_hook_named_child_arguments() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();
    let pre_tool_input = input(&command(
        &binary,
        "submit -- /bin/echo --hook-receipt=literal-child-flag",
    ));
    let signer = ReceiptSigner::new([7; 32]);
    let output = handle_pre_tool_use_with_receipt(
        &pre_tool_input,
        &binary,
        &mut store,
        &signer,
        &Config::default(),
        longrun::hook::pre_tool_use::now_ms().expect("clock"),
    )
    .expect("hook")
    .expect("allow output");

    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("allow")
    );
    let pending = store
        .pending(&pre_tool_input.tool_use_id)
        .expect("pending submission");
    let receipt = signer
        .parse(
            pending
                .signed_receipt
                .as_deref()
                .expect("stored signed receipt"),
        )
        .expect("parse receipt");
    let payload = receipt
        .verify(
            &signer,
            &ReceiptExpectation {
                session_id: pre_tool_input.common.session_id.clone(),
                turn_id: pre_tool_input.turn_id.clone(),
                tool_use_id: pre_tool_input.tool_use_id.clone(),
                cwd: pending.cwd.clone(),
                command_hash: pending.command_hash.clone(),
            },
            time::OffsetDateTime::now_utc(),
        )
        .expect("verify receipt");
    assert_eq!(payload.args[0].value, "--hook-receipt=literal-child-flag");
}

#[test]
fn pre_tool_use_accepts_quoted_program_arguments() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();
    let pre_tool_input = input(&command(
        &binary,
        r#"submit -- /bin/sh -c 'sleep 90; printf "DONE\n"'"#,
    ));
    let signer = ReceiptSigner::new([7; 32]);
    let output = handle_pre_tool_use_with_receipt(
        &pre_tool_input,
        &binary,
        &mut store,
        &signer,
        &Config::default(),
        longrun::hook::pre_tool_use::now_ms().expect("clock"),
    )
    .expect("hook")
    .expect("allow output");

    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("allow")
    );
    let pending = store
        .pending(&pre_tool_input.tool_use_id)
        .expect("pending submission");
    let receipt = signer
        .parse(
            pending
                .signed_receipt
                .as_deref()
                .expect("stored signed receipt"),
        )
        .expect("parse receipt");
    let payload = receipt
        .verify(
            &signer,
            &ReceiptExpectation {
                session_id: pre_tool_input.common.session_id.clone(),
                turn_id: pre_tool_input.turn_id.clone(),
                tool_use_id: pre_tool_input.tool_use_id.clone(),
                cwd: pending.cwd.clone(),
                command_hash: pending.command_hash.clone(),
            },
            time::OffsetDateTime::now_utc(),
        )
        .expect("verify receipt");
    assert_eq!(payload.program.value, "/bin/sh");
    assert_eq!(
        payload
            .args
            .iter()
            .map(|value| value.value.as_str())
            .collect::<Vec<_>>(),
        vec!["-c", "sleep 90; printf \"DONE\\n\""]
    );
}

#[cfg(unix)]
#[test]
fn pre_tool_use_canonicalizes_the_submitted_working_directory() {
    use std::{fs, os::unix::fs::symlink};

    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!("longrun-hook-cwd-{}", Uuid::now_v7()));
    let real = root.join("real");
    let link = root.join("link");
    fs::create_dir_all(&real).expect("real directory");
    symlink(&real, &link).expect("working-directory symlink");

    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();
    let mut pre_tool_input = input(&command(&binary, "submit -- /bin/echo ok"));
    pre_tool_input.common.cwd = link.clone();
    handle_pre_tool_use(&pre_tool_input, &binary, &mut store, 1)
        .expect("hook")
        .expect("allow output");

    assert_eq!(
        store
            .pending(&pre_tool_input.tool_use_id)
            .expect("pending submission")
            .cwd
            .to_os_string()
            .expect("UTF-8 working directory"),
        fs::canonicalize(link)
            .expect("canonical working directory")
            .into_os_string()
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn pre_tool_use_accepts_explicit_submit_shell_only_with_a_script() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();
    let mut config = Config::default();
    config.execution.allow_shell = true;
    let output = handle_pre_tool_use_with_receipt(
        &input(&command(&binary, "submit-shell --script 'echo ok'")),
        &binary,
        &mut store,
        &ReceiptSigner::new([7; 32]),
        &config,
        1,
    )
    .expect("hook")
    .expect("allow output");
    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("allow")
    );
    assert!(
        output
            .hook_specific_output
            .updated_input
            .expect("rewrite")
            .command
            .contains("'submit'")
    );
    assert!(
        handle_pre_tool_use(
            &input(&command(&binary, "submit-shell")),
            &binary,
            &mut store,
            1
        )
        .expect("hook")
        .is_some()
    );
}

#[test]
fn pre_tool_use_requires_config_for_submit_shell() {
    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();
    let output = handle_pre_tool_use(
        &input(&command(&binary, "submit-shell --script 'echo denied'")),
        &binary,
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
fn pre_tool_use_rejects_per_submission_config() {
    use std::fs;

    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!("longrun-hook-config-{}", Uuid::now_v7()));
    fs::create_dir_all(&root).expect("config root");
    let mut store = Store::open_in_memory().expect("store");
    let binary = test_binary();
    let mut pre_tool_input = input(&command(
        &binary,
        "submit-shell --config longrun.toml --script 'printf configured'",
    ));
    pre_tool_input.common.cwd = root.clone();
    let output = handle_pre_tool_use_with_receipt(
        &pre_tool_input,
        &binary,
        &mut store,
        &ReceiptSigner::new([7; 32]),
        &Config::default(),
        longrun::hook::pre_tool_use::now_ms().expect("clock"),
    )
    .expect("hook")
    .expect("deny output");
    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("deny")
    );
    assert!(
        output
            .hook_specific_output
            .permission_decision_reason
            .as_deref()
            .expect("reason")
            .contains("--config is not supported by Codex hooks")
    );
    assert!(store.pending(&pre_tool_input.tool_use_id).is_err());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn strict_shell_parser_preserves_windows_path_separators_inside_quotes() {
    assert_eq!(
        parse_strict_shell_words(r#""C:\Longrun\longrun.exe" submit -- echo ok"#)
            .expect("parse Windows path"),
        vec![
            r"C:\Longrun\longrun.exe".to_owned(),
            "submit".to_owned(),
            "--".to_owned(),
            "echo".to_owned(),
            "ok".to_owned(),
        ]
    );
}
