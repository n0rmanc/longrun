use std::time::Duration;

use tokio::{
    sync::watch,
    time::{MissedTickBehavior, interval},
};
use uuid::Uuid;

use crate::{
    config::Config, error::Result, paths::AppPaths, protocol::JobResult, runner::Runner,
    store::Store,
};

pub async fn run_worker(
    job_id: Uuid,
    database: &std::path::Path,
    config: &Config,
    paths: &AppPaths,
) -> Result<JobResult> {
    run_worker_with_runner(job_id, database, config, paths, &Runner::new()).await
}

pub async fn run_worker_with_runner(
    job_id: Uuid,
    database: &std::path::Path,
    config: &Config,
    paths: &AppPaths,
    runner: &Runner,
) -> Result<JobResult> {
    let mut store = Store::open(database)?;
    let claim = Uuid::now_v7().to_string();
    let job = store.claim_execution(job_id, &claim)?;
    store.mark_running(job_id, &claim)?;
    let (stopped, mut stop_receiver) = watch::channel(false);
    let heartbeat_database = database.to_path_buf();
    let heartbeat_claim = claim.clone();
    let heartbeat = tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let now_ms = time::OffsetDateTime::now_utc()
                        .unix_timestamp_nanos()
                        .div_euclid(1_000_000) as i64;
                    if Store::open(&heartbeat_database)
                        .and_then(|mut store| store.touch_execution(job_id, &heartbeat_claim, now_ms))
                        .is_err()
                    {
                        return;
                    }
                }
                changed = stop_receiver.changed() => {
                    if changed.is_err() || *stop_receiver.borrow() {
                        return;
                    }
                }
            }
        }
    });
    let execution = runner
        .execute_with_cancellation(&job, config, paths, Some(database))
        .await;
    let _ = stopped.send(true);
    let _ = heartbeat.await;
    let result = execution?;
    store.finish_execution(&result, &claim)?;
    Ok(result)
}
