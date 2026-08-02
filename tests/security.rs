#[cfg(unix)]
mod unix {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use base64::Engine;
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
    fn hook_rejects_danger_profile_without_explicit_configuration() {
        let root =
            std::env::temp_dir().join(format!("longrun-security-profile-{}", Uuid::now_v7()));
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let input = input(
            "/opt/longrun --permission-profile :danger-full-access -- /bin/echo denied",
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
        .expect("deny output");
        assert_eq!(
            output.hook_specific_output.permission_decision.as_deref(),
            Some("deny")
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn environment_policy_hides_protected_values_by_default() {
        let config = Config::default();
        assert!(config.environment.is_protected("GITHUB_TOKEN"));
        assert!(config.environment.is_protected("db_password"));
        assert!(!config.environment.allows("GITHUB_TOKEN"));
        assert!(!config.environment.allows("PATH"));
    }

    #[tokio::test]
    #[ignore = "requires a configured real Codex :workspace sandbox profile"]
    async fn live_workspace_profile_denies_outside_write_and_network() {
        let root = std::env::temp_dir().join(format!("longrun-security-live-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let marker = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"))
            .join(format!(".longrun-live-outside-{}", Uuid::now_v7()));
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let script = format!(
            "if touch '{}'; then printf 'write=allowed\\n'; else printf 'write=denied\\n'; fi; \
             if /usr/bin/curl -fsS --max-time 2 https://example.com >/dev/null 2>&1; \
             then printf 'network=allowed\\n'; else printf 'network=denied\\n'; fi",
            marker.display()
        );
        let target = TargetSpec {
            protocol_version: 2,
            program: NativeString::from_os_string("/bin/sh".into()),
            args: vec![
                NativeString::from_os_string("-c".into()),
                NativeString::from_os_string(script.into()),
            ],
            cwd: NativeString::from_os_string(
                std::env::current_dir().expect("cwd").into_os_string(),
            ),
            timeout_ms: 10_000,
            permission_profile: ":workspace".into(),
            environment_policy: EnvironmentPolicy::default(),
            created_at_ms: 1,
            command_hash: "sha256:security-live".into(),
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
            .expect("sandbox result");
        let stdout = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(result.stdout.tail_base64url)
            .expect("stdout");
        let stdout = String::from_utf8(stdout).expect("UTF-8");
        assert!(stdout.contains("write=denied"), "{stdout}");
        assert!(stdout.contains("network=denied"), "{stdout}");
        if marker.exists() {
            fs::remove_file(&marker).expect("remove unexpected marker");
        }
        assert!(!marker.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
