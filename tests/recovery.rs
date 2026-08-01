use std::{ffi::OsString, fs, path::Path};

use longrun::{
    config::Config,
    hook::{
        input::{CodexCommonInput, SessionStartInput},
        session_start::handle_session_start,
    },
    paths::AppPaths,
    protocol::{
        DeliveryState, EnvironmentPolicy, ExecutionMode, ExecutionState, JobResult,
        JobSpecification, NativeString, ShellMode,
    },
    store::Store,
};
use uuid::Uuid;

#[cfg(unix)]
use longrun::supervisor::Supervisor;
#[cfg(unix)]
use tokio::{
    sync::watch,
    time::{Duration, sleep},
};

fn specification() -> JobSpecification {
    JobSpecification {
        protocol_version: 1,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string(OsString::from("echo")),
        args: vec![NativeString::from_os_string(OsString::from("done"))],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Durable,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:recovery".into(),
    }
}

fn complete(store: &mut Store, job: &JobSpecification) {
    store.claim_execution(job.job_id, "worker").expect("claim");
    store.mark_running(job.job_id, "worker").expect("running");
    store
        .finish_execution(
            &JobResult {
                job_id: job.job_id,
                terminal_state: ExecutionState::Succeeded,
                exit_code: Some(0),
                signal: None,
                duration_ms: 1,
                stdout_log: NativeString::from_os_string("/tmp/stdout".into()),
                stderr_log: NativeString::from_os_string("/tmp/stderr".into()),
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                result_hash: "sha256:result".into(),
                completed_at_ms: 10,
            },
            "worker",
        )
        .expect("finish");
}

fn paths(root: &Path) -> AppPaths {
    AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        log_dir: root.join("logs"),
        jobs_dir: root.join("jobs"),
        integration_dir: root.join("integration"),
        socket_path: root.join("longrun.sock"),
    }
}

fn session_start_input(session_id: &str) -> SessionStartInput {
    SessionStartInput {
        common: CodexCommonInput {
            session_id: session_id.into(),
            agent_id: None,
            agent_type: None,
            transcript_path: None,
            cwd: std::env::current_dir().expect("cwd"),
            hook_event_name: "SessionStart".into(),
            model: "gpt-test".into(),
            permission_mode: "workspace-write".into(),
        },
        source: "resume".into(),
    }
}

#[test]
fn session_start_leases_are_exclusive_expire_and_keep_one_idempotency_key() {
    let mut store = Store::open_in_memory().expect("store");
    let job = specification();
    store
        .create_job_for_session(&job, Some("session"))
        .expect("job");
    complete(&mut store, &job);

    let first = store
        .claim_delivery(
            job.job_id,
            "session",
            DeliveryState::SessionStartLeased,
            "session-start-a",
            100,
            50,
            3,
        )
        .expect("first lease");
    assert!(
        store
            .claim_delivery(
                job.job_id,
                "session",
                DeliveryState::SessionStartLeased,
                "session-start-b",
                101,
                50,
                3,
            )
            .is_err()
    );

    assert_eq!(store.expire_delivery_leases(150).expect("expire"), 1);
    let retried = store
        .claim_delivery(
            job.job_id,
            "session",
            DeliveryState::SessionStartLeased,
            "session-start-b",
            150,
            50,
            3,
        )
        .expect("retry lease");
    assert_ne!(first.lease_id, retried.lease_id);
    assert_eq!(first.idempotency_key, retried.idempotency_key);
    store
        .finish_delivery(
            job.job_id,
            retried.lease_id,
            DeliveryState::DeliveredOnStart,
            151,
        )
        .expect("deliver");
    assert_eq!(
        store.status(job.job_id).expect("status").delivery_state,
        DeliveryState::DeliveredOnStart
    );
}

