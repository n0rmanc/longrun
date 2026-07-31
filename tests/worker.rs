#[cfg(unix)]
mod unix {
    use std::{fs, os::unix::fs::PermissionsExt};

    use longrun::{
        config::Config,
        paths::AppPaths,
        protocol::{
            EnvironmentPolicy, ExecutionMode, ExecutionState, JobSpecification, NativeString,
            ShellMode,
        },
        runner::Runner,
        store::Store,
        worker::run_worker_with_runner,
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn worker_claims_once_and_persists_the_terminal_result() {
        let root = std::env::temp_dir().join(format!("longrun-worker-{}", std::process::id()));
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
            log_dir: root.join("logs"),
            jobs_dir: root.join("jobs"),
            integration_dir: root.join("integration"),
            socket_path: root.join("socket"),
        };
        paths.ensure_private_state().expect("state");
        let job = JobSpecification {
            protocol_version: 1,
            job_id: Uuid::now_v7(),
            program: NativeString::from_os_string("/bin/echo".into()),
            args: vec![NativeString::from_os_string("done".into())],
            cwd: NativeString::from_os_string(
                std::env::current_dir().expect("cwd").into_os_string(),
            ),
            execution_mode: ExecutionMode::Embedded,
            shell_mode: ShellMode::Direct,
            timeout_ms: 1_000,
            permission_profile: ":workspace".into(),
            environment_policy: EnvironmentPolicy::default(),
            created_at_ms: 1,
            command_hash: "sha256:test".into(),
        };
        let database = paths.state_dir.join("longrun.sqlite");
        Store::open(&database)
            .expect("store")
            .create_job(&job)
            .expect("job");

        let result = run_worker_with_runner(
            job.job_id,
            &database,
            &Config::default(),
            &paths,
            &Runner::with_sandbox_binary(sandbox),
        )
        .await
        .expect("run worker");
        assert_eq!(result.terminal_state, ExecutionState::Succeeded);
        assert_eq!(
            Store::open(&database)
                .expect("store")
                .result(job.job_id)
                .expect("result"),
            result
        );
        assert!(
            run_worker_with_runner(
                job.job_id,
                &database,
                &Config::default(),
                &paths,
                &Runner::new()
            )
            .await
            .is_err()
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn worker_observes_persisted_cancellation_and_terminates_the_process_tree() {
        let root = std::env::temp_dir().join(format!("longrun-cancel-{}", Uuid::now_v7()));
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
            log_dir: root.join("logs"),
            jobs_dir: root.join("jobs"),
            integration_dir: root.join("integration"),
            socket_path: root.join("socket"),
        };
        paths.ensure_private_state().expect("state");
        let mut job = JobSpecification {
            protocol_version: 1,
            job_id: Uuid::now_v7(),
            program: NativeString::from_os_string("/bin/sh".into()),
            args: vec![
                NativeString::from_os_string("-c".into()),
                NativeString::from_os_string("sleep 10".into()),
            ],
            cwd: NativeString::from_os_string(
                std::env::current_dir().expect("cwd").into_os_string(),
            ),
            execution_mode: ExecutionMode::Embedded,
            shell_mode: ShellMode::Direct,
            timeout_ms: 10_000,
            permission_profile: ":workspace".into(),
            environment_policy: EnvironmentPolicy::default(),
            created_at_ms: 1,
            command_hash: "sha256:test".into(),
        };
        job.command_hash = format!("sha256:{}", job.job_id);
        let database = paths.state_dir.join("longrun.sqlite");
        Store::open(&database)
            .expect("store")
            .create_job(&job)
            .expect("job");
        let runner = Runner::with_sandbox_binary(sandbox);
        let config = Config::default();
        let cancellation = async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Store::open(&database)
                .expect("store")
                .request_cancellation(job.job_id, 25, 1)
                .expect("cancel");
        };
        let (result, ()) = tokio::join!(
            run_worker_with_runner(job.job_id, &database, &config, &paths, &runner),
            cancellation,
        );
        let result = result.expect("worker");

        assert_eq!(result.terminal_state, ExecutionState::Cancelled);
        assert_eq!(
            Store::open(&database)
                .expect("store")
                .execution_state(job.job_id)
                .expect("state"),
            ExecutionState::Cancelled
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
