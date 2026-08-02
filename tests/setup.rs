use std::{fs, process::ExitCode};

use longrun::{error::Error, paths::AppPaths};

use uuid::Uuid;

pub fn ephemeral_paths(root: &std::path::Path) -> AppPaths {
    AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        runtime_dir: root.join("runtime"),
        handoff_dir: root.join("runtime/handoffs"),
        integration_dir: root.join("data/codex"),
    }
}

pub fn ephemeral_root(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("longrun-{prefix}-{}", Uuid::now_v7()))
}

#[test]
fn errors_have_stable_process_exit_codes() {
    assert_eq!(
        Error::InvalidInput("x".into()).exit_code(),
        ExitCode::from(2)
    );
    assert_eq!(Error::Denied("x".into()).exit_code(), ExitCode::from(77));
    assert_eq!(Error::NotFound("x".into()).exit_code(), ExitCode::from(127));
}

#[test]
fn state_directories_are_created() {
    let root = ephemeral_root("setup");
    let paths = ephemeral_paths(&root);

    paths.ensure_private_state().expect("create state");
    assert!(paths.handoff_dir.is_dir());
    assert!(paths.integration_dir.is_dir());
    fs::remove_dir_all(root).expect("remove test state");
}
