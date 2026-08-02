use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const MAX_TIMEOUT_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const DEFAULT_HANDOFF_TTL_MS: u64 = 5 * 60 * 1_000;
const MAX_HANDOFF_TTL_MS: u64 = 15 * 60 * 1_000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub execution: ExecutionConfig,
    pub handoff: HandoffConfig,
    pub output: OutputConfig,
    pub diagnostics: DiagnosticsConfig,
}

impl Config {
    pub fn from_toml(source: &str) -> Result<Self> {
        let config: Self =
            toml::from_str(source).map_err(|error| Error::Config(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(source) => Self::from_toml(&source),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.execution.timeout_ms == 0 || self.execution.timeout_ms > MAX_TIMEOUT_MS {
            return Err(Error::Config(
                "execution.timeout_ms is outside the allowed range".into(),
            ));
        }
        if self.execution.termination_grace_ms == 0 {
            return Err(Error::Config(
                "execution.termination_grace_ms must be positive".into(),
            ));
        }
        if self.execution.forced_cleanup_margin_ms == 0 {
            return Err(Error::Config(
                "execution.forced_cleanup_margin_ms must be positive".into(),
            ));
        }
        if self.execution.result_serialization_margin_ms == 0 {
            return Err(Error::Config(
                "execution.result_serialization_margin_ms must be positive".into(),
            ));
        }
        if self.output.model_max_bytes == 0 || self.output.tail_bytes == 0 {
            return Err(Error::Config("output byte limits must be positive".into()));
        }
        if self.handoff.ttl_ms == 0 || self.handoff.ttl_ms > MAX_HANDOFF_TTL_MS {
            return Err(Error::Config(
                "handoff.ttl_ms is outside the allowed range".into(),
            ));
        }
        let minimum_post_timeout = self.minimum_post_tool_use_timeout_ms()?;
        if self.execution.post_tool_use_timeout_ms < minimum_post_timeout {
            return Err(Error::Config(format!(
                "execution.post_tool_use_timeout_ms must be at least {minimum_post_timeout} ms"
            )));
        }
        Ok(())
    }

    pub fn minimum_post_tool_use_timeout_ms(&self) -> Result<u64> {
        self.execution
            .timeout_ms
            .checked_add(self.execution.termination_grace_ms)
            .and_then(|value| value.checked_add(self.execution.forced_cleanup_margin_ms))
            .and_then(|value| value.checked_add(self.execution.result_serialization_margin_ms))
            .ok_or_else(|| Error::Config("PostToolUse timeout arithmetic overflowed".into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionConfig {
    pub timeout_ms: u64,
    pub termination_grace_ms: u64,
    pub forced_cleanup_margin_ms: u64,
    pub result_serialization_margin_ms: u64,
    pub post_tool_use_timeout_ms: u64,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        let timeout_ms = 24 * 60 * 60 * 1_000;
        let termination_grace_ms = 5_000;
        let forced_cleanup_margin_ms = 2_000;
        let result_serialization_margin_ms = 1_000;
        Self {
            timeout_ms,
            termination_grace_ms,
            forced_cleanup_margin_ms,
            result_serialization_margin_ms,
            post_tool_use_timeout_ms: timeout_ms
                + termination_grace_ms
                + forced_cleanup_margin_ms
                + result_serialization_margin_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HandoffConfig {
    pub ttl_ms: u64,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        Self {
            ttl_ms: DEFAULT_HANDOFF_TTL_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub model_max_bytes: usize,
    pub tail_bytes: usize,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            model_max_bytes: 32 * 1024,
            tail_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiagnosticsConfig {
    pub log_level: String,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
        }
    }
}
