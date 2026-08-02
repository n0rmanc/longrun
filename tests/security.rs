#[cfg(unix)]
mod unix {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use longrun::{
        config::Config,
        hook::{
            input::{CodexCommonInput, PreToolUseInput},
            pre_tool_use::handle_pre_tool_use,
        },
        paths::AppPaths,
        protocol::{EnvironmentPolicy, NativeString, TargetSpec, TerminalReason},
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
    async fn hook_runner_fails_closed_without_falling_back_to_direct_execution() {
        let root =
            std::env::temp_dir().join(format!("longrun-security-sandbox-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let sandbox = root.join("codex");
        fs::write(
            &sandbox,
            "#!/bin/sh\nprintf 'sandbox denied\\n' >&2\nexit 42\n",
        )
        .expect("sandbox");
        fs::set_permissions(&sandbox, fs::Permissions::from_mode(0o755)).expect("mode");
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let marker = root.join("target-ran");
        let target = TargetSpec {
            protocol_version: 2,
            program: NativeString::from_os_string("/bin/sh".into()),
            args: vec![
                NativeString::from_os_string("-c".into()),
                NativeString::from_os_string(format!("touch {}", marker.display()).into()),
            ],
            cwd: NativeString::from_os_string(
                std::env::current_dir().expect("cwd").into_os_string(),
            ),
            timeout_ms: 1_000,
            permission_profile: ":workspace".into(),
            environment_policy: EnvironmentPolicy::default(),
            created_at_ms: 1,
            command_hash: "sha256:security".into(),
        };
        let result = Runner::with_sandbox_binary(sandbox)
            .execute(
                &target,
                &Config::default(),
                &paths,
                ExecutionMode::CodexHook,
                OutputMode::Capture,
            )
            .await
            .expect("sandbox result");
        assert_eq!(result.terminal_reason, TerminalReason::Exited);
        assert_eq!(result.exit_code, Some(42));
        assert!(!marker.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn danger_full_access_requires_explicit_configuration() {
        let config = Config::default();
        assert!(!config.permits_permission_profile(":danger-full-access"));
        let config =
            Config::from_toml("[execution]\nallow_danger_full_access = true").expect("config");
        assert!(config.permits_permission_profile(":danger-full-access"));
    }

    #[test]
    fn environment_policy_hides_protected_values_by_default() {
        let config = Config::default();
        assert!(config.environment.is_protected("GITHUB_TOKEN"));
        assert!(config.environment.is_protected("db_password"));
        assert!(!config.environment.allows("GITHUB_TOKEN"));
        assert!(!config.environment.allows("PATH"));
    }
}
