use std::{ffi::OsString, path::PathBuf, process::Stdio, sync::Arc};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::Command,
    sync::{Semaphore, watch},
};

use crate::{
    config::Config,
    error::{Error, Result},
    ipc::{read_frame, validate_protocol_version, write_frame},
    paths::AppPaths,
    protocol::{
        ExecutionMode, ExecutionState, IpcError, IpcMethod, IpcRequest, IpcResponse,
        JobSpecification, PROTOCOL_VERSION,
    },
    store::Store,
};

#[derive(Clone)]
pub struct Supervisor {
    paths: AppPaths,
    executable: PathBuf,
    config_path: PathBuf,
    worker_path: OsString,
    permits: Arc<Semaphore>,
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
        })
    }

    pub async fn run(&self) -> Result<()> {
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let server = self.serve_until(shutdown_receiver);
        tokio::pin!(server);
        tokio::select! {
            result = &mut server => result,
            result = tokio::signal::ctrl_c() => {
                result.map_err(Error::from)?;
                let _ = shutdown_sender.send(true);
                server.await
            }
        }
    }

    pub async fn serve_until(&self, shutdown: watch::Receiver<bool>) -> Result<()> {
        self.resume_accepted_jobs()?;
        #[cfg(unix)]
        {
            self.serve_unix(shutdown).await
        }
        #[cfg(windows)]
        {
            self.serve_windows(shutdown).await
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = shutdown;
            Err(Error::Unavailable(
                "Longrun supervisor is unsupported on this platform".into(),
            ))
        }
    }

    fn resume_accepted_jobs(&self) -> Result<()> {
        for job in
            Store::open(self.paths.state_dir.join("longrun.sqlite"))?.accepted_durable_jobs()?
        {
            self.spawn_worker(job.job_id);
        }
        Ok(())
    }

    fn spawn_worker(&self, job_id: uuid::Uuid) {
        let executable = self.executable.clone();
        let config_path = self.config_path.clone();
        let state_dir = self.paths.state_dir.clone();
        let log_dir = self.paths.log_dir.clone();
        let worker_path = self.worker_path.clone();
        let permits = self.permits.clone();
        tokio::spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return;
            };
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
        });
    }

    #[cfg(unix)]
    async fn serve_unix(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let listener = crate::ipc::unix::bind(&self.paths.socket_path).await?;
        let result = loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break Ok(());
                    }
                }
                connection = listener.accept() => {
                    let (stream, _) = connection?;
                    let supervisor = self.clone();
                    tokio::spawn(async move {
                        let _ = supervisor.handle_connection(stream).await;
                    });
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
                    let supervisor = self.clone();
                    tokio::spawn(async move {
                        let _ = supervisor.handle_connection(stream).await;
                    });
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
            Ok(result) => response_ok(request.request_id, result),
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
