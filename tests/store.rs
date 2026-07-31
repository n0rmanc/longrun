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

    assert_eq!(store.schema_version().expect("schema version"), 1);
    assert_eq!(store.journal_mode().expect("journal mode"), "wal");
    fs::remove_dir_all(root).expect("remove test state");
}
