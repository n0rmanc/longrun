use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use zeroize::Zeroizing;

use crate::{
    error::{Error, Result},
    protocol::{EnvironmentPolicy, ExecutionMode, JobSpecification, NativeString, ShellMode},
};

const RECEIPT_PREFIX: &str = "LONGRUN_RECEIPT_V1 ";
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptPayload {
    pub receipt_version: u32,
    pub job_id: uuid::Uuid,
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub cwd: NativeString,
    pub program: NativeString,
    pub args: Vec<NativeString>,
    pub execution_mode: ExecutionMode,
    pub shell_mode: ShellMode,
    pub timeout_ms: u64,
    pub permission_profile: String,
    pub environment_policy: EnvironmentPolicy,
    pub command_hash: String,
    pub nonce: String,
    pub issued_at: String,
    pub expires_at: String,
}

impl ReceiptPayload {
    #[allow(clippy::too_many_arguments)]
    pub fn from_job(
        job: JobSpecification,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        tool_use_id: impl Into<String>,
        issued_at: impl Into<String>,
        expires_at: impl Into<String>,
        nonce: impl Into<String>,
    ) -> Self {
        Self {
            receipt_version: 1,
            job_id: job.job_id,
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            tool_use_id: tool_use_id.into(),
            cwd: job.cwd,
            program: job.program,
            args: job.args,
            execution_mode: job.execution_mode,
            shell_mode: job.shell_mode,
            timeout_ms: job.timeout_ms,
            permission_profile: job.permission_profile,
            environment_policy: job.environment_policy,
            command_hash: job.command_hash,
            nonce: nonce.into(),
            issued_at: issued_at.into(),
            expires_at: expires_at.into(),
        }
    }

