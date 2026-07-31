#[cfg(unix)]
#[tokio::test]
async fn timeout_kills_the_owned_process_group() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };

    use longrun::{
        config::Config,
        paths::AppPaths,
        protocol::{
            EnvironmentPolicy, ExecutionMode, ExecutionState, JobSpecification, NativeString,
            ShellMode,
        },
        runner::Runner,
    };
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!("longrun-tree-{}", std::process::id()));
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
        program: NativeString::from_os_string("/bin/sh".into()),
        args: vec![
            NativeString::from_os_string("-c".into()),
            NativeString::from_os_string("sleep 10 & echo $!; wait".into()),
        ],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::Direct,
        timeout_ms: 250,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:test".into(),
    };
    let mut config = Config::default();
    config.execution.termination_grace_ms = 25;
    let result = Runner::with_sandbox_binary(sandbox)
        .execute(&job, &config, &paths)
        .await
        .expect("timeout");
    assert_eq!(result.terminal_state, ExecutionState::TimedOut);
    let pid: String = fs::read_to_string(result.stdout_log.to_os_string().expect("path"))
        .expect("pid")
        .trim()
        .into();
    assert!(
        !pid.is_empty(),
        "descendant PID must be recorded before timeout"
    );
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

#[cfg(windows)]
#[tokio::test]
async fn timeout_kills_the_owned_job_object() {
    use std::fs;

    use longrun::{
        config::Config,
        paths::AppPaths,
        protocol::{
            EnvironmentPolicy, ExecutionMode, ExecutionState, JobSpecification, NativeString,
            ShellMode,
        },
        runner::Runner,
    };
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!("longrun-job-{}", Uuid::now_v7()));
    fs::create_dir_all(&root).expect("root");
    let sandbox = root.join("codex.cmd");
    fs::write(
        &sandbox,
        "@echo off\r\n:loop\r\nif \"%~1\"==\"--\" goto run\r\nshift\r\ngoto loop\r\n:run\r\nshift\r\ncall %1 %2 %3 %4 %5 %6 %7 %8 %9\r\n",
    )
    .expect("script");
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
        program: NativeString::from_os_string("cmd.exe".into()),
        args: vec![
            NativeString::from_os_string("/C".into()),
            NativeString::from_os_string("timeout /T 30 /NOBREAK".into()),
        ],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::Direct,
        timeout_ms: 25,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:test".into(),
    };
    let mut config = Config::default();
    config.execution.termination_grace_ms = 25;

    let result = Runner::with_sandbox_binary(sandbox)
        .execute(&job, &config, &paths)
        .await
        .expect("timeout");

    assert_eq!(result.terminal_state, ExecutionState::TimedOut);
    fs::remove_dir_all(root).expect("cleanup");
}
