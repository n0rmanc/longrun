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
    let result = runner
        .execute_with_cancellation(&job, config, paths, Some(database))
        .await?;
    store.finish_execution(&result, &claim)?;
    Ok(result)
}
