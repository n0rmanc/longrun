use std::process::Stdio;

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::{Duration, Instant, sleep},
};

use crate::{
    config::Config,
    error::{Error, Result},
    output::RollingOutput,
    paths::AppPaths,
    platform,
    protocol::{CapturedOutput, ResultEnvelope, TargetSpec, TerminalReason},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Direct,
    CodexHook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Passthrough,
    Capture,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Runner;

impl Runner {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        target: &TargetSpec,
        config: &Config,
        _paths: &AppPaths,
        _mode: ExecutionMode,
        output_mode: OutputMode,
    ) -> Result<ResultEnvelope> {
        let cwd = target.cwd.to_os_string()?;
        let program = target.program.to_os_string()?;
        let args = target
            .args
            .iter()
            .map(|argument| argument.to_os_string())
            .collect::<Result<Vec<_>>>()?;

        let mut command = Command::new(&program);
        command
            .args(&args)
            .current_dir(&cwd)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        platform::configure_command(&mut command)?;
        let started = Instant::now();
        let mut child = command.spawn()?;
        let process_tree = match platform::track_child(&child) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                let _ = child.kill().await;
                return Err(error);
            }
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Unavailable("target stdout was not captured".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::Unavailable("target stderr was not captured".into()))?;

        let stdout_task = tokio::spawn(read_output(
            stdout,
            OutputStream::Stdout,
            output_mode,
            config.output.tail_bytes,
        ));
        let stderr_task = tokio::spawn(read_output(
            stderr,
            OutputStream::Stderr,
            output_mode,
            config.output.tail_bytes,
        ));
        let timeout = sleep(Duration::from_millis(target.timeout_ms));
        tokio::pin!(timeout);
        let shutdown = platform::wait_for_shutdown();
        tokio::pin!(shutdown);

        let (terminal_reason, exit_code, signal) = tokio::select! {
            status = child.wait() => {
                let status = status?;
                let signal = signal_name(status);
                platform::cleanup_after_exit(&process_tree, config.execution.termination_grace_ms)
                    .await?;
                (TerminalReason::Exited, status.code(), signal)
            }
            _ = &mut timeout => {
                platform::terminate(&mut child, &process_tree, config.execution.termination_grace_ms).await?;
                (TerminalReason::TimedOut, None, None)
            }
            shutdown_result = &mut shutdown => {
                shutdown_result?;
                platform::terminate(&mut child, &process_tree, config.execution.termination_grace_ms).await?;
                (TerminalReason::OwnerShutdown, None, None)
            }
        };

        let stdout = stdout_task
            .await
            .map_err(|error| Error::Unavailable(format!("stdout capture failed: {error}")))??;
        let stderr = stderr_task
            .await
            .map_err(|error| Error::Unavailable(format!("stderr capture failed: {error}")))??;

        Ok(ResultEnvelope {
            protocol_version: crate::protocol::PROTOCOL_VERSION,
            terminal_reason,
            exit_code,
            signal,
            duration_ms: started.elapsed().as_millis() as u64,
            stdout,
            stderr,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

async fn read_output<R>(
    mut reader: R,
    stream: OutputStream,
    output_mode: OutputMode,
    tail_bytes: usize,
) -> Result<CapturedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut rolling = RollingOutput::new(tail_bytes)?;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let bytes = &buffer[..read];
        rolling.push(bytes);
        if output_mode == OutputMode::Passthrough {
            match stream {
                OutputStream::Stdout => {
                    let mut output = tokio::io::stdout();
                    output.write_all(bytes).await?;
                    output.flush().await?;
                }
                OutputStream::Stderr => {
                    let mut output = tokio::io::stderr();
                    output.write_all(bytes).await?;
                    output.flush().await?;
                }
            }
        }
    }
    Ok(rolling.finish())
}

fn signal_name(status: std::process::ExitStatus) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        status.signal().map(|signal| signal.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}
