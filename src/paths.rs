use std::{
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
    pub runtime_dir: PathBuf,
    pub handoff_dir: PathBuf,
    pub integration_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let project = ProjectDirs::from("dev", "longrun", "Longrun")
            .ok_or_else(|| Error::Unavailable("cannot determine application directories".into()))?;
        let data_dir = project.data_local_dir().to_path_buf();
        let state_dir = data_dir.join("state");
        let runtime_dir = BaseDirs::new()
            .and_then(|dirs| dirs.runtime_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| state_dir.join("runtime"));

        Ok(Self {
            config_dir: project.config_dir().to_path_buf(),
            integration_dir: data_dir.join("codex"),
            handoff_dir: runtime_dir.join("longrun-handoffs"),
            runtime_dir,
            state_dir,
            data_dir,
        })
    }

    pub fn ensure_private_state(&self) -> Result<()> {
        for path in [
            &self.config_dir,
            &self.data_dir,
            &self.state_dir,
            &self.runtime_dir,
            &self.handoff_dir,
            &self.integration_dir,
        ] {
            fs::create_dir_all(path)?;
            set_private_permissions(path)?;
        }
        Ok(())
    }
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
