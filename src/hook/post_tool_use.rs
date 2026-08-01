use time::OffsetDateTime;

use crate::{
    config::Config,
    error::{Error, Result},
    hook::{
        input::PostToolUseInput,
        output::{PostToolUseOutput, bounded_result_context},
    },
    paths::AppPaths,
    protocol::{DeliveryState, sha256_hex},
    receipt::{ReceiptExpectation, ReceiptSigner},
    runner::Runner,
    store::Store,
    supervisor,
    worker::run_worker_with_runner,
};

const ACTIVE_HOOK_LEASE_GRACE_MS: i64 = 5 * 60 * 1_000;
const RECEIPT_HANDLE_PREFIX: &str = "LONGRUN_RECEIPT_HANDLE_V1 ";

pub async fn handle_post_tool_use(
    input: &PostToolUseInput,
    paths: &AppPaths,
    config: &Config,
    runner: &Runner,
) -> Result<Option<PostToolUseOutput>> {
    if input.common.hook_event_name != "PostToolUse" || input.tool_name != "Bash" {
        return Ok(None);
    }
    let Some(line) = receipt_line(&input.tool_response)? else {
        return Ok(None);
    };
    let database = paths.state_dir.join("longrun.sqlite");
    let mut store = Store::open(&database)?;
    let now = OffsetDateTime::now_utc();
    let now_ms = now.unix_timestamp_nanos().div_euclid(1_000_000) as i64;
    store.cleanup_expired_pending(now_ms)?;
    let pending = match store.pending(&input.tool_use_id) {
        Ok(pending) => pending,
        Err(_) => return Ok(None),
    };
    let token = line
        .strip_prefix(RECEIPT_HANDLE_PREFIX)
        .ok_or_else(|| Error::InvalidInput("missing Longrun receipt handle prefix".into()))?;
    if token.is_empty() || sha256_hex(token.as_bytes()) != pending.hook_token_hash {
        return Err(Error::Denied(
            "receipt handle does not match pending submission".into(),
        ));
    }
    let signer = ReceiptSigner::load_or_create(&paths.state_dir.join("receipt.key"))?;
    let receipt = signer.parse(
        pending
            .signed_receipt
            .as_deref()
            .ok_or_else(|| Error::Denied("pending submission has no signed receipt".into()))?,
    )?;
    let expected = ReceiptExpectation {
        session_id: input.common.session_id.clone(),
        turn_id: input.turn_id.clone(),
        tool_use_id: input.tool_use_id.clone(),
        cwd: crate::protocol::NativeString::from_os_string(
            std::fs::canonicalize(&input.common.cwd)?.into_os_string(),
        ),
        command_hash: pending.command_hash.clone(),
    };
    let payload = receipt.verify(&signer, &expected, now)?;
    let job = payload.to_job_specification()?;
    if !config.permits_permission_profile(&job.permission_profile) {
        return Err(Error::Denied(
            "danger-full-access requires explicit configuration".into(),
        ));
    }
    store.consume_pending_and_create_job(&input.tool_use_id, &payload.nonce, &job, now_ms)?;
    let lease = store.claim_delivery(
        job.job_id,
        &input.common.session_id,
        DeliveryState::HookLeased,
        "post-tool-use",
        now_ms,
        active_hook_lease_ms(job.timeout_ms)?,
        config.recovery.retry_budget,
    )?;
    drop(store);
    let result = if job.execution_mode == crate::protocol::ExecutionMode::Durable {
        supervisor::start_existing(paths, job.job_id).await?;
        supervisor::wait(paths, job.job_id)
            .await?
            .result
            .ok_or_else(|| Error::Unavailable("durable worker completed without a result".into()))?
    } else {
        run_worker_with_runner(job.job_id, &database, config, paths, runner).await?
    };
    let delivered_at_ms = OffsetDateTime::now_utc()
        .unix_timestamp_nanos()
        .div_euclid(1_000_000) as i64;
    Store::open(&database)?.finish_delivery(
        job.job_id,
        lease.lease_id,
        DeliveryState::DeliveredInTurn,
        delivered_at_ms,
    )?;
    Ok(Some(PostToolUseOutput::completed(bounded_result_context(
        &result,
        config.output.model_max_bytes,
    ))))
}

fn active_hook_lease_ms(timeout_ms: u64) -> Result<i64> {
    let timeout_ms: i64 = timeout_ms
        .try_into()
        .map_err(|_| Error::InvalidInput("job timeout exceeds delivery lease range".into()))?;
    Ok(timeout_ms.saturating_add(ACTIVE_HOOK_LEASE_GRACE_MS))
}

fn receipt_line(response: &serde_json::Value) -> Result<Option<&str>> {
    let response = match response {
        serde_json::Value::String(response) => response,
        serde_json::Value::Object(response) => match response.get("output") {
            Some(serde_json::Value::String(response)) => response,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    let mut matches = response
        .lines()
        .filter(|line| line.starts_with(RECEIPT_HANDLE_PREFIX));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(Error::InvalidInput(
            "multiple Longrun receipts in tool response".into(),
        ));
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::receipt_line;

    #[test]
    fn receipt_line_accepts_only_documented_text_shapes() {
        assert_eq!(
            receipt_line(&json!("LONGRUN_RECEIPT_HANDLE_V1 one")).expect("string"),
            Some("LONGRUN_RECEIPT_HANDLE_V1 one")
        );
        assert_eq!(
            receipt_line(&json!({"output": "LONGRUN_RECEIPT_HANDLE_V1 two"})).expect("output"),
            Some("LONGRUN_RECEIPT_HANDLE_V1 two")
        );
        assert!(
            receipt_line(&json!({"stdout": "LONGRUN_RECEIPT_HANDLE_V1 three"}))
                .expect("other field")
                .is_none()
        );
        assert!(
            receipt_line(&json!({"output": ["LONGRUN_RECEIPT_HANDLE_V1 four"]}))
                .expect("non-text output")
                .is_none()
        );
        assert!(
            receipt_line(&json!(
                "LONGRUN_RECEIPT_HANDLE_V1 one\nLONGRUN_RECEIPT_HANDLE_V1 two"
            ))
            .is_err()
        );
    }
}
