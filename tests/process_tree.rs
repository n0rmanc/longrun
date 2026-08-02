#[cfg(unix)]
#[tokio::test]
async fn timeout_kills_the_owned_process_group_and_descendant() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };

    use base64::Engine;
    use longrun::{
        config::Config,
        paths::AppPaths,
        protocol::{EnvironmentPolicy, NativeString, TargetSpec, TerminalReason},
        runner::{ExecutionMode, OutputMode, Runner},
    };
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!("longrun-tree-{}", Uuid::now_v7()));
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
        runtime_dir: root.join("runtime"),
        handoff_dir: root.join("runtime/handoffs"),
        integration_dir: root.join("integration"),
    };
    paths.ensure_private_state().expect("state");
    let target = TargetSpec {
        protocol_version: 2,
        program: NativeString::from_os_string("/bin/sh".into()),
        args: vec![
            NativeString::from_os_string("-c".into()),
            NativeString::from_os_string("sleep 10 & echo $!; wait".into()),
        ],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:test".into(),
    };
    let mut config = Config::default();
    config.execution.termination_grace_ms = 25;
    config.execution.post_tool_use_timeout_ms = 10_000;
    let result = Runner::with_sandbox_binary(sandbox)
        .execute(
            &target,
            &config,
            &paths,
            ExecutionMode::CodexHook,
            OutputMode::Capture,
        )
        .await
        .expect("timeout");
    assert_eq!(result.terminal_reason, TerminalReason::TimedOut);
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(result.stdout.tail_base64url)
        .expect("stdout");
    let pid = String::from_utf8(bytes).expect("pid").trim().to_owned();
    assert!(!pid.is_empty(), "descendant PID must be recorded");
    assert!(
        !Command::new("kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .expect("kill")
            .success()
    );
    fs::remove_dir_all(root).expect("cleanup");
}
