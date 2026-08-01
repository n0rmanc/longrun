use std::{ffi::OsString, fs};

use longrun::{
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

fn complete(
    store: &mut Store,
    job: &JobSpecification,
    root: &std::path::Path,
    completed_at_ms: i64,
    delivered: bool,
    stdout: &[u8],
) -> JobResult {
    store.create_job(job).expect("job");
    store.claim_execution(job.job_id, "claim").expect("claim");
    store.mark_running(job.job_id, "claim").expect("running");
    let stdout_log = root.join(format!("{}.stdout.log", job.job_id));
    let stderr_log = root.join(format!("{}.stderr.log", job.job_id));
    fs::write(&stdout_log, stdout).expect("stdout");
    fs::write(&stderr_log, []).expect("stderr");
    let result = JobResult {
        job_id: job.job_id,
        terminal_state: ExecutionState::Succeeded,
        exit_code: Some(0),
        signal: None,
        duration_ms: 1,
        stdout_log: NativeString::from_os_string(stdout_log.into_os_string()),
        stderr_log: NativeString::from_os_string(stderr_log.into_os_string()),
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        result_hash: "sha256:test".into(),
        completed_at_ms,
    };
    store.finish_execution(&result, "claim").expect("finish");
    if delivered {
        store
            .transition_delivery(job.job_id, DeliveryState::HookLeased)
            .expect("lease");
        store
            .transition_delivery(job.job_id, DeliveryState::DeliveredInTurn)
            .expect("deliver");
    }
    result
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

    assert_eq!(store.schema_version().expect("schema version"), 4);
    assert_eq!(store.journal_mode().expect("journal mode"), "wal");
    assert!(store.integrity_check().expect("integrity check"));
    drop(store);
    fs::remove_dir_all(root).expect("remove test state");
}

#[test]
fn version_two_delivery_rows_upgrade_to_leased_recovery_schema() {
    let root = std::env::temp_dir().join(format!("longrun-v2-{}", Uuid::now_v7()));
    fs::create_dir_all(&root).expect("root");
    let database = root.join("state.sqlite");
    let connection = rusqlite::Connection::open(&database).expect("legacy database");
    connection
        .execute_batch(
            "CREATE TABLE deliveries (
                job_id TEXT PRIMARY KEY,
                state TEXT NOT NULL,
                lease_id TEXT,
                lease_expires_at_ms INTEGER,
                idempotency_key TEXT
             );
             PRAGMA user_version = 2;",
        )
        .expect("legacy schema");
    drop(connection);

    let mut store = Store::open(&database).expect("migrate");
    assert_eq!(store.schema_version().expect("schema version"), 4);
    store
        .create_job_for_session(&specification(), Some("session"))
        .expect("current delivery insert");
    drop(store);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn version_three_execution_rows_gain_the_worker_heartbeat_column() {
    let root = std::env::temp_dir().join(format!("longrun-v3-{}", Uuid::now_v7()));
    fs::create_dir_all(&root).expect("root");
    let database = root.join("state.sqlite");
    let connection = rusqlite::Connection::open(&database).expect("legacy database");
    connection
        .execute_batch(
            "CREATE TABLE executions (
                job_id TEXT PRIMARY KEY,
                state TEXT NOT NULL,
                execution_claim TEXT,
                worker_id TEXT,
                pid INTEGER,
                started_at_ms INTEGER,
                finished_at_ms INTEGER,
                cancel_requested_at_ms INTEGER,
                cancel_grace_ms INTEGER
             );
             PRAGMA user_version = 3;",
        )
        .expect("legacy schema");
    drop(connection);

    let store = Store::open(&database).expect("migrate");
    assert_eq!(store.schema_version().expect("schema version"), 4);
    let connection = rusqlite::Connection::open(&database).expect("connection");
    let columns = connection
        .prepare("PRAGMA table_info(executions)")
        .expect("table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("column names");
    assert!(columns.iter().any(|column| column == "heartbeat_at_ms"));
    drop(connection);
    drop(store);
    fs::remove_dir_all(root).expect("cleanup");
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
fn retention_selects_only_delivered_terminal_jobs_by_age_and_log_budget() {
    let root = std::env::temp_dir().join(format!("longrun-retention-{}", Uuid::now_v7()));
    fs::create_dir_all(&root).expect("root");
    let mut store = Store::open_in_memory().expect("store");
    let aged = specification();
    let retained = specification();
    let undelivered = specification();
    complete(&mut store, &aged, &root, 1, true, b"a");
    complete(
        &mut store,
        &retained,
        &root,
        172_800_000,
        true,
        b"1234567890",
    );
    complete(&mut store, &undelivered, &root, 1, false, b"ignored");

    let selected = store
        .retention_candidates(172_800_000, 1, 5)
        .expect("retention");
    assert_eq!(
        selected
            .iter()
            .map(|result| result.job_id)
            .collect::<Vec<_>>(),
        vec![aged.job_id, retained.job_id]
    );
    assert_eq!(
        store.gc(&root, 172_800_000, 1, 5, false).expect("gc"),
        vec![aged.job_id, retained.job_id]
    );
    assert!(store.status(aged.job_id).is_err());
    assert!(store.status(retained.job_id).is_err());
    assert!(!root.join(format!("{}.stdout.log", aged.job_id)).exists());
    assert!(
        !root
            .join(format!("{}.stdout.log", retained.job_id))
            .exists()
    );
    assert_eq!(
        store
            .status(undelivered.job_id)
            .expect("undelivered")
            .delivery_state,
        DeliveryState::Undelivered
    );
    fs::remove_dir_all(root).expect("cleanup");
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
