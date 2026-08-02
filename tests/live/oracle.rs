#![cfg(unix)]

#[path = "hook_support.rs"]
mod hook_support;

use std::env;

#[test]
#[ignore = "release-gated live Codex hook test"]
fn codex_hook_runs_one_oracle_browser_review() {
    if env::var_os("LONGRUN_ORACLE_LIVE").is_none() {
        return;
    }
    let profile = env::var("ORACLE_BROWSER_PROFILE")
        .unwrap_or_else(|_| "/Users/norman/.oracle/browser-profile".into());
    let root = hook_support::test_root("oracle-success");
    let command = format!(
        "{} --permission-profile :danger-full-access -- oracle --engine browser --browser-manual-login --browser-manual-login-profile-dir {} --browser-keep-browser --model gpt-5-pro -p 'Reply exactly LONGRUN_ORACLE_HOOK_OK'",
        hook_support::shell_quote(env!("CARGO_BIN_EXE_longrun")),
        hook_support::shell_quote(&profile)
    );
    let output = hook_support::run_hooked_target(
        &root,
        &command,
        &[],
        "oracle-live-session",
        "oracle-live-turn",
        "oracle-live-tool",
    );
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("context");
    assert_eq!(output["continue"], false);
    assert!(context.contains("Terminal reason: Exited"));
    hook_support::cleanup(&root);
}

#[test]
#[ignore = "release-gated live Oracle failure test"]
fn codex_hook_returns_oracle_failure_without_reattachment() {
    if env::var_os("LONGRUN_ORACLE_LIVE_FAILURE").is_none() {
        return;
    }
    let profile = env::var("ORACLE_BROWSER_PROFILE")
        .unwrap_or_else(|_| "/Users/norman/.oracle/browser-profile".into());
    let root = hook_support::test_root("oracle-failure");
    let command = format!(
        "{} --permission-profile :danger-full-access -- oracle --engine browser --browser-manual-login --browser-manual-login-profile-dir {} --model gpt-5-pro --file /definitely/missing/longrun-file",
        hook_support::shell_quote(env!("CARGO_BIN_EXE_longrun")),
        hook_support::shell_quote(&profile)
    );
    let output = hook_support::run_hooked_target(
        &root,
        &command,
        &[],
        "oracle-failure-session",
        "oracle-failure-turn",
        "oracle-failure-tool",
    );
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("context");
    assert_eq!(output["continue"], false);
    assert!(context.contains("Exit code:"));
    hook_support::cleanup(&root);
}
