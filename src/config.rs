use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    protocol::default_deny_patterns,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub execution: ExecutionConfig,
    pub output: OutputConfig,
    pub environment: EnvironmentConfig,
    pub recovery: RecoveryConfig,
    pub retention: RetentionConfig,
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
        if self.execution.permission_profile.trim().is_empty() {
            return Err(Error::Config(
                "execution.permission_profile must not be empty".into(),
            ));
        }
        if self.execution.concurrency == 0 {
            return Err(Error::Config(
                "execution.concurrency must be positive".into(),
            ));
        }
        if self.execution.termination_grace_ms == 0 {
            return Err(Error::Config(
                "execution.termination_grace_ms must be positive".into(),
            ));
        }
        if self.output.model_max_bytes == 0 || self.output.tail_bytes == 0 {
            return Err(Error::Config("output byte limits must be positive".into()));
        }
        if self.recovery.retry_budget == 0 {
            return Err(Error::Config(
                "recovery.retry_budget must be positive".into(),
            ));
        }
        if self.retention.max_log_bytes == 0 {
            return Err(Error::Config(
                "retention.max_log_bytes must be positive".into(),
            ));
        }
        Ok(())
    }

    pub fn permits_permission_profile(&self, profile: &str) -> bool {
        profile != ":danger-full-access" || self.execution.allow_danger_full_access
    }
}

const MAX_TIMEOUT_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionConfig {
    pub timeout_ms: u64,
    pub permission_profile: String,
    pub allow_shell: bool,
    pub allow_danger_full_access: bool,
    pub termination_grace_ms: u64,
    pub concurrency: usize,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 24 * 60 * 60 * 1_000,
            permission_profile: ":workspace".into(),
            allow_shell: false,
            allow_danger_full_access: false,
            termination_grace_ms: 5_000,
            concurrency: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct EnvironmentConfig {
    pub pass: Vec<String>,
    #[serde(default = "default_deny_patterns")]
    pub deny_patterns: Vec<String>,
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            pass: Vec::new(),
            deny_patterns: default_deny_patterns(),
        }
    }
}

impl EnvironmentConfig {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecoveryConfig {
    pub auto_resume: bool,
    pub retry_budget: u32,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            auto_resume: false,
            retry_budget: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetentionConfig {
    pub max_age_days: u32,
    pub max_log_bytes: u64,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_age_days: 30,
            max_log_bytes: 10 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
