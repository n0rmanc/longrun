use std::{fs, process::ExitCode};

use longrun::{error::Error, paths::AppPaths};

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
    let root = std::env::temp_dir().join(format!("longrun-setup-{}", std::process::id()));
    let paths = AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        log_dir: root.join("state/logs"),
        jobs_dir: root.join("state/jobs"),
        integration_dir: root.join("data/codex"),
        socket_path: root.join("longrun.sock"),
    };

    paths.ensure_private_state().expect("create state");
    assert!(paths.log_dir.is_dir());
    assert!(paths.jobs_dir.is_dir());
    assert!(paths.integration_dir.is_dir());
    fs::remove_dir_all(root).expect("remove test state");
}
