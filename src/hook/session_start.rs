use std::path::Path;

use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    hook::{
        input::SessionStartInput,
        output::{SessionStartOutput, bounded_result_context},
    },
    paths::AppPaths,
    protocol::DeliveryState,
    store::Store,
};

const SESSION_START_LEASE_MS: i64 = 5 * 60 * 1_000;

pub struct SessionStartDelivery {
    pub output: SessionStartOutput,
    pub job_id: Uuid,
    pub lease_id: Uuid,
}

pub fn handle_session_start(
    input: &SessionStartInput,
    executable: &Path,
    paths: &AppPaths,
    config: &Config,
    now_ms: i64,
) -> Result<Option<SessionStartDelivery>> {
    if input.common.hook_event_name != "SessionStart" {
        return Ok(None);
    }
    let executable = executable
        .to_str()
        .filter(|_| executable.is_absolute())
        .ok_or_else(|| {
            Error::Unavailable("Longrun executable path must be absolute UTF-8".into())
        })?;
    let database = paths.state_dir.join("longrun.sqlite");
    let mut store = Store::open(&database)?;
    store.expire_delivery_leases(now_ms)?;
    let Some(result) = store.undelivered_result_for_session(&input.common.session_id)? else {
        return Ok(None);
    };
    let lease = match store.claim_delivery(
        result.job_id,
        &input.common.session_id,
        DeliveryState::SessionStartLeased,
        "session-start",
        now_ms,
        SESSION_START_LEASE_MS,
        config.recovery.retry_budget,
    ) {
        Ok(lease) => lease,
        Err(Error::Denied(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let context = format!(
        "Longrun is active at {executable}. Use `{executable} submit -- PROGRAM ARG...` for finite commands expected to run longer than two minutes.\n\nRecovered result (delivery idempotency key: {}):\n{}",
        lease.idempotency_key,
        bounded_result_context(&result, config.output.model_max_bytes),
    );
    Ok(Some(SessionStartDelivery {
        output: SessionStartOutput::context(context),
        job_id: result.job_id,
        lease_id: lease.lease_id,
    }))
}
