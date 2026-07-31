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
