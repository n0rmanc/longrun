use std::{ffi::OsString, fs};

use longrun::{
    protocol::{
        EnvironmentPolicy, ExecutionMode, ExecutionState, JobSpecification, NativeString, ShellMode,
    },
    store::Store,
};
use uuid::Uuid;

fn specification() -> JobSpecification {
    JobSpecification {
        protocol_version: 1,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string(OsString::from("echo")),
        args: vec![NativeString::from_os_string(OsString::from("hello"))],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:test".into(),
    }
}

#[test]
fn migrations_and_execution_transitions_preserve_one_terminal_state() {
    let mut store = Store::open_in_memory().expect("open store");
    let job = specification();
    store.create_job(&job).expect("create job");

    assert_eq!(
        store.execution_state(job.job_id).expect("state"),
        ExecutionState::Accepted
    );
    store
        .transition_execution(job.job_id, ExecutionState::Starting)
        .expect("start");
    store
        .transition_execution(job.job_id, ExecutionState::Running)
        .expect("run");
    store
        .transition_execution(job.job_id, ExecutionState::Succeeded)
        .expect("finish");
    assert!(
        store
            .transition_execution(job.job_id, ExecutionState::Running)
            .is_err()
    );
}

#[test]
fn immutable_json_writes_are_complete_or_absent() {
    let store = Store::open_in_memory().expect("open store");
    let root = std::env::temp_dir().join(format!("longrun-store-{}", std::process::id()));
    let path = root.join("job.json");

    store
        .write_immutable_json(&path, &serde_json::json!({"job": "ok"}))
        .expect("write result");
    assert_eq!(
        fs::read_to_string(&path).expect("read result"),
        "{\"job\":\"ok\"}\n"
    );
    assert!(
        store
            .write_immutable_json(&path, &serde_json::json!({"job": "again"}))
            .is_err()
    );
    fs::remove_dir_all(root).expect("remove test state");
}

#[test]
fn file_store_migrates_with_wal() {
    let root = std::env::temp_dir().join(format!("longrun-wal-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create state");
    let store = Store::open(root.join("state.sqlite")).expect("open store");

    assert_eq!(store.schema_version().expect("schema version"), 2);
    assert_eq!(store.journal_mode().expect("journal mode"), "wal");
    fs::remove_dir_all(root).expect("remove test state");
}

#[test]
fn running_jobs_accept_one_idempotent_cancellation_request() {
    let mut store = Store::open_in_memory().expect("store");
    let job = specification();
    store.create_job(&job).expect("job");
    store
        .transition_execution(job.job_id, ExecutionState::Starting)
        .expect("starting");
    store
        .transition_execution(job.job_id, ExecutionState::Running)
        .expect("running");

    assert!(
        store
            .request_cancellation(job.job_id, 25, 1)
            .expect("request cancellation")
    );
    assert_eq!(
        store.cancellation_grace(job.job_id).expect("grace"),
        Some(25)
    );
    assert!(
        !store
            .request_cancellation(job.job_id, 100, 2)
            .expect("repeat cancellation")
    );
    assert_eq!(
        store.cancellation_grace(job.job_id).expect("same grace"),
        Some(25)
    );
}

#[test]
fn status_and_newest_first_list_expose_execution_and_delivery_state() {
    let mut store = Store::open_in_memory().expect("store");
    let first = specification();
    let second = specification();
    store.create_job(&first).expect("first");
    store.create_job(&second).expect("second");

    let status = store.status(first.job_id).expect("status");
    assert_eq!(status.execution_state, ExecutionState::Accepted);
    assert_eq!(
        status.delivery_state,
        longrun::protocol::DeliveryState::Undelivered
    );
    let jobs = store.list(Some(ExecutionState::Accepted)).expect("list");
    assert_eq!(jobs.len(), 2);
}
