use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedInput {
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookUniversalOutput {
    #[serde(rename = "continue", skip_serializing_if = "is_true")]
    pub continue_processing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub suppress_output: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
}

impl Default for HookUniversalOutput {
    fn default() -> Self {
        Self {
            continue_processing: true,
            stop_reason: None,
            suppress_output: false,
            system_message: None,
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreToolUseOutput {
    #[serde(flatten)]
    pub universal: HookUniversalOutput,
    pub hook_specific_output: PreHookSpecificOutput,
}

impl PreToolUseOutput {
    pub fn allow(command: String) -> Self {
        Self {
            universal: HookUniversalOutput::default(),
            hook_specific_output: PreHookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: Some("allow".into()),
                permission_decision_reason: None,
                updated_input: Some(UpdatedInput { command }),
                additional_context: None,
            },
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            universal: HookUniversalOutput::default(),
            hook_specific_output: PreHookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: Some("deny".into()),
                permission_decision_reason: Some(reason.into()),
                updated_input: None,
                additional_context: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostHookSpecificOutput {
    pub hook_event_name: &'static str,
    pub additional_context: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostToolUseOutput {
    #[serde(flatten)]
    pub universal: HookUniversalOutput,
    pub hook_specific_output: PostHookSpecificOutput,
}

impl PostToolUseOutput {
    pub fn completed(additional_context: String) -> Self {
        Self {
            universal: HookUniversalOutput {
                continue_processing: false,
                system_message: Some("Longrun completed the submitted command.".into()),
                ..HookUniversalOutput::default()
            },
            hook_specific_output: PostHookSpecificOutput {
                hook_event_name: "PostToolUse",
                additional_context,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartHookSpecificOutput {
    pub hook_event_name: &'static str,
    pub additional_context: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartOutput {
    #[serde(flatten)]
    pub universal: HookUniversalOutput,
    pub hook_specific_output: SessionStartHookSpecificOutput,
}

impl SessionStartOutput {
    pub fn context(additional_context: String) -> Self {
        Self {
            universal: HookUniversalOutput::default(),
            hook_specific_output: SessionStartHookSpecificOutput {
                hook_event_name: "SessionStart",
                additional_context,
            },
        }
    }
}

const fn is_true(value: &bool) -> bool {
    *value
}

const fn is_false(value: &bool) -> bool {
    !*value
}
