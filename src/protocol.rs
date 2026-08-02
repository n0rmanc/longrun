use std::ffi::{OsStr, OsString};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeEncoding {
    Utf8,
    UnixBytes,
    WindowsUtf16Le,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeString {
    pub encoding: NativeEncoding,
    pub value: String,
}

impl NativeString {
    pub fn from_os_string(value: OsString) -> Self {
        native_from_os_string(value)
    }

    pub fn from_os_str(value: &OsStr) -> Self {
        Self::from_os_string(value.to_os_string())
    }

    pub fn to_os_string(&self) -> Result<OsString> {
        native_to_os_string(self)
    }

    pub fn from_windows_units(units: &[u16]) -> Self {
        let bytes = units
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>();
        Self {
            encoding: NativeEncoding::WindowsUtf16Le,
            value: URL_SAFE_NO_PAD.encode(bytes),
        }
    }

    pub fn to_windows_units(&self) -> Result<Vec<u16>> {
        if self.encoding != NativeEncoding::WindowsUtf16Le {
            return Err(Error::InvalidInput("native string is not UTF-16LE".into()));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.value)
            .map_err(|error| Error::InvalidInput(format!("invalid native string: {error}")))?;
        if bytes.len() % 2 != 0 {
            return Err(Error::InvalidInput(
                "UTF-16LE byte length must be even".into(),
            ));
        }
        Ok(bytes
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect())
    }
}

#[cfg(unix)]
fn native_from_os_string(value: OsString) -> NativeString {
    use std::os::unix::ffi::OsStringExt;

    match value.into_string() {
        Ok(value) => NativeString {
            encoding: NativeEncoding::Utf8,
            value,
        },
        Err(value) => NativeString {
            encoding: NativeEncoding::UnixBytes,
            value: URL_SAFE_NO_PAD.encode(value.into_vec()),
        },
    }
}

#[cfg(windows)]
fn native_from_os_string(value: OsString) -> NativeString {
    use std::os::windows::ffi::OsStrExt;

    match value.to_str() {
        Some(value) => NativeString {
            encoding: NativeEncoding::Utf8,
            value: value.to_owned(),
        },
        None => NativeString::from_windows_units(&value.encode_wide().collect::<Vec<_>>()),
    }
}

#[cfg(not(any(unix, windows)))]
fn native_from_os_string(value: OsString) -> NativeString {
    NativeString {
        encoding: NativeEncoding::Utf8,
        value: value.to_string_lossy().into_owned(),
    }
}

#[cfg(unix)]
fn native_to_os_string(value: &NativeString) -> Result<OsString> {
    use std::os::unix::ffi::OsStringExt;

    match value.encoding {
        NativeEncoding::Utf8 => Ok(value.value.clone().into()),
        NativeEncoding::UnixBytes => URL_SAFE_NO_PAD
            .decode(&value.value)
            .map(OsString::from_vec)
            .map_err(|error| Error::InvalidInput(format!("invalid native string: {error}"))),
        NativeEncoding::WindowsUtf16Le => Err(Error::InvalidInput(
            "cannot execute Windows UTF-16 arguments on Unix".into(),
        )),
    }
}

#[cfg(windows)]
fn native_to_os_string(value: &NativeString) -> Result<OsString> {
    use std::os::windows::ffi::OsStringExt;

    match value.encoding {
        NativeEncoding::Utf8 => Ok(value.value.clone().into()),
        NativeEncoding::WindowsUtf16Le => Ok(OsString::from_wide(&value.to_windows_units()?)),
        NativeEncoding::UnixBytes => Err(Error::InvalidInput(
            "cannot execute Unix byte arguments on Windows".into(),
        )),
    }
}

#[cfg(not(any(unix, windows)))]
fn native_to_os_string(value: &NativeString) -> Result<OsString> {
    match value.encoding {
        NativeEncoding::Utf8 => Ok(value.value.clone().into()),
        _ => Err(Error::InvalidInput(
            "unsupported native string encoding on this platform".into(),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentPolicy {
    #[serde(default)]
    pub pass: Vec<String>,
    #[serde(default = "default_deny_patterns")]
    pub deny_patterns: Vec<String>,
}

impl Default for EnvironmentPolicy {
    fn default() -> Self {
        Self {
            pass: Vec::new(),
            deny_patterns: default_deny_patterns(),
        }
    }
}

impl EnvironmentPolicy {
    pub fn is_protected(&self, name: &str) -> bool {
        let name = name.to_ascii_uppercase();
        self.deny_patterns
            .iter()
            .map(|pattern| pattern.to_ascii_uppercase())
            .any(|pattern| name.contains(&pattern))
    }

    pub fn allows(&self, name: &str) -> bool {
        self.pass.iter().any(|allowed| allowed == name)
    }
}

pub fn default_deny_patterns() -> Vec<String> {
    ["SECRET", "TOKEN", "PASSWORD", "API_KEY", "PRIVATE_KEY"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub fn sha256_hex(input: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(input))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSpec {
    pub protocol_version: u32,
    pub program: NativeString,
    pub args: Vec<NativeString>,
    pub cwd: NativeString,
    pub timeout_ms: u64,
    pub permission_profile: String,
    pub environment_policy: EnvironmentPolicy,
    pub created_at_ms: i64,
    pub command_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffState {
    Prepared,
    Armed,
    Claimed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    pub protocol_version: u32,
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub binary_path: NativeString,
    pub target: TargetSpec,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub state: HandoffState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalReason {
    Exited,
    TimedOut,
    Cancelled,
    OwnerShutdown,
    SpawnFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedOutput {
    pub total_bytes: u64,
    pub tail_base64url: String,
    pub truncated: bool,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultEnvelope {
    pub protocol_version: u32,
    pub terminal_reason: TerminalReason,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub duration_ms: u64,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
}
