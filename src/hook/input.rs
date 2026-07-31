use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct CodexCommonInput {
    pub session_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
    pub transcript_path: Option<PathBuf>,
    pub cwd: PathBuf,
    pub hook_event_name: String,
    pub model: String,
    pub permission_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreToolUseInput {
    #[serde(flatten)]
    pub common: CodexCommonInput,
    pub turn_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostToolUseInput {
    #[serde(flatten)]
    pub common: CodexCommonInput,
    pub turn_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: Value,
    pub tool_response: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionStartInput {
    #[serde(flatten)]
    pub common: CodexCommonInput,
    pub source: String,
}

impl PreToolUseInput {
    pub fn bash_command(&self) -> Option<&str> {
        self.tool_input.get("command")?.as_str()
    }
}
