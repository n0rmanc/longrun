#[cfg(unix)]
mod unix {
    use std::{fs, path::Path};

    use longrun::{
        config::Config,
        hook::{
            input::{CodexCommonInput, PreToolUseInput},
            pre_tool_use::handle_pre_tool_use,
        },
        paths::AppPaths,
        protocol::{NativeString, TargetSpec, TerminalReason},
        runner::{ExecutionMode, OutputMode, Runner},
    };
    use uuid::Uuid;

    fn paths(root: &std::path::Path) -> AppPaths {
        AppPaths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            state_dir: root.join("state"),
            runtime_dir: root.join("runtime"),
            handoff_dir: root.join("runtime/handoffs"),
            integration_dir: root.join("integration"),
        }
    }

    fn input(command: &str, cwd: &Path) -> PreToolUseInput {
        PreToolUseInput {
            common: CodexCommonInput {
                session_id: "session".into(),
                agent_id: None,
                agent_type: None,
                transcript_path: None,
                cwd: cwd.into(),
                hook_event_name: "PreToolUse".into(),
                model: "gpt-test".into(),
                permission_mode: "default".into(),
            },
            turn_id: "turn".into(),
            tool_use_id: "tool".into(),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({"command": command}),
        }
    }

    #[test]
    fn hook_requires_the_absolute_installed_binary_and_rejects_shadowing() {
        let root = std::env::temp_dir().join(format!("longrun-security-{}", Uuid::now_v7()));
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let expected = Path::new("/opt/longrun");
        assert!(
            handle_pre_tool_use(
                &input(
                    "longrun /bin/echo shadowed",
                    &std::env::current_dir().expect("cwd")
                ),
                expected,
                &paths,
                &Config::default(),
                1,
            )
            .expect("hook")
            .is_none()
        );
        assert!(
            handle_pre_tool_use(
                &input(
                    "\"/tmp/longrun\" /bin/echo shadowed",
                    &std::env::current_dir().expect("cwd"),
                ),
                expected,
                &paths,
                &Config::default(),
                1,
            )
            .expect("hook")
            .is_none()
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn hook_runner_executes_the_target_without_a_sandbox() {
        let root = std::env::temp_dir().join(format!("longrun-security-direct-{}", Uuid::now_v7()));
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let marker = root.join("target-ran");
        let target = TargetSpec {
            protocol_version: 3,
            program: NativeString::from_os_string("/bin/sh".into()),
            args: vec![
                NativeString::from_os_string("-c".into()),
                NativeString::from_os_string(format!("touch {}", marker.display()).into()),
            ],
            cwd: NativeString::from_os_string(
                std::env::current_dir().expect("cwd").into_os_string(),
            ),
            timeout_ms: 1_000,
            created_at_ms: 1,
            command_hash: "sha256:security".into(),
        };
        let result = Runner::new()
            .execute(
                &target,
                &Config::default(),
                &paths,
                ExecutionMode::CodexHook,
                OutputMode::Capture,
            )
            .await
            .expect("direct result");
        assert_eq!(result.terminal_reason, TerminalReason::Exited);
        assert_eq!(result.exit_code, Some(0));
        assert!(marker.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn hook_rewrites_an_explicit_command() {
        let root =
            std::env::temp_dir().join(format!("longrun-security-profile-{}", Uuid::now_v7()));
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let input = input(
            "/opt/longrun -- /bin/echo allowed",
            &std::env::current_dir().expect("cwd"),
        );
        let output = handle_pre_tool_use(
            &input,
            Path::new("/opt/longrun"),
            &paths,
            &Config::default(),
            1,
        )
        .expect("hook")
        .expect("allow output");
        assert_eq!(
            output.hook_specific_output.permission_decision.as_deref(),
            Some("allow")
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
