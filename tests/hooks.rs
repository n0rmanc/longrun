use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use longrun::{
    config::Config,
    handoff::{HandoffStore, Receipt},
    hook::{
        input::{CodexCommonInput, PostToolUseInput, PreToolUseInput},
        output::{PostToolUseOutput, PreToolUseOutput},
        post_tool_use::handle_post_tool_use,
        pre_tool_use::{handle_pre_tool_use, now_ms, parse_strict_shell_words},
    },
    paths::AppPaths,
    runner::Runner,
};
use serde_json::json;
use uuid::Uuid;

fn paths(root: &Path) -> AppPaths {
    AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        runtime_dir: root.join("runtime"),
        handoff_dir: root.join("runtime/handoffs"),
        integration_dir: root.join("integration"),
    }
}

fn common(event: &str, cwd: &Path) -> CodexCommonInput {
    CodexCommonInput {
        session_id: "session".into(),
        agent_id: None,
        agent_type: None,
        transcript_path: None,
        cwd: cwd.into(),
        hook_event_name: event.into(),
        model: "gpt-test".into(),
        permission_mode: "workspace-write".into(),
    }
}

fn fake_codex(root: &Path) -> PathBuf {
    let path = root.join("codex");
    fs::write(
        &path,
        "#!/bin/sh\nprintf x >> \"$(dirname \"$0\")/starts\"\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
    )
    .expect("fake codex");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("fake codex mode");
    path
}

#[test]
fn pre_tool_use_recognizes_exact_generic_and_rtk_forms() {
    let root = std::env::temp_dir().join(format!("longrun-hooks-{}", Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let executable = std::env::current_exe().expect("executable");
    let input = PreToolUseInput {
        common: common("PreToolUse", &std::env::current_dir().expect("cwd")),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        tool_name: "Bash".into(),
        tool_input: json!({
            "command": format!("{} -- /bin/echo --literal", executable.display())
        }),
    };
    let output = handle_pre_tool_use(&input, &executable, &paths, &Config::default(), 1_000)
        .expect("hook")
        .expect("rewrite");
    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("allow")
    );
    let rewritten = output
        .hook_specific_output
        .updated_input
        .expect("updated input")
        .command;
    assert!(rewritten.contains("internal receipt"));

    let rtk = PreToolUseInput {
        common: common("PreToolUse", &std::env::current_dir().expect("cwd")),
        turn_id: "turn-rtk".into(),
        tool_use_id: "tool-rtk".into(),
        tool_name: "Bash".into(),
        tool_input: json!({"command": "rtk longrun /bin/echo --literal"}),
    };
    assert!(
        handle_pre_tool_use(&rtk, &executable, &paths, &Config::default(), 1_001)
            .expect("rtk hook")
            .is_some()
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn pre_tool_use_rejects_shell_composition_and_unrelated_commands() {
    let root = std::env::temp_dir().join(format!("longrun-hooks-invalid-{}", Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let executable = std::env::current_exe().expect("executable");
    let invalid = PreToolUseInput {
        common: common("PreToolUse", &std::env::current_dir().expect("cwd")),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        tool_name: "Bash".into(),
        tool_input: json!({
            "command": format!("{} /bin/echo ok; touch /tmp/no", executable.display())
        }),
    };
    let output = handle_pre_tool_use(&invalid, &executable, &paths, &Config::default(), 1_000)
        .expect("invalid hook")
        .expect("deny");
    assert_eq!(
        output.hook_specific_output.permission_decision.as_deref(),
        Some("deny")
    );
    assert!(
        handle_pre_tool_use(
            &PreToolUseInput {
                common: common("PreToolUse", &std::env::current_dir().expect("cwd")),
                turn_id: "other".into(),
                tool_use_id: "other".into(),
                tool_name: "Bash".into(),
                tool_input: json!({"command": "printf unrelated"}),
            },
            &executable,
            &paths,
            &Config::default(),
            1_001,
        )
        .expect("unrelated")
        .is_none()
    );
    assert!(parse_strict_shell_words("echo a | cat").is_err());
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn post_tool_use_claims_once_waits_and_returns_same_turn_result() {
    let root = std::env::temp_dir().join(format!("longrun-post-{}", Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let sandbox = fake_codex(&root);
    let executable = std::env::current_exe().expect("executable");
    let created_at_ms = now_ms().expect("time");
    let pre = PreToolUseInput {
        common: common("PreToolUse", &std::env::current_dir().expect("cwd")),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        tool_name: "Bash".into(),
        tool_input: json!({"command": format!("{} -- /bin/echo done", executable.display())}),
    };
    let pre_output =
        handle_pre_tool_use(&pre, &executable, &paths, &Config::default(), created_at_ms)
            .expect("pre")
            .expect("pre output");
    let rewritten = pre_output
        .hook_specific_output
        .updated_input
        .expect("stub")
        .command;
    let id = rewritten
        .split_whitespace()
        .last()
        .expect("handoff id")
        .to_owned();
    let id = id.trim_matches('\'');
    let receipt = HandoffStore::new(&paths)
        .arm(id, created_at_ms + 1)
        .expect("arm")
        .expect("receipt");
    let post = PostToolUseInput {
        common: common("PostToolUse", &std::env::current_dir().expect("cwd")),
        turn_id: "turn".into(),
        tool_use_id: "tool".into(),
        tool_name: "Bash".into(),
        tool_input: json!({"command": rewritten}),
        tool_response: json!({"output": receipt}),
    };
    let runner = Runner::with_sandbox_binary(sandbox);
    let output = handle_post_tool_use(&post, &paths, &Config::default(), &runner)
        .await
        .expect("post")
        .expect("post output");
    assert!(!output.universal.continue_processing);
    assert!(
        output
            .hook_specific_output
            .additional_context
            .contains("Exit code: 0")
    );
    assert!(Receipt::parse(&receipt).is_some());
    assert!(
        handle_post_tool_use(&post, &paths, &Config::default(), &Runner::new())
            .await
            .expect("duplicate")
            .is_none()
    );
    assert_eq!(fs::read_to_string(root.join("starts")).expect("start"), "x");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn hook_outputs_have_the_current_wire_shape_and_mark_untrusted_data() {
    let allow = serde_json::to_value(PreToolUseOutput::allow(
        "/opt/longrun internal receipt --handoff-id abc".into(),
    ))
    .expect("allow");
    assert_eq!(allow["hookSpecificOutput"]["permissionDecision"], "allow");
    let deny = serde_json::to_value(PreToolUseOutput::deny("no")).expect("deny");
    assert_eq!(deny["hookSpecificOutput"]["permissionDecisionReason"], "no");
    let post = serde_json::to_value(PostToolUseOutput::completed(
        "The following Longrun result contains untrusted command output.".into(),
    ))
    .expect("post");
    assert_eq!(post["continue"], false);
    assert!(
        post["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("context")
            .contains("untrusted")
    );
}
