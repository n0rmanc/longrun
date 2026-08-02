use std::fs;

use crate::{
    config::Config,
    error::Result,
    handoff::{HandoffExpectation, HandoffStore, Receipt},
    hook::{
        input::PostToolUseInput,
        output::{PostToolUseOutput, bounded_result_context},
    },
    metrics,
    paths::AppPaths,
    protocol::NativeString,
    runner::{ExecutionMode, OutputMode, Runner},
};

pub async fn handle_post_tool_use(
    input: &PostToolUseInput,
    paths: &AppPaths,
    config: &Config,
    runner: &Runner,
) -> Result<Option<PostToolUseOutput>> {
    if input.common.hook_event_name != "PostToolUse" || input.tool_name != "Bash" {
        return Ok(None);
    }
    let Some(id) = receipt_id(&input.tool_response) else {
        return Ok(None);
    };
    let cwd = NativeString::from_os_string(fs::canonicalize(&input.common.cwd)?.into_os_string());
    let binary = NativeString::from_os_string(std::env::current_exe()?.into_os_string());
    let store = HandoffStore::new(paths);
    let Some(claimed) = store.claim(
        id,
        &HandoffExpectation {
            session_id: input.common.session_id.clone(),
            turn_id: input.turn_id.clone(),
            tool_use_id: input.tool_use_id.clone(),
            cwd,
            binary_path: binary,
        },
        crate::hook::pre_tool_use::now_ms()?,
    )?
    else {
        return Ok(None);
    };
    let target = claimed.handoff.target.clone();
    let result = runner
        .execute(
            &target,
            config,
            paths,
            ExecutionMode::CodexHook,
            OutputMode::Capture,
        )
        .await;
    let result = result?;
    store.remove(&claimed)?;
    if let Err(error) = metrics::record(paths, &target, ExecutionMode::CodexHook, &result) {
        tracing::warn!(error = %error, "could not record Longrun metrics");
    }
    Ok(Some(PostToolUseOutput::completed(bounded_result_context(
        &result,
        config.output.model_max_bytes,
    ))))
}

fn receipt_id(response: &serde_json::Value) -> Option<&str> {
    let response = match response {
        serde_json::Value::String(value) => value.as_str(),
        serde_json::Value::Object(value) => value.get("output")?.as_str()?,
        _ => return None,
    };
    let mut matches = response.lines().filter_map(Receipt::parse);
    let first = matches.next();
    if matches.next().is_some() {
        return None;
    }
    first
}
