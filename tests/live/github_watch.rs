#![cfg(unix)]

#[path = "hook_support.rs"]
mod hook_support;

use std::env;

#[test]
#[ignore = "release-gated live Codex hook test"]
fn codex_hook_waits_for_a_github_actions_run_once() {
    let Some(run_id) = env::var_os("LONGRUN_GITHUB_RUN_ID") else {
        return;
    };
    let Some(token) = env::var_os("GH_TOKEN") else {
        return;
    };
    let repo = env::var("LONGRUN_GITHUB_REPO").unwrap_or_else(|_| "n0rmanc/longrun".into());
    let root = hook_support::test_root("github-success");
    let command = format!(
        "{} --env-pass GH_TOKEN --permission-profile :danger-full-access -- gh run watch {} --repo {} --exit-status",
        hook_support::shell_quote(env!("CARGO_BIN_EXE_longrun")),
        run_id.to_string_lossy(),
        repo
    );
    let token = token.to_string_lossy().into_owned();
    let output = hook_support::run_hooked_target(
        &root,
        &command,
        &[("GH_TOKEN", &token)],
        "github-live-session",
        "github-live-turn",
        "github-live-tool",
    );
    assert_eq!(output["continue"], false);
    assert!(
        output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("context")
            .contains("Terminal reason:")
    );
    hook_support::cleanup(&root);
}

#[test]
#[ignore = "release-gated live GitHub failure/auth test"]
fn codex_hook_returns_github_failure_without_retry() {
    let Some(run_id) = env::var_os("LONGRUN_GITHUB_FAILURE_RUN_ID") else {
        return;
    };
    let Some(token) = env::var_os("GH_TOKEN") else {
        return;
    };
    let repo = env::var("LONGRUN_GITHUB_REPO").unwrap_or_else(|_| "n0rmanc/longrun".into());
    let root = hook_support::test_root("github-failure");
    let command = format!(
        "{} --env-pass GH_TOKEN --permission-profile :danger-full-access -- gh run watch {} --repo {} --exit-status",
        hook_support::shell_quote(env!("CARGO_BIN_EXE_longrun")),
        run_id.to_string_lossy(),
        repo
    );
    let token = token.to_string_lossy().into_owned();
    let output = hook_support::run_hooked_target(
        &root,
        &command,
        &[("GH_TOKEN", &token)],
        "github-failure-session",
        "github-failure-turn",
        "github-failure-tool",
    );
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("context");
    assert_eq!(output["continue"], false);
    assert!(context.contains("Exit code:"));
    let handoff_dir = if cfg!(target_os = "macos") {
        root.join(
            "home/Library/Application Support/dev.longrun.Longrun/state/runtime/longrun-handoffs",
        )
    } else {
        root.join("runtime/longrun-handoffs")
    };
    assert!(
        !handoff_dir.join("result.json").exists(),
        "ephemeral hook must not write a result file"
    );
    hook_support::cleanup(&root);
}

#[test]
#[ignore = "release-gated live GitHub auth test"]
fn codex_hook_reports_github_auth_failure_without_widening_access() {
    let Some(run_id) = env::var_os("LONGRUN_GITHUB_RUN_ID") else {
        return;
    };
    let repo = env::var("LONGRUN_GITHUB_REPO").unwrap_or_else(|_| "n0rmanc/longrun".into());
    let root = hook_support::test_root("github-auth");
    let script = format!(
        "GH_TOKEN=invalid gh run watch {} --repo {} --exit-status",
        run_id.to_string_lossy(),
        repo
    );
    let command = format!(
        "{} --permission-profile :danger-full-access -- /bin/sh -c {}",
        hook_support::shell_quote(env!("CARGO_BIN_EXE_longrun")),
        hook_support::shell_quote(&script)
    );
    let output = hook_support::run_hooked_target(
        &root,
        &command,
        &[],
        "github-auth-session",
        "github-auth-turn",
        "github-auth-tool",
    );
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("context");
    assert_eq!(output["continue"], false);
    assert!(context.contains("Exit code:"));
    hook_support::cleanup(&root);
}
