use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct BashInput {
    pub command: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreToolUseInput {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub cwd: PathBuf,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: BashInput,
}
