//! Deterministic target helpers shared by the ephemeral execution tests.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

pub fn test_root(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("longrun-{prefix}-{}", Uuid::now_v7()))
}

pub fn write_executable(root: &Path, name: &str, script: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, script).expect("write fixture executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make fixture executable");
    }
    path
}

pub fn direct_target_args(script: impl Into<OsString>) -> Vec<OsString> {
    vec![
        OsString::from("/bin/sh"),
        OsString::from("-c"),
        script.into(),
    ]
}

pub fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
