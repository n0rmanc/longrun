#[cfg(unix)]
mod unix {
    use std::{
        fs,
        path::Path,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use longrun::{
        config::Config,
        paths::AppPaths,
        protocol::{NativeString, TargetSpec, TerminalReason},
        runner::{ExecutionMode, OutputMode, Runner},
    };
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
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

    fn target(script: impl Into<String>, timeout_ms: u64) -> TargetSpec {
        TargetSpec {
            protocol_version: 3,
            program: NativeString::from_os_string("/bin/sh".into()),
            args: vec![
                NativeString::from_os_string("-c".into()),
                NativeString::from_os_string(std::ffi::OsString::from(script.into())),
            ],
            cwd: NativeString::from_os_string(
                std::env::current_dir().expect("cwd").into_os_string(),
            ),
            timeout_ms,
            created_at_ms: 1,
            command_hash: "sha256:test".into(),
        }
    }

    fn wait_for_pid_file(path: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(value) = fs::read_to_string(path)
                && let Ok(pid) = value.trim().parse()
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    async fn wait_for_pid_file_async(path: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(value) = fs::read_to_string(path)
                && let Ok(pid) = value.trim().parse()
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {}",
                path.display()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn pid_is_alive(pid: i32) -> bool {
        if !Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("probe pid")
            .success()
        {
            return false;
        }
        let state = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("process state");
        state.status.success()
            && !String::from_utf8_lossy(&state.stdout)
                .trim()
                .starts_with('Z')
    }

    fn wait_for_pid_gone(pid: i32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while pid_is_alive(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(!pid_is_alive(pid), "process {pid} is still alive");
    }

    fn descendant_pid(stdout: &longrun::protocol::CapturedOutput) -> i32 {
        let bytes = URL_SAFE_NO_PAD
            .decode(&stdout.tail_base64url)
            .expect("stdout");
        String::from_utf8(bytes)
            .expect("pid output")
            .trim()
            .parse()
            .expect("descendant pid")
    }

    #[tokio::test]
    async fn timeout_kills_the_owned_process_group_and_descendant() {
        let root = std::env::temp_dir().join(format!("longrun-tree-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let mut config = Config::default();
        config.execution.termination_grace_ms = 25;
        config.execution.post_tool_use_timeout_ms = 10_000;
        let result = Runner::new()
            .execute(
                &target("sleep 10 & echo $!; wait", 1_000),
                &config,
                &paths,
                ExecutionMode::CodexHook,
                OutputMode::Capture,
            )
            .await
            .expect("timeout");
        assert_eq!(result.terminal_reason, TerminalReason::TimedOut);
        wait_for_pid_gone(descendant_pid(&result.stdout));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn dropping_the_runner_future_kills_the_owned_child() {
        let root = std::env::temp_dir().join(format!("longrun-drop-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let pid_file = root.join("child.pid");
        let target = target(
            format!("echo $$ > {}; sleep 30", pid_file.display()),
            60_000,
        );
        let config = Config::default();
        let task = tokio::spawn(async move {
            Runner::new()
                .execute(
                    &target,
                    &config,
                    &paths,
                    ExecutionMode::Direct,
                    OutputMode::Capture,
                )
                .await
        });
        let pid = wait_for_pid_file_async(&pid_file).await;
        task.abort();
        assert!(
            task.await.is_err(),
            "aborted task should not complete normally"
        );
        wait_for_pid_gone(pid);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn leader_exit_cleans_descendants_from_the_owned_process_group() {
        let root = std::env::temp_dir().join(format!("longrun-leader-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let paths = paths(&root);
        paths.ensure_private_state().expect("state");
        let result = Runner::new()
            .execute(
                &target("sleep 30 & echo $!; exit 0", 5_000),
                &Config::default(),
                &paths,
                ExecutionMode::Direct,
                OutputMode::Capture,
            )
            .await
            .expect("leader exit");
        assert_eq!(result.terminal_reason, TerminalReason::Exited);
        assert_eq!(result.exit_code, Some(0));
        wait_for_pid_gone(descendant_pid(&result.stdout));
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn owner_signal_kills_the_active_target_tree(signal: Signal) {
        let root = std::env::temp_dir().join(format!("longrun-owner-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let pid_file = root.join("owner-child.pid");
        let script = format!("echo $$ > {}; sleep 30", pid_file.display());
        let mut process = Command::new(env!("CARGO_BIN_EXE_longrun"))
            .args(["--timeout", "30s", "--", "/bin/sh", "-c", &script])
            .env("HOME", root.join("home"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("longrun");
        let pid = wait_for_pid_file(&pid_file);
        kill(Pid::from_raw(process.id() as i32), signal).expect("owner signal");
        let status = process.wait().expect("wait longrun");
        assert_eq!(status.code(), Some(130));
        wait_for_pid_gone(pid);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn owner_shutdown_kills_the_active_target_tree() {
        owner_signal_kills_the_active_target_tree(Signal::SIGTERM);
    }

    #[test]
    fn interrupt_and_hangup_kill_the_active_target_tree() {
        owner_signal_kills_the_active_target_tree(Signal::SIGINT);
        owner_signal_kills_the_active_target_tree(Signal::SIGHUP);
    }
}
