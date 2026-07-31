use std::{
    collections::HashSet,
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::Command,
    sync::{Semaphore, watch},
    time::{Duration, Instant, MissedTickBehavior, interval, sleep},
};

use crate::{
    config::Config,
    error::{Error, Result},
    ipc::{read_frame, validate_protocol_version, write_frame},
    paths::AppPaths,
    protocol::{
        ExecutionMode, ExecutionState, IpcError, IpcEvent, IpcEventKind, IpcMethod, IpcRequest,
        IpcResponse, JobResult, JobSpecification, NativeString, PROTOCOL_VERSION,
    },
    store::Store,
};

const WORKER_HEARTBEAT_STALE_MS: i64 = 5_000;

#[derive(Clone)]
pub struct Supervisor {
    paths: AppPaths,
    executable: PathBuf,
    config_path: PathBuf,
    worker_path: OsString,
    permits: Arc<Semaphore>,
    active_jobs: Arc<Mutex<HashSet<uuid::Uuid>>>,
    connections: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    stopping: Arc<AtomicBool>,
    termination_grace_ms: u64,
}

impl Supervisor {
    pub fn new(
        paths: AppPaths,
        config: &Config,
        executable: PathBuf,
        config_path: PathBuf,
        worker_path: OsString,
    ) -> Result<Self> {
        if !executable.is_absolute() {
            return Err(Error::Unavailable(
                "Longrun supervisor executable path must be absolute".into(),
            ));
        }
        Ok(Self {
            permits: Arc::new(Semaphore::new(config.execution.concurrency)),
            paths,
            executable,
            config_path,
            worker_path,
            active_jobs: Arc::new(Mutex::new(HashSet::new())),
            connections: Arc::new(Mutex::new(Vec::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            termination_grace_ms: config.execution.termination_grace_ms,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let server = self.serve_until(shutdown_receiver);
        let shutdown = crate::platform::wait_for_shutdown();
        tokio::pin!(server);
        tokio::pin!(shutdown);
        tokio::select! {
            result = &mut server => result,
            result = &mut shutdown => {
                result?;
                let _ = shutdown_sender.send(true);
                server.await
            }
        }
    }

    pub async fn serve_until(&self, shutdown: watch::Receiver<bool>) -> Result<()> {
        self.reconcile_incomplete_workers()?;
        self.resume_accepted_jobs()?;
        #[cfg(unix)]
        let result = { self.serve_unix(shutdown).await };
        #[cfg(windows)]
        let result = { self.serve_windows(shutdown).await };
        #[cfg(not(any(unix, windows)))]
        let result = {
            let _ = shutdown;
            Err(Error::Unavailable(
                "Longrun supervisor is unsupported on this platform".into(),
            ))
        };
        self.shutdown_workers().await?;
        self.drain_connections().await;
        result
    }

    fn resume_accepted_jobs(&self) -> Result<()> {
        for job in
            Store::open(self.paths.state_dir.join("longrun.sqlite"))?.accepted_durable_jobs()?
        {
            self.spawn_worker(job.job_id);
        }
        Ok(())
    }

    fn reconcile_incomplete_workers(&self) -> Result<()> {
        let database = self.paths.state_dir.join("longrun.sqlite");
        let now = now_ms()?;
        let stale_before = now.saturating_sub(WORKER_HEARTBEAT_STALE_MS);
        let mut store = Store::open(&database)?;
        for job_id in store.incomplete_durable_job_ids()? {
            if store.fail_stale_execution(
                &persistence_gap_result(job_id, &self.paths, now),
                stale_before,
            )? {
                continue;
            }
            if !store.execution_state(job_id)?.is_terminal() {
                self.active_jobs
                    .lock()
                    .expect("supervisor active-job lock is poisoned")
                    .insert(job_id);
            }
        }
        self.active_jobs
            .lock()
            .expect("supervisor active-job lock is poisoned")
            .retain(|job_id| {
                store
                    .execution_state(*job_id)
                    .is_ok_and(|state| !state.is_terminal())
            });
        Ok(())
    }

    fn spawn_worker(&self, job_id: uuid::Uuid) {
        if self.stopping.load(Ordering::Acquire)
            || !self
                .active_jobs
                .lock()
                .expect("supervisor active-job lock is poisoned")
                .insert(job_id)
        {
            return;
        }
        let executable = self.executable.clone();
        let config_path = self.config_path.clone();
        let state_dir = self.paths.state_dir.clone();
        let log_dir = self.paths.log_dir.clone();
        let worker_path = self.worker_path.clone();
        let permits = self.permits.clone();
        let active_jobs = self.active_jobs.clone();
        let stopping = self.stopping.clone();
        tokio::spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                active_jobs
                    .lock()
                    .expect("supervisor active-job lock is poisoned")
                    .remove(&job_id);
                return;
            };
            if stopping.load(Ordering::Acquire) {
                active_jobs
                    .lock()
                    .expect("supervisor active-job lock is poisoned")
                    .remove(&job_id);
                return;
            }
            let mut worker = Command::new(executable);
            worker
                .arg("--config")
                .arg(config_path)
                .arg("internal")
                .arg("worker")
                .arg(job_id.to_string())
                .arg("--state-dir")
                .arg(state_dir)
                .arg("--log-dir")
                .arg(log_dir)
                .env("PATH", worker_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Ok(mut worker) = worker.spawn() {
                let _ = worker.wait().await;
            }
            active_jobs
                .lock()
                .expect("supervisor active-job lock is poisoned")
                .remove(&job_id);
        });
    }

    async fn shutdown_workers(&self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        let job_ids = self
            .active_jobs
            .lock()
            .expect("supervisor active-job lock is poisoned")
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let database = self.paths.state_dir.join("longrun.sqlite");
        let now = now_ms()?;
        let mut cancelling = Vec::new();
        for job_id in job_ids {
            if Store::open(&database)?.request_cancellation(
                job_id,
                self.termination_grace_ms,
                now,
            )? {
                cancelling.push(job_id);
            }
        }
        if cancelling.is_empty() {
            return Ok(());
        }
        let deadline =
            Instant::now() + Duration::from_millis(self.termination_grace_ms.saturating_add(2_000));
        loop {
            let store = Store::open(&database)?;
            if cancelling.iter().all(|job_id| {
                store
                    .execution_state(*job_id)
                    .is_ok_and(ExecutionState::is_terminal)
            }) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Unavailable(
                    "durable workers did not stop within the termination grace period".into(),
                ));
            }
            sleep(Duration::from_millis(25)).await;
        }
    }

