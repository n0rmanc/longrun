use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use getrandom::fill;
use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    paths::AppPaths,
    protocol::{Handoff, HandoffState, NativeString, TargetSpec, sha256_hex},
};

pub const RECEIPT_PREFIX: &str = "LONGRUN_EPHEMERAL_RECEIPT_V1 ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffExpectation {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub cwd: NativeString,
    pub binary_path: NativeString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedHandoff {
    pub handoff: Handoff,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct HandoffStore {
    dir: PathBuf,
}

impl HandoffStore {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dir: paths.handoff_dir.clone(),
        }
    }

    pub fn with_directory(dir: PathBuf) -> Self {
        Self { dir }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &self,
        session_id: String,
        turn_id: String,
        tool_use_id: String,
        binary_path: NativeString,
        target: TargetSpec,
        created_at_ms: i64,
        ttl_ms: u64,
    ) -> Result<Handoff> {
        self.ensure_dir()?;
        let id = random_id()?;
        let ttl_ms = i64::try_from(ttl_ms)
            .map_err(|_| Error::Config("handoff TTL is outside the supported range".into()))?;
        let handoff = Handoff {
            protocol_version: crate::protocol::PROTOCOL_VERSION,
            id: id.clone(),
            session_id,
            turn_id,
            tool_use_id,
            binary_path,
            target,
            created_at_ms,
            expires_at_ms: created_at_ms.saturating_add(ttl_ms),
            state: HandoffState::Prepared,
        };
        self.write(&self.path_for(&id), &handoff)?;
        Ok(handoff)
    }

    pub fn arm(&self, id: &str, now_ms: i64) -> Result<Option<String>> {
        let path = self.path_for(id);
        let Some(mut handoff) = self.read(&path)? else {
            return Ok(None);
        };
        if handoff.id != id || handoff.state != HandoffState::Prepared {
            return Ok(None);
        }
        if handoff.expires_at_ms <= now_ms {
            let _ = fs::remove_file(path);
            return Ok(None);
        }
        handoff.state = HandoffState::Armed;
        self.write(&path, &handoff)?;
        Ok(Some(format!("{RECEIPT_PREFIX}{id}")))
    }

    pub fn claim(
        &self,
        id: &str,
        expectation: &HandoffExpectation,
        now_ms: i64,
    ) -> Result<Option<ClaimedHandoff>> {
        self.ensure_dir()?;
        let source = self.path_for(id);
        let claimed_path = self.claimed_path(id)?;
        match fs::rename(&source, &claimed_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }

        let Some(handoff) = self.read(&claimed_path)? else {
            let _ = fs::remove_file(&claimed_path);
            return Ok(None);
        };
        let valid = handoff.id == id
            && handoff.state == HandoffState::Armed
            && handoff.expires_at_ms > now_ms
            && handoff.session_id == expectation.session_id
            && handoff.turn_id == expectation.turn_id
            && handoff.tool_use_id == expectation.tool_use_id
            && handoff.target.cwd == expectation.cwd
            && handoff.binary_path == expectation.binary_path;
        if !valid {
            let _ = fs::remove_file(&claimed_path);
            return Ok(None);
        }
        Ok(Some(ClaimedHandoff {
            handoff,
            path: claimed_path,
        }))
    }

    pub fn remove(&self, claimed: &ClaimedHandoff) -> Result<()> {
        match fs::remove_file(&claimed.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn cleanup_expired(&self, now_ms: i64) -> Result<usize> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };
        let mut removed = 0;
        for entry in entries {
            let path = entry?.path();
            if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("json")
            {
                continue;
            }
            let Some(handoff) = self.read(&path)? else {
                let _ = fs::remove_file(path);
                removed += 1;
                continue;
            };
            if handoff.expires_at_ms <= now_ms {
                fs::remove_file(path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn path_for_id(&self, id: &str) -> PathBuf {
        self.path_for(id)
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn claimed_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.dir.join(format!("{id}.claimed-{}.json", random_id()?)))
    }

    fn read(&self, path: &Path) -> Result<Option<Handoff>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn write(&self, path: &Path, handoff: &Handoff) -> Result<()> {
        self.ensure_dir()?;
        let bytes = serde_json::to_vec(handoff)?;
        let temp = path.with_file_name(format!(
            ".{}.{}.tmp",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("handoff"),
            random_id()?
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(temp, path)?;
        Ok(())
    }
}

fn random_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    fill(&mut bytes)
        .map_err(|error| Error::Unavailable(format!("cannot obtain handoff id: {error}")))?;
    Ok(sha256_hex(&bytes)[7..39].to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
}

impl Receipt {
    pub fn parse(line: &str) -> Option<&str> {
        line.strip_prefix(RECEIPT_PREFIX).filter(|id| {
            !id.is_empty() && id.chars().all(|character| character.is_ascii_hexdigit())
        })
    }
}
