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
