use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use directories::{BaseDirs, ProjectDirs};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
    pub jobs_dir: PathBuf,
    pub integration_dir: PathBuf,
    pub socket_path: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let project = ProjectDirs::from("dev", "longrun", "Longrun")
            .ok_or_else(|| Error::Unavailable("cannot determine application directories".into()))?;
        let data_dir = project.data_local_dir().to_path_buf();
        let state_dir = data_dir.join("state");
        let runtime_dir = BaseDirs::new()
            .and_then(|dirs| dirs.runtime_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| state_dir.clone());
        #[cfg(unix)]
        let socket_path = unix_socket_path(&runtime_dir, &data_dir);
        #[cfg(not(unix))]
        let socket_path = runtime_dir.join("longrun.sock");

        Ok(Self {
            config_dir: project.config_dir().to_path_buf(),
            log_dir: state_dir.join("logs"),
            jobs_dir: state_dir.join("jobs"),
            integration_dir: data_dir.join("codex"),
            socket_path,
            data_dir,
            state_dir,
        })
    }

    pub fn ensure_private_state(&self) -> Result<()> {
        for path in [
            &self.config_dir,
            &self.data_dir,
            &self.state_dir,
            &self.log_dir,
            &self.jobs_dir,
            &self.integration_dir,
        ] {
            fs::create_dir_all(path)?;
            set_private_permissions(path)?;
        }
        Ok(())
    }

    #[cfg(unix)]
    pub fn ipc_endpoint(&self) -> OsString {
        self.socket_path.clone().into_os_string()
    }

    #[cfg(windows)]
    pub fn ipc_endpoint(&self) -> OsString {
        r"\\.\pipe\longrun".into()
    }
}

#[cfg(unix)]
fn unix_socket_path(runtime_dir: &Path, data_dir: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;

    let candidate = runtime_dir.join("longrun.sock");
    if candidate.as_os_str().as_bytes().len() < 100 {
        return candidate;
    }
    let hash = crate::protocol::sha256_hex(data_dir.as_os_str().as_bytes());
    std::env::temp_dir().join(format!("longrun-{}.sock", &hash[7..23]))
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::ffi::OsStrExt;

    use super::unix_socket_path;

    #[test]
    fn long_runtime_directories_get_a_short_stable_socket_name() {
        let runtime = std::env::temp_dir().join("x".repeat(120));
        let data = std::env::temp_dir().join("longrun-data");
        let first = unix_socket_path(&runtime, &data);
        let second = unix_socket_path(&runtime, &data);
        assert_eq!(first, second);
        assert!(first.as_os_str().as_bytes().len() < 100);
        assert!(first.starts_with(std::env::temp_dir()));
    }
}