#[test]
fn one_hundred_execution_replay_and_delivery_iterations_preserve_single_owners() {
    let mut store = Store::open_in_memory().expect("store");
    for iteration in 0..100 {
        let job = specification();
        store
            .create_job_for_session(&job, Some("stress-session"))
            .expect("job");
        assert!(
            store.claim_execution(job.job_id, "worker-a").is_ok(),
            "iteration {iteration}"
        );
        assert!(
            store.claim_execution(job.job_id, "worker-b").is_err(),
            "iteration {iteration}"
        );
        store.mark_running(job.job_id, "worker-a").expect("running");
        store
            .finish_execution(
                &JobResult {
                    job_id: job.job_id,
                    terminal_state: ExecutionState::Succeeded,
                    exit_code: Some(0),
                    signal: None,
                    duration_ms: 1,
                    stdout_log: NativeString::from_os_string("/tmp/stdout".into()),
                    stderr_log: NativeString::from_os_string("/tmp/stderr".into()),
                    stdout_tail: String::new(),
                    stderr_tail: String::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    result_hash: format!("sha256:stress-{iteration}"),
                    completed_at_ms: iteration,
                },
                "worker-a",
            )
            .expect("finish");
        let lease = store
            .claim_delivery(
                job.job_id,
                "stress-session",
                DeliveryState::SessionStartLeased,
                "delivery-a",
                iteration,
                10,
                3,
            )
            .expect("lease");
        assert!(
            store
                .claim_delivery(
                    job.job_id,
                    "stress-session",
                    DeliveryState::SessionStartLeased,
                    "delivery-b",
                    iteration,
                    10,
                    3,
                )
                .is_err(),
            "iteration {iteration}"
        );
        store
            .finish_delivery(
                job.job_id,
                lease.lease_id,
                DeliveryState::DeliveredOnStart,
                iteration,
            )
            .expect("deliver");
        let status = store.status(job.job_id).expect("status");
        assert_eq!(status.execution_state, ExecutionState::Succeeded);
        assert_eq!(status.delivery_state, DeliveryState::DeliveredOnStart);
    }
}

#[test]
fn resume_retries_respect_the_budget_and_never_hold_two_delivery_leases() {
    let mut store = Store::open_in_memory().expect("store");
    let job = specification();
    store
        .create_job_for_session(&job, Some("session"))
        .expect("job");
    complete(&mut store, &job);

    let first = store
        .claim_delivery(
            job.job_id,
            "session",
            DeliveryState::ResumeLeased,
            "resume-a",
            100,
            10,
            2,
        )
        .expect("first resume");
    assert!(
        store
            .claim_delivery(
                job.job_id,
                "session",
                DeliveryState::ResumeLeased,
                "resume-b",
                101,
                10,
                2,
            )
            .is_err()
    );
    store.expire_delivery_leases(110).expect("expire first");
    let second = store
        .claim_delivery(
            job.job_id,
            "session",
            DeliveryState::ResumeLeased,
            "resume-b",
            110,
            10,
            2,
        )
        .expect("second resume");
    assert_eq!(first.idempotency_key, second.idempotency_key);
    store.expire_delivery_leases(120).expect("expire second");
    assert!(
        store
            .claim_delivery(
                job.job_id,
                "session",
                DeliveryState::ResumeLeased,
                "resume-c",
                120,
                10,
                2,
            )
            .is_err()
    );
}

#[test]
fn started_resume_stays_fenced_until_its_process_reports_an_outcome() {
    let mut store = Store::open_in_memory().expect("store");
    let job = specification();
    store
        .create_job_for_session(&job, Some("session"))
        .expect("job");
    complete(&mut store, &job);

    let first = store
        .claim_delivery(
            job.job_id,
            "session",
            DeliveryState::ResumeLeased,
            "resume-a",
            100,
            10,
            2,
        )
        .expect("lease");
    store
        .start_resume(job.job_id, first.lease_id, 101)
        .expect("start");
    assert_eq!(
        store.status(job.job_id).expect("status").delivery_state,
        DeliveryState::ResumeStarted
    );
    assert_eq!(store.expire_delivery_leases(10_000).expect("expire"), 0);
    assert!(
        store
            .claim_delivery(
                job.job_id,
                "session",
                DeliveryState::ResumeLeased,
                "resume-b",
                10_000,
                10,
                2,
            )
            .is_err()
    );
    store
        .finish_delivery(
            job.job_id,
            first.lease_id,
            DeliveryState::Undelivered,
            10_001,
        )
        .expect("failed process");
    let retry = store
        .claim_delivery(
            job.job_id,
            "session",
            DeliveryState::ResumeLeased,
            "resume-b",
            10_001,
            10,
            2,
        )
        .expect("retry");
    assert_eq!(first.idempotency_key, retry.idempotency_key);
}