    fn spawn_connection<S>(&self, stream: S)
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let supervisor = self.clone();
        let handle = tokio::spawn(async move {
            let _ = supervisor.handle_connection(stream).await;
        });
        let mut connections = self
            .connections
            .lock()
            .expect("supervisor connection lock is poisoned");
        connections.retain(|connection| !connection.is_finished());
        connections.push(handle);
    }

    async fn drain_connections(&self) {
        let connections = std::mem::take(
            &mut *self
                .connections
                .lock()
                .expect("supervisor connection lock is poisoned"),
        );
        for connection in connections {
            let _ = connection.await;
        }
    }

    #[cfg(unix)]
    async fn serve_unix(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let listener = crate::ipc::unix::bind(&self.paths.socket_path).await?;
        let mut recovery = interval(Duration::from_secs(1));
        recovery.set_missed_tick_behavior(MissedTickBehavior::Delay);
        recovery.tick().await;
        let result = loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break Ok(());
                    }
                }
                connection = listener.accept() => {
                    let (stream, _) = connection?;
                    self.spawn_connection(stream);
                }
                _ = recovery.tick() => {
                    self.reconcile_incomplete_workers()?;
                }
            }
        };
        drop(listener);
        match std::fs::remove_file(&self.paths.socket_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        result
    }

    #[cfg(windows)]
    async fn serve_windows(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let endpoint = self.paths.ipc_endpoint().to_string_lossy().into_owned();
        let mut listener = crate::ipc::windows::first_server(&endpoint)?;
        let mut recovery = interval(Duration::from_secs(1));
        recovery.set_missed_tick_behavior(MissedTickBehavior::Delay);
        recovery.tick().await;
        loop {
            let next = crate::ipc::windows::next_server(&endpoint)?;
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                connected = listener.connect() => {
                    connected?;
                    let stream = listener;
                    listener = next;
                    self.spawn_connection(stream);
                }
                _ = recovery.tick() => {
                    self.reconcile_incomplete_workers()?;
                }
            }
        }
    }

    async fn handle_connection<S>(&self, mut stream: S) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let request: IpcRequest = read_frame(&mut stream).await?;
        let response = match self.handle_request(&request).await {
            Ok(result) => {
                if request.method == IpcMethod::Wait {
                    write_frame(
                        &mut stream,
                        &IpcEvent {
                            protocol_version: PROTOCOL_VERSION,
                            job_id: job_id(&request.params)?,
                            event: IpcEventKind::Completed,
                            payload: result.clone(),
                        },
                    )
                    .await?;
                }
                response_ok(request.request_id, result)
            }
            Err(error) => response_error(request.request_id, &error),
        };
        write_frame(&mut stream, &response).await
    }

    async fn handle_request(&self, request: &IpcRequest) -> Result<serde_json::Value> {
        validate_protocol_version(request.protocol_version)?;
        match request.method {
            IpcMethod::Health => Ok(serde_json::json!({
                "healthy": true,
                "protocol_version": PROTOCOL_VERSION,
            })),
            IpcMethod::Submit => self.submit(&request.params),
            IpcMethod::Wait => {
                let job_id = job_id(&request.params)?;
                loop {
                    let status =
                        Store::open(self.paths.state_dir.join("longrun.sqlite"))?.status(job_id)?;
                    if status.execution_state.is_terminal() {
                        return Ok(serde_json::to_value(status)?);
                    }
                    if self.stopping.load(Ordering::Acquire)
                        && status.execution_state == ExecutionState::Accepted
                    {
                        return Err(Error::Unavailable(
                            "supervisor stopped before the durable job started".into(),
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
            IpcMethod::Status => {
                let status = Store::open(self.paths.state_dir.join("longrun.sqlite"))?
                    .status(job_id(&request.params)?)?;
                Ok(serde_json::to_value(status)?)
            }
            IpcMethod::Cancel => {
                let job_id = job_id(&request.params)?;
                let grace_ms = request
                    .params
                    .get("grace_ms")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| Error::InvalidInput("missing cancellation grace_ms".into()))?;
                let requested = Store::open(self.paths.state_dir.join("longrun.sqlite"))?
                    .request_cancellation(job_id, grace_ms, now_ms()?)?;
                Ok(serde_json::json!({ "cancellation_requested": requested }))
            }
            _ => Err(Error::Unavailable(
                "supervisor method is not initialized".into(),
            )),
        }
    }

    fn submit(&self, params: &serde_json::Value) -> Result<serde_json::Value> {
        if params.get("job").is_none()
            && params.get("protocol_version").is_none()
            && let Some(job_id) = params.get("job_id")
        {
            let job_id = serde_json::from_value(job_id.clone())?;
            let store = Store::open(self.paths.state_dir.join("longrun.sqlite"))?;
            let job = store.job(job_id)?;
            if job.execution_mode != ExecutionMode::Durable {
                return Err(Error::InvalidInput(
                    "supervisor accepts durable jobs only".into(),
                ));
            }
            if store.execution_state(job_id)? == ExecutionState::Accepted {
                self.spawn_worker(job_id);
            }
            return Ok(serde_json::json!({ "job_id": job_id }));
        }
        let job: JobSpecification = match params.get("job") {
            Some(job) => serde_json::from_value(job.clone())?,
            None => serde_json::from_value(params.clone())?,
        };
        if job.execution_mode != ExecutionMode::Durable {
            return Err(Error::InvalidInput(
                "supervisor accepts durable jobs only".into(),
            ));
        }
        Store::open(self.paths.state_dir.join("longrun.sqlite"))?.create_job(&job)?;
        self.spawn_worker(job.job_id);
        Ok(serde_json::json!({ "job_id": job.job_id }))
    }
}

pub async fn request(
    paths: &AppPaths,
    method: IpcMethod,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let request = IpcRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: uuid::Uuid::now_v7(),
        method,
        params,
    };
    #[cfg(unix)]
    let response = crate::ipc::unix::request(&paths.socket_path, &request).await?;
    #[cfg(windows)]
    let response =
        crate::ipc::windows::request(&paths.ipc_endpoint().to_string_lossy(), &request).await?;
    #[cfg(not(any(unix, windows)))]
    let response = {
        let _ = request;
        return Err(Error::Unavailable(
            "Longrun supervisor is unsupported on this platform".into(),
        ));
    };
    if response.ok {
        response
            .result
            .ok_or_else(|| Error::Unavailable("supervisor returned no result".into()))
    } else {
        Err(Error::Unavailable(response.error.map_or_else(
            || "supervisor rejected the request".into(),
            |error| error.message,
        )))
    }
}

pub async fn submit(paths: &AppPaths, job: &JobSpecification) -> Result<()> {
    request(paths, IpcMethod::Submit, serde_json::json!({ "job": job }))
        .await
        .map(|_| ())
}

pub async fn start_existing(paths: &AppPaths, job_id: uuid::Uuid) -> Result<()> {
    request(
        paths,
        IpcMethod::Submit,
        serde_json::json!({ "job_id": job_id }),
    )
    .await
    .map(|_| ())
}

pub async fn wait(paths: &AppPaths, job_id: uuid::Uuid) -> Result<crate::store::JobStatus> {
    serde_json::from_value(request(paths, IpcMethod::Wait, job_params(job_id)).await?)
        .map_err(Error::from)
}

fn job_id(params: &serde_json::Value) -> Result<uuid::Uuid> {
    params
        .get("job_id")
        .cloned()
        .ok_or_else(|| Error::InvalidInput("missing job_id".into()))
        .and_then(|job_id| serde_json::from_value(job_id).map_err(Error::from))
}

fn job_params(job_id: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({ "job_id": job_id })
}

fn now_ms() -> Result<i64> {
    time::OffsetDateTime::now_utc()
        .unix_timestamp_nanos()
        .div_euclid(1_000_000)
        .try_into()
        .map_err(|_| Error::Unavailable("system clock is out of range".into()))
}

fn persistence_gap_result(job_id: uuid::Uuid, paths: &AppPaths, completed_at_ms: i64) -> JobResult {
    JobResult {
        job_id,
        terminal_state: ExecutionState::Failed,
        exit_code: None,
        signal: None,
        duration_ms: 0,
        stdout_log: NativeString::from_os_string(
            paths
                .log_dir
                .join(format!("{job_id}.stdout.log"))
                .into_os_string(),
        ),
        stderr_log: NativeString::from_os_string(
            paths
                .log_dir
                .join(format!("{job_id}.stderr.log"))
                .into_os_string(),
        ),
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        result_hash: "sha256:worker-persistence-gap".into(),
        completed_at_ms,
    }
}

fn response_ok(request_id: uuid::Uuid, result: serde_json::Value) -> IpcResponse {
    IpcResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        result: Some(result),
        error: None,
    }
}

fn response_error(request_id: uuid::Uuid, error: &Error) -> IpcResponse {
    let code = match error {
        Error::InvalidInput(_) => "invalid_input",
        Error::Config(_) => "config",
        Error::Denied(_) => "denied",
        Error::NotFound(_) => "not_found",
        Error::Unavailable(_) => "unavailable",
        Error::Io(_) | Error::Sqlite(_) | Error::Json(_) => "internal",
    };
    IpcResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: false,
        result: None,
        error: Some(IpcError {
            code: code.into(),
            message: error.to_string(),
        }),
    }
}
