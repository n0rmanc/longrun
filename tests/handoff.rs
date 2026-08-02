use std::{fs, sync::Arc};

use longrun::{
    config::Config,
    handoff::{HandoffExpectation, HandoffStore},
    paths::AppPaths,
    protocol::{EnvironmentPolicy, NativeString, TargetSpec},
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

fn target() -> TargetSpec {
    TargetSpec {
        protocol_version: 2,
        program: NativeString::from_os_string("/bin/echo".into()),
        args: vec![NativeString::from_os_string("done".into())],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 100,
        command_hash: "sha256:test".into(),
    }
}

fn expectation(handoff: &longrun::protocol::Handoff) -> HandoffExpectation {
    HandoffExpectation {
        session_id: handoff.session_id.clone(),
        turn_id: handoff.turn_id.clone(),
        tool_use_id: handoff.tool_use_id.clone(),
        cwd: handoff.target.cwd.clone(),
        binary_path: handoff.binary_path.clone(),
    }
}

#[test]
fn handoff_transitions_prepared_to_armed_to_claimed_and_is_deleted() {
    let root = std::env::temp_dir().join(format!("longrun-handoff-{}", Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let store = HandoffStore::new(&paths);
    let handoff = store
        .prepare(
            "session".into(),
            "turn".into(),
            "tool".into(),
            NativeString::from_os_string("/opt/longrun".into()),
            target(),
            100,
            Config::default().handoff.ttl_ms,
        )
        .expect("prepare");
    assert!(store.arm(&handoff.id, 101).expect("arm").is_some());
    assert!(
        store
            .arm(&handoff.id, 102)
            .expect("duplicate arm")
            .is_none()
    );
    let claimed = store
        .claim(&handoff.id, &expectation(&handoff), 103)
        .expect("claim")
        .expect("claimed");
    assert_eq!(
        claimed.handoff.state,
        longrun::protocol::HandoffState::Armed
    );
    assert!(
        store
            .claim(&handoff.id, &expectation(&handoff), 104)
            .expect("replay")
            .is_none()
    );
    store.remove(&claimed).expect("delete");
    assert!(!claimed.path.exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn expired_and_mismatched_handoffs_never_start_a_target() {
    let root = std::env::temp_dir().join(format!("longrun-handoff-expiry-{}", Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let store = HandoffStore::new(&paths);
    let handoff = store
        .prepare(
            "session".into(),
            "turn".into(),
            "tool".into(),
            NativeString::from_os_string("/opt/longrun".into()),
            target(),
            100,
            1,
        )
        .expect("prepare");
    assert!(store.arm(&handoff.id, 102).expect("expired arm").is_none());

    let handoff = store
        .prepare(
            "session".into(),
            "turn".into(),
            "tool".into(),
            NativeString::from_os_string("/opt/longrun".into()),
            target(),
            100,
            1_000,
        )
        .expect("prepare mismatch");
    assert!(store.arm(&handoff.id, 101).expect("arm mismatch").is_some());
    let mut mismatch = expectation(&handoff);
    mismatch.turn_id = "other-turn".into();
    assert!(
        store
            .claim(&handoff.id, &mismatch, 102)
            .expect("mismatch")
            .is_none()
    );
    assert!(store.cleanup_expired(2_000).expect("cleanup") <= 1);
    fs::remove_dir_all(root).expect("cleanup root");
}

#[tokio::test]
async fn concurrent_claims_allow_exactly_one_owner_for_one_handoff() {
    let root = std::env::temp_dir().join(format!("longrun-handoff-race-{}", Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let store = Arc::new(HandoffStore::new(&paths));
    let handoff = store
        .prepare(
            "session".into(),
            "turn".into(),
            "tool".into(),
            NativeString::from_os_string("/opt/longrun".into()),
            target(),
            100,
            1_000,
        )
        .expect("prepare");
    assert!(store.arm(&handoff.id, 101).expect("arm").is_some());
    let expected = expectation(&handoff);
    let mut tasks = Vec::new();
    for _ in 0..100 {
        let store = Arc::clone(&store);
        let expected = expected.clone();
        let id = handoff.id.clone();
        tasks.push(tokio::spawn(async move {
            store.claim(&id, &expected, 102).expect("claim")
        }));
    }
    let mut owners = 0;
    for task in tasks {
        if task.await.expect("join").is_some() {
            owners += 1;
        }
    }
    assert_eq!(owners, 1);
    fs::remove_dir_all(root).expect("cleanup");
}