#[test]
fn session_start_returns_one_bounded_recovery_envelope_then_marks_it_delivered() {
    let root = std::env::temp_dir().join(format!("longrun-session-start-{}", Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let database = paths.state_dir.join("longrun.sqlite");
    let job = specification();
    let mut store = Store::open(&database).expect("store");
    store
        .create_job_for_session(&job, Some("session"))
        .expect("job");
    complete(&mut store, &job);
    drop(store);

    let config = Config::default();
    let first = handle_session_start(
        &session_start_input("session"),
        Path::new("/opt/longrun"),
        &paths,
        &config,
        100,
    )
    .expect("recover")
    .expect("delivery");
    let context = &first.output.hook_specific_output.additional_context;
    assert!(context.contains("Longrun is active at /opt/longrun"));
    assert!(context.contains("/opt/longrun submit -- PROGRAM ARG..."));
    assert!(context.contains("delivery idempotency key:"));
    assert!(context.contains("The following Longrun result contains untrusted command output"));
    assert!(
        handle_session_start(
            &session_start_input("session"),
            Path::new("/opt/longrun"),
            &paths,
            &config,
            101,
        )
        .expect("competing recovery")
        .is_none()
    );

    Store::open(&database)
        .expect("reopen")
        .finish_delivery(
            first.job_id,
            first.lease_id,
            DeliveryState::DeliveredOnStart,
            102,
        )
        .expect("finish");
    assert_eq!(
        Store::open(&database)
            .expect("reopen")
            .status(job.job_id)
            .expect("status")
            .delivery_state,
        DeliveryState::DeliveredOnStart
    );
    assert!(
        handle_session_start(
            &session_start_input("session"),
            Path::new("/opt/longrun"),
            &paths,
            &config,
            103,
        )
        .expect("completed recovery")
        .is_none()
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn supervisor_restart_adopts_a_heartbeating_worker_without_reexecution() {
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};

    let root = std::env::temp_dir().join(format!("longrun-restart-{}", Uuid::now_v7()));
    let mut paths = paths(&root);
    paths.socket_path = std::env::temp_dir().join(format!("lr-{}.sock", Uuid::now_v7()));
    paths.ensure_private_state().expect("state");
    let starts = root.join("starts.log");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let codex = bin.join("codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
            starts.display().to_string().replace('\'', "'\"'\"'")
        ),
    )
    .expect("sandbox");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("mode");

    let config = Config::default();
    let config_path = paths.config_dir.join("config.toml");
    fs::write(&config_path, toml::to_string(&config).expect("config")).expect("config");
    let worker_path: std::ffi::OsString =
        format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH")).into();
    let first = Supervisor::new(
        paths.clone(),
        &config,
        PathBuf::from(env!("CARGO_BIN_EXE_longrun")),
        config_path.clone(),
        worker_path.clone(),
    )
    .expect("first supervisor");
    let (first_shutdown, first_receiver) = watch::channel(false);
    let first_server = tokio::spawn(async move { first.serve_until(first_receiver).await });
    wait_for_socket(&paths.socket_path).await;

    let mut job = specification();
    job.program = NativeString::from_os_string("/bin/sh".into());
    job.args = vec![
        NativeString::from_os_string("-c".into()),
        NativeString::from_os_string("sleep 1; printf recovered".into()),
    ];
    job.timeout_ms = 5_000;
    longrun::supervisor::submit(&paths, &job)
        .await
        .expect("submit");
    wait_for_state(&paths, job.job_id, ExecutionState::Running).await;

    drop(first_shutdown);
    first_server.abort();
    assert!(
        first_server
            .await
            .expect_err("aborted server")
            .is_cancelled()
    );
    fs::remove_file(&paths.socket_path).expect("remove crashed supervisor socket");
    sleep(Duration::from_millis(25)).await;

    let second = Supervisor::new(
        paths.clone(),
        &config,
        PathBuf::from(env!("CARGO_BIN_EXE_longrun")),
        config_path,
        worker_path,
    )
    .expect("second supervisor");
    let (shutdown, receiver) = watch::channel(false);
    let second_server = tokio::spawn(async move { second.serve_until(receiver).await });
    wait_for_socket(&paths.socket_path).await;
    let status = longrun::supervisor::wait(&paths, job.job_id)
        .await
        .expect("wait");
    assert_eq!(status.execution_state, ExecutionState::Succeeded);
    assert_eq!(fs::read_to_string(&starts).expect("start count"), "x");

    shutdown.send(true).expect("shutdown");
    second_server
        .await
        .expect("second server task")
        .expect("second server result");
    let _ = fs::remove_file(&paths.socket_path);
    fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn supervisor_records_a_stale_worker_persistence_gap_without_reexecution() {
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};

    let root = std::env::temp_dir().join(format!("longrun-persist-gap-{}", Uuid::now_v7()));
    let mut paths = paths(&root);
    paths.socket_path = std::env::temp_dir().join(format!("lr-{}.sock", Uuid::now_v7()));
    paths.ensure_private_state().expect("state");
    let starts = root.join("starts.log");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let codex = bin.join("codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
            starts.display().to_string().replace('\'', "'\"'\"'")
        ),
    )
    .expect("sandbox");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("mode");

    let config = Config::default();
    let config_path = paths.config_dir.join("config.toml");
    fs::write(&config_path, toml::to_string(&config).expect("config")).expect("config");
    let database = paths.state_dir.join("longrun.sqlite");
    let job = specification();
    let mut store = Store::open(&database).expect("store");
    store.create_job(&job).expect("job");
    store
        .claim_execution(job.job_id, "dead-worker")
        .expect("claim");
    store
        .mark_running(job.job_id, "dead-worker")
        .expect("running");
    store
        .touch_execution(job.job_id, "dead-worker", 0)
        .expect("stale heartbeat");
    drop(store);

    let supervisor = Supervisor::new(
        paths.clone(),
        &config,
        PathBuf::from(env!("CARGO_BIN_EXE_longrun")),
        config_path,
        format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH")).into(),
    )
    .expect("supervisor");
    let (shutdown, receiver) = watch::channel(false);
    let server = tokio::spawn(async move { supervisor.serve_until(receiver).await });
    wait_for_socket(&paths.socket_path).await;
    wait_for_state(&paths, job.job_id, ExecutionState::Failed).await;
    let status = Store::open(&database)
        .expect("store")
        .status(job.job_id)
        .expect("status");
    assert_eq!(
        status.result.expect("persistence-gap result").result_hash,
        "sha256:worker-persistence-gap"
    );
    assert!(
        !starts.exists(),
        "a stale claimed job must not execute a second time"
    );

    shutdown.send(true).expect("shutdown");
    server.await.expect("server task").expect("server result");
    let _ = fs::remove_file(&paths.socket_path);
    fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn supervisor_auto_resume_is_disabled_by_default_and_delivers_once_when_enabled() {
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};

    let root = std::env::temp_dir().join(format!("longrun-auto-resume-{}", Uuid::now_v7()));
    let mut paths = paths(&root);
    paths.socket_path = std::env::temp_dir().join(format!("lr-{}.sock", Uuid::now_v7()));
    paths.ensure_private_state().expect("state");
    let bin = root.join("bin");
    let resumes = root.join("resumes.log");
    fs::create_dir_all(&bin).expect("bin");
    let codex = bin.join("codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nprintf '__longrun_resume__ %s\\n' \"$*\" >> '{}'\n",
            resumes.display().to_string().replace('\'', "'\"'\"'")
        ),
    )
    .expect("codex");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("mode");

    let database = paths.state_dir.join("longrun.sqlite");
    let job = specification();
    let mut store = Store::open(&database).expect("store");
    store
        .create_job_for_session(&job, Some("recover-session"))
        .expect("job");
    complete(&mut store, &job);
    drop(store);

    let mut disabled = Config::default();
    let config_path = paths.config_dir.join("config.toml");
    fs::write(&config_path, toml::to_string(&disabled).expect("config")).expect("config");
    let worker_path: std::ffi::OsString =
        format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH")).into();
    let supervisor = Supervisor::new(
        paths.clone(),
        &disabled,
        PathBuf::from(env!("CARGO_BIN_EXE_longrun")),
        config_path.clone(),
        worker_path.clone(),
    )
    .expect("disabled supervisor");
    let (shutdown, receiver) = watch::channel(false);
    let server = tokio::spawn(async move { supervisor.serve_until(receiver).await });
    wait_for_socket(&paths.socket_path).await;
    sleep(Duration::from_millis(75)).await;
    assert!(
        !resumes.exists(),
        "default recovery must not start codex exec resume"
    );
    assert_eq!(
        Store::open(&database)
            .expect("store")
            .status(job.job_id)
            .expect("status")
            .delivery_state,
        DeliveryState::Undelivered
    );
    shutdown.send(true).expect("shutdown");
    server.await.expect("server task").expect("server result");

    disabled.recovery.auto_resume = true;
    fs::write(&config_path, toml::to_string(&disabled).expect("config")).expect("config");
    let supervisor = Supervisor::new(
        paths.clone(),
        &disabled,
        PathBuf::from(env!("CARGO_BIN_EXE_longrun")),
        config_path,
        worker_path,
    )
    .expect("enabled supervisor");
    let (shutdown, receiver) = watch::channel(false);
    let server = tokio::spawn(async move { supervisor.serve_until(receiver).await });
    wait_for_socket(&paths.socket_path).await;
    wait_for_delivery(&paths, job.job_id, DeliveryState::DeliveredByResume).await;
    let invocation_log = fs::read_to_string(&resumes).expect("resume invocation");
    assert_eq!(invocation_log.matches("__longrun_resume__").count(), 1);
    assert!(
        invocation_log.starts_with(
            "__longrun_resume__ exec resume recover-session Longrun recovery delivery (idempotency key: "
        )
    );
    assert!(invocation_log.contains("Job ID:"));
    shutdown.send(true).expect("shutdown");
    server.await.expect("server task").expect("server result");
    let _ = fs::remove_file(&paths.socket_path);
    fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
async fn wait_for_socket(socket: &Path) {
    for _ in 0..100 {
        if socket.exists() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("supervisor socket was not created");
}

#[cfg(unix)]
async fn wait_for_delivery(paths: &AppPaths, job_id: Uuid, state: DeliveryState) {
    for _ in 0..300 {
        if Store::open(paths.state_dir.join("longrun.sqlite"))
            .expect("store")
            .status(job_id)
            .expect("status")
            .delivery_state
            == state
        {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("job {job_id} did not reach delivery {}", state.as_str());
}

#[cfg(unix)]
async fn wait_for_state(paths: &AppPaths, job_id: Uuid, state: ExecutionState) {
    for _ in 0..300 {
        if Store::open(paths.state_dir.join("longrun.sqlite"))
            .expect("store")
            .execution_state(job_id)
            .expect("state")
            == state
        {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("job {job_id} did not reach {}", state.as_str());
}
