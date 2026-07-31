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
        ExecutionMode, IpcError, IpcMethod, IpcRequest, IpcResponse, JobSpecification,
        PROTOCOL_VERSION,
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
        let response = match self.handle_request(&request) {
            Ok(result) => response_ok(request.request_id, result),
            Err(error) => response_error(request.request_id, &error),
        };
        write_frame(&mut stream, &response).await
    }

    fn handle_request(&self, request: &IpcRequest) -> Result<serde_json::Value> {
        validate_protocol_version(request.protocol_version)?;
        match request.method {
            IpcMethod::Health => Ok(serde_json::json!({
                "healthy": true,
                "protocol_version": PROTOCOL_VERSION,
            })),
            IpcMethod::Submit => {
                let job: JobSpecification = serde_json::from_value(request.params.clone())?;
                if job.execution_mode != ExecutionMode::Durable {
                    return Err(Error::InvalidInput(
                        "supervisor accepts durable jobs only".into(),
                    ));
                }
                Store::open(self.paths.state_dir.join("longrun.sqlite"))?.create_job(&job)?;
                self.spawn_worker(job.job_id);
                Ok(serde_json::json!({ "job_id": job.job_id }))
            }
            _ => Err(Error::Unavailable(
                "supervisor method is not initialized".into(),
            )),
        }
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
