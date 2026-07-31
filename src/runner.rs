use std::{ffi::OsString, path::PathBuf, process::Stdio};

use base64::Engine;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::{Duration, Instant, sleep},
};

use crate::{
    config::Config,
    error::{Error, Result},
    output::byte_tail,
    paths::AppPaths,
    platform,
    protocol::{ExecutionState, JobResult, JobSpecification, NativeString, ShellMode},
    store::Store,
};

pub struct Runner {
    sandbox_binary: OsString,
}

impl Runner {
    pub fn new() -> Self {
        Self {
            sandbox_binary: "codex".into(),
        }
    }

    pub fn with_sandbox_binary(binary: PathBuf) -> Self {
        Self {
            sandbox_binary: binary.into_os_string(),
        }
    }

    pub async fn execute(
        &self,
        job: &JobSpecification,
        config: &Config,
        paths: &AppPaths,
    ) -> Result<JobResult> {
        self.execute_with_cancellation(job, config, paths, None)
            .await
    }

    pub async fn execute_with_cancellation(
        &self,
        job: &JobSpecification,
        config: &Config,
        paths: &AppPaths,
        cancellation_database: Option<&std::path::Path>,
    ) -> Result<JobResult> {
        if !config.permits_permission_profile(&job.permission_profile) {
            return Err(Error::Denied(
                "danger-full-access requires explicit configuration".into(),
            ));
        }
        let cwd = job.cwd.to_os_string()?;
        let program = job.program.to_os_string()?;
        let args = job
            .args
            .iter()
            .map(NativeString::to_os_string)
            .collect::<Result<Vec<_>>>()?;
        let (program, args) = command_for(job.shell_mode, program, args)?;
        let stdout_path = paths.log_dir.join(format!("{}.stdout.log", job.job_id));
        let stderr_path = paths.log_dir.join(format!("{}.stderr.log", job.job_id));
        let started = Instant::now();
        let mut command = Command::new(&self.sandbox_binary);
        command
            .arg("sandbox")
            .arg("-P")
            .arg(&job.permission_profile)
            .arg("-C")
            .arg(&cwd)
            .arg("--")
            .arg(program)
            .args(args)
            .current_dir(&cwd)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        copy_safe_environment(&mut command, job);
        platform::configure_command(&mut command)?;
        let mut child = command.spawn()?;
        let process_tree = match platform::track_child(&child) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(error);
            }
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Unavailable("sandbox stdout was not captured".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::Unavailable("sandbox stderr was not captured".into()))?;
        let stdout_task = tokio::spawn(copy_stream(stdout, stdout_path.clone()));
        let stderr_task = tokio::spawn(copy_stream(stderr, stderr_path.clone()));
        let timeout = sleep(Duration::from_millis(job.timeout_ms));
        tokio::pin!(timeout);
        let (state, exit_code) = loop {
            tokio::select! {
                status = child.wait() => {
                    let status = status?;
                    break (if status.success() { ExecutionState::Succeeded } else { ExecutionState::Failed }, status.code());
                }
                _ = &mut timeout => {
                    let _ = platform::terminate(&mut child, &process_tree, config.execution.termination_grace_ms).await?;
                    break (ExecutionState::TimedOut, None);
                }
                _ = sleep(Duration::from_millis(50)), if cancellation_database.is_some() => {
                    let database = cancellation_database.expect("cancellation database is checked");
                    if let Some(grace_ms) = Store::open(database)?.cancellation_grace(job.job_id)? {
                        let _ = platform::terminate(&mut child, &process_tree, grace_ms).await?;
                        break (ExecutionState::Cancelled, None);
                    }
                }
            }
        };
        stdout_task
            .await
            .map_err(|error| Error::Unavailable(format!("stdout task failed: {error}")))??;
        stderr_task
            .await
            .map_err(|error| Error::Unavailable(format!("stderr task failed: {error}")))??;
        let stdout = tokio::fs::read(&stdout_path).await?;
        let stderr = tokio::fs::read(&stderr_path).await?;
        let stdout_tail = byte_tail(&stdout, config.output.tail_bytes);
        let stderr_tail = byte_tail(&stderr, config.output.tail_bytes);
        let finished = time::OffsetDateTime::now_utc()
            .unix_timestamp_nanos()
            .div_euclid(1_000_000) as i64;
        Ok(JobResult {
            job_id: job.job_id,
            terminal_state: state,
            exit_code,
            signal: None,
            duration_ms: started.elapsed().as_millis() as u64,
            stdout_log: NativeString::from_os_string(stdout_path.into_os_string()),
            stderr_log: NativeString::from_os_string(stderr_path.into_os_string()),
            stdout_tail: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(stdout_tail.bytes),
            stderr_tail: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(stderr_tail.bytes),
            stdout_truncated: stdout_tail.truncated,
            stderr_truncated: stderr_tail.truncated,
            result_hash: format!("{}:{}", stdout_tail.sha256, stderr_tail.sha256),
            completed_at_ms: finished,
        })
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

fn command_for(
    shell_mode: ShellMode,
    program: OsString,
    args: Vec<OsString>,
) -> Result<(OsString, Vec<OsString>)> {
    match shell_mode {
        ShellMode::Direct => Ok((program, args)),
        ShellMode::ExplicitShell => {
            let script = args
                .first()
                .cloned()
                .ok_or_else(|| Error::InvalidInput("shell command is missing its script".into()))?;
            #[cfg(unix)]
            {
                Ok(("/bin/sh".into(), vec!["-c".into(), script]))
            }
            #[cfg(windows)]
            {
                Ok(("cmd.exe".into(), vec!["/C".into(), script]))
            }
            #[cfg(not(any(unix, windows)))]
            {
                Err(Error::Unavailable(
                    "shell execution is unsupported on this platform".into(),
                ))
            }
        }
    }
}

fn copy_safe_environment(command: &mut Command, job: &JobSpecification) {
    for name in ["PATH", "HOME", "TMPDIR", "SYSTEMROOT", "WINDIR", "COMSPEC"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for name in &job.environment_policy.pass {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

async fn copy_stream<R>(mut reader: R, path: PathBuf) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut file = tokio::fs::File::create(path).await?;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).await?;
    }
    file.sync_all().await?;
    Ok(())
}
