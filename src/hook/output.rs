use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedInput {
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreHookSpecificOutput {
    pub hook_event_name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<UpdatedInput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreToolUseOutput {
    pub hook_specific_output: PreHookSpecificOutput,
}

impl PreToolUseOutput {
    pub fn allow(command: String) -> Self {
        Self {
            hook_specific_output: PreHookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: Some("allow".into()),
                permission_decision_reason: None,
                updated_input: Some(UpdatedInput { command }),
            },
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            hook_specific_output: PreHookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: Some("deny".into()),
                permission_decision_reason: Some(reason.into()),
                updated_input: None,
            },
        }
    }
}
