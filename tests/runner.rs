#[cfg(unix)]
mod unix {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    use longrun::{
        config::Config,
        paths::AppPaths,
        protocol::{EnvironmentPolicy, ExecutionMode, JobSpecification, NativeString, ShellMode},
        runner::Runner,
        store::Store,
        worker::run_worker_with_runner,
    };
    use uuid::Uuid;

    fn fake_codex(root: &std::path::Path) -> PathBuf {
        let path = root.join("codex");
        fs::write(
            &path,
            "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
        )
        .expect("write fake sandbox");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make executable");
        path
    }

    fn specification() -> JobSpecification {
        JobSpecification {
            protocol_version: 1,
            job_id: Uuid::now_v7(),
            program: NativeString::from_os_string("/bin/sh".into()),
            args: vec![
                NativeString::from_os_string("-c".into()),
                NativeString::from_os_string("printf out; printf err >&2; exit 7".into()),
            ],
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
        }
    }

    #[tokio::test]
    async fn runner_persists_separate_logs_and_child_exit_status() {
        let root = std::env::temp_dir().join(format!("longrun-runner-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let paths = AppPaths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            state_dir: root.join("state"),
            log_dir: root.join("logs"),
            jobs_dir: root.join("jobs"),
            integration_dir: root.join("integration"),
            socket_path: root.join("longrun.sock"),
        };
        paths.ensure_private_state().expect("state");
        let result = Runner::with_sandbox_binary(fake_codex(&root))
            .execute(&specification(), &Config::default(), &paths)
            .await
            .expect("run");

        assert_eq!(result.exit_code, Some(7));
        assert_eq!(
            fs::read_to_string(
                root.join("logs")
                    .join(format!("{}.stdout.log", result.job_id))
            )
            .expect("stdout"),
            "out"
        );
        assert_eq!(
            fs::read_to_string(
                root.join("logs")
                    .join(format!("{}.stderr.log", result.job_id))
            )
            .expect("stderr"),
            "err"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn worker_persists_runner_result_before_returning() {
        let root = std::env::temp_dir().join(format!("longrun-runner-store-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let paths = AppPaths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            state_dir: root.join("state"),
            log_dir: root.join("logs"),
            jobs_dir: root.join("jobs"),
            integration_dir: root.join("integration"),
            socket_path: root.join("longrun.sock"),
        };
        paths.ensure_private_state().expect("state");
        let job = specification();
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
            &Runner::with_sandbox_binary(fake_codex(&root)),
        )
        .await
        .expect("worker");
        assert_eq!(
            Store::open(&database)
                .expect("store")
                .result(job.job_id)
                .expect("result"),
            result
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
