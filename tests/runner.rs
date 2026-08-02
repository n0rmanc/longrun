#[cfg(unix)]
mod unix {
    use std::fs;

    use base64::Engine;
    use longrun::{
        config::Config,
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

    fn target(script: &str) -> TargetSpec {
        TargetSpec {
            protocol_version: 3,
            program: NativeString::from_os_string("/bin/sh".into()),
            args: vec![
                NativeString::from_os_string("-c".into()),
                NativeString::from_os_string(script.into()),
            ],
            cwd: NativeString::from_os_string(
                std::env::current_dir().expect("cwd").into_os_string(),
            ),
            timeout_ms: 1_000,
            created_at_ms: 1,
            command_hash: "sha256:test".into(),
        }
    }

    #[tokio::test]
    async fn hook_runner_executes_directly_and_keeps_bounded_output_in_memory() {
        let root = std::env::temp_dir().join(format!("longrun-runner-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let result = Runner::new()
            .execute(
                &target("printf out; printf err >&2; exit 7"),
                &Config::default(),
                &paths,
                ExecutionMode::CodexHook,
                OutputMode::Capture,
            )
            .await
            .expect("run");

        assert_eq!(result.terminal_reason, TerminalReason::Exited);
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(result.stdout.tail_base64url)
                .expect("stdout"),
            b"out"
        );
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(result.stderr.tail_base64url)
                .expect("stderr"),
            b"err"
        );
        assert!(
            fs::read_dir(&paths.handoff_dir)
                .expect("handoff dir")
                .next()
                .is_none()
        );
        assert!(!paths.state_dir.join("longrun.sqlite").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn direct_runner_returns_target_status_without_requiring_codex() {
        let root = std::env::temp_dir().join(format!("longrun-runner-direct-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let result = Runner::new()
            .execute(
                &target("printf direct; exit 3"),
                &Config::default(),
                &paths,
                ExecutionMode::Direct,
                OutputMode::Capture,
            )
            .await
            .expect("direct run");
        assert_eq!(result.exit_code, Some(3));
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(result.stdout.tail_base64url)
                .expect("stdout"),
            b"direct"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
