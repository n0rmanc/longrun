use std::ffi::{OsStr, OsString};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{Error, Result};

pub const PROTOCOL_VERSION: u32 = 1;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Embedded,
    Durable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellMode {
    Direct,
    ExplicitShell,
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

pub fn default_deny_patterns() -> Vec<String> {
    ["SECRET", "TOKEN", "PASSWORD", "API_KEY", "PRIVATE_KEY"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub fn sha256_hex(input: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(input))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingState {
    Pending,
    Claimed,
    Consumed,
    Rejected,
}

impl PendingState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Consumed => "consumed",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingSubmission {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub cwd: NativeString,
    pub binary_path: NativeString,
    pub expected_program: NativeString,
    pub expected_args: Vec<NativeString>,
    pub command_hash: String,
    pub hook_token_hash: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub state: PendingState,
}

impl PendingSubmission {
    pub fn matches_job(&self, job: &JobSpecification) -> bool {
        self.cwd == job.cwd
            && self.expected_program == job.program
            && self.expected_args == job.args
            && self.command_hash == job.command_hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSpecification {
    pub protocol_version: u32,
    pub job_id: Uuid,
    pub program: NativeString,
    pub args: Vec<NativeString>,
    pub cwd: NativeString,
    pub execution_mode: ExecutionMode,
    pub shell_mode: ShellMode,
    pub timeout_ms: u64,
    pub permission_profile: String,
    pub environment_policy: EnvironmentPolicy,
    pub created_at_ms: i64,
    pub command_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobResult {
    pub job_id: Uuid,
    pub terminal_state: ExecutionState,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub duration_ms: u64,
    pub stdout_log: NativeString,
    pub stderr_log: NativeString,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub result_hash: String,
    pub completed_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Accepted,
    Starting,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

impl ExecutionState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Cancelled
        )
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Accepted, Self::Starting)
                | (Self::Starting, Self::Running | Self::Failed)
                | (
                    Self::Running,
                    Self::Succeeded | Self::Failed | Self::TimedOut | Self::Cancelled
                )
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for ExecutionState {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "timed_out" => Ok(Self::TimedOut),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(Error::InvalidInput(format!(
                "unknown execution state: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Undelivered,
    HookLeased,
    DeliveredInTurn,
    SessionStartLeased,
    DeliveredOnStart,
    ResumeLeased,
    ResumeStarted,
    DeliveredByResume,
}

impl DeliveryState {
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Undelivered,
                Self::HookLeased | Self::SessionStartLeased | Self::ResumeLeased
            ) | (Self::HookLeased, Self::Undelivered | Self::DeliveredInTurn)
                | (
                    Self::SessionStartLeased,
                    Self::Undelivered | Self::DeliveredOnStart
                )
                | (Self::ResumeLeased, Self::Undelivered | Self::ResumeStarted)
                | (
                    Self::ResumeStarted,
                    Self::Undelivered | Self::DeliveredByResume
                )
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Undelivered => "undelivered",
            Self::HookLeased => "hook_leased",
            Self::DeliveredInTurn => "delivered_in_turn",
            Self::SessionStartLeased => "session_start_leased",
            Self::DeliveredOnStart => "delivered_on_start",
            Self::ResumeLeased => "resume_leased",
            Self::ResumeStarted => "resume_started",
            Self::DeliveredByResume => "delivered_by_resume",
        }
    }
}

impl std::str::FromStr for DeliveryState {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "undelivered" => Ok(Self::Undelivered),
            "hook_leased" => Ok(Self::HookLeased),
            "delivered_in_turn" => Ok(Self::DeliveredInTurn),
            "session_start_leased" => Ok(Self::SessionStartLeased),
            "delivered_on_start" => Ok(Self::DeliveredOnStart),
            "resume_leased" => Ok(Self::ResumeLeased),
            "resume_started" => Ok(Self::ResumeStarted),
            "delivered_by_resume" => Ok(Self::DeliveredByResume),
            _ => Err(Error::InvalidInput(format!(
                "unknown delivery state: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcMethod {
    Submit,
    Wait,
    Status,
    List,
    Logs,
    Cancel,
    Gc,
    Health,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcRequest {
    pub protocol_version: u32,
    pub request_id: Uuid,
    pub method: IpcMethod,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcResponse {
    pub protocol_version: u32,
    pub request_id: Uuid,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcEventKind {
    Accepted,
    Started,
    OutputAvailable,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcEvent {
    pub protocol_version: u32,
    pub job_id: Uuid,
    pub event: IpcEventKind,
    pub payload: serde_json::Value,
}