    pub fn to_job_specification(&self) -> Result<JobSpecification> {
        let issued_at = OffsetDateTime::parse(&self.issued_at, &Rfc3339)
            .map_err(|error| Error::InvalidInput(format!("invalid receipt timestamp: {error}")))?;
        Ok(JobSpecification {
            protocol_version: crate::protocol::PROTOCOL_VERSION,
            job_id: self.job_id,
            program: self.program.clone(),
            args: self.args.clone(),
            cwd: self.cwd.clone(),
            execution_mode: self.execution_mode,
            shell_mode: self.shell_mode,
            timeout_ms: self.timeout_ms,
            permission_profile: self.permission_profile.clone(),
            environment_policy: self.environment_policy.clone(),
            created_at_ms: issued_at.unix_timestamp_nanos().div_euclid(1_000_000) as i64,
            command_hash: self.command_hash.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptExpectation {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub cwd: NativeString,
    pub command_hash: String,
}

impl ReceiptExpectation {
    pub fn from_payload(payload: &ReceiptPayload) -> Self {
        Self {
            session_id: payload.session_id.clone(),
            turn_id: payload.turn_id.clone(),
            tool_use_id: payload.tool_use_id.clone(),
            cwd: payload.cwd.clone(),
            command_hash: payload.command_hash.clone(),
        }
    }

    fn matches(&self, payload: &ReceiptPayload) -> bool {
        self.session_id == payload.session_id
            && self.turn_id == payload.turn_id
            && self.tool_use_id == payload.tool_use_id
            && self.cwd == payload.cwd
            && self.command_hash == payload.command_hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    payload_bytes: Vec<u8>,
    signature: Vec<u8>,
}

impl Receipt {
    pub fn to_line(&self) -> String {
        format!(
            "{RECEIPT_PREFIX}{}.{}",
            URL_SAFE_NO_PAD.encode(&self.payload_bytes),
            URL_SAFE_NO_PAD.encode(&self.signature)
        )
    }

    pub fn verify(
        &self,
        signer: &ReceiptSigner,
        expectation: &ReceiptExpectation,
        now: OffsetDateTime,
    ) -> Result<ReceiptPayload> {
        signer.verify_bytes(&self.payload_bytes, &self.signature)?;
        let payload: ReceiptPayload = serde_json::from_slice(&self.payload_bytes)?;
        if payload.receipt_version != 1 {
            return Err(Error::InvalidInput("unsupported receipt version".into()));
        }
        if !expectation.matches(&payload) {
            return Err(Error::Denied(
                "receipt context does not match pending submission".into(),
            ));
        }
        let expires_at = OffsetDateTime::parse(&payload.expires_at, &Rfc3339)
            .map_err(|error| Error::InvalidInput(format!("invalid receipt expiry: {error}")))?;
        if expires_at <= now {
            return Err(Error::Denied("receipt has expired".into()));
        }
        Ok(payload)
    }
}

pub struct ReceiptSigner {
    secret: Zeroizing<[u8; 32]>,
}

impl ReceiptSigner {
    pub fn new(secret: [u8; 32]) -> Self {
        Self {
            secret: Zeroizing::new(secret),
        }
    }

    pub fn load_or_create(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(secret) => return Self::from_bytes(secret),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let parent = path
            .parent()
            .ok_or_else(|| Error::InvalidInput("receipt secret has no parent directory".into()))?;
        fs::create_dir_all(parent)?;
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|error| {
            Error::Unavailable(format!("cannot obtain receipt entropy: {error}"))
        })?;
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                file.write_all(&secret)?;
                file.sync_all()?;
                set_secret_permissions(path)?;
                Ok(Self::new(secret))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::from_bytes(fs::read(path)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn issue(&self, payload: &ReceiptPayload) -> Result<Receipt> {
        let payload_bytes = serde_json::to_vec(payload)?;
        let signature = self.sign_bytes(&payload_bytes)?;
        Ok(Receipt {
            payload_bytes,
            signature,
        })
    }

    pub fn random_nonce() -> Result<String> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| {
            Error::Unavailable(format!("cannot obtain receipt entropy: {error}"))
        })?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn parse(&self, line: &str) -> Result<Receipt> {
        let encoded = line
            .strip_prefix(RECEIPT_PREFIX)
            .ok_or_else(|| Error::InvalidInput("missing Longrun receipt prefix".into()))?;
        let (payload, signature) = encoded
            .split_once('.')
            .ok_or_else(|| Error::InvalidInput("malformed Longrun receipt".into()))?;
        if signature.contains('.') {
            return Err(Error::InvalidInput("malformed Longrun receipt".into()));
        }
        Ok(Receipt {
            payload_bytes: URL_SAFE_NO_PAD.decode(payload).map_err(|error| {
                Error::InvalidInput(format!("invalid receipt payload: {error}"))
            })?,
            signature: URL_SAFE_NO_PAD.decode(signature).map_err(|error| {
                Error::InvalidInput(format!("invalid receipt signature: {error}"))
            })?,
        })
    }

    fn from_bytes(secret: Vec<u8>) -> Result<Self> {
        let secret: [u8; 32] = secret
            .try_into()
            .map_err(|_| Error::Config("receipt secret must be exactly 32 bytes".into()))?;
        Ok(Self::new(secret))
    }

    fn sign_bytes(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut mac = HmacSha256::new_from_slice(self.secret.as_ref()).map_err(|error| {
            Error::Unavailable(format!("cannot initialize receipt HMAC: {error}"))
        })?;
        mac.update(payload);
        Ok(mac.finalize().into_bytes().to_vec())
    }

    fn verify_bytes(&self, payload: &[u8], signature: &[u8]) -> Result<()> {
        let mut mac = HmacSha256::new_from_slice(self.secret.as_ref()).map_err(|error| {
            Error::Unavailable(format!("cannot initialize receipt HMAC: {error}"))
        })?;
        mac.update(payload);
        mac.verify_slice(signature)
            .map_err(|_| Error::Denied("receipt signature is invalid".into()))
    }
}

#[cfg(unix)]
fn set_secret_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_secret_permissions(_: &Path) -> Result<()> {
    Ok(())
}
