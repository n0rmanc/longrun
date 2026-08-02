#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::process::ExitStatus;

use tokio::process::{Child, Command};

use crate::error::Result;

#[cfg(unix)]
pub struct ProcessTree(unix::ProcessGroup);
#[cfg(windows)]
pub struct ProcessTree(windows::JobObject);
#[cfg(not(any(unix, windows)))]
pub struct ProcessTree;

#[cfg(unix)]
pub fn configure_command(command: &mut Command) -> Result<()> {
    unix::configure_command(command)
}

#[cfg(windows)]
pub fn configure_command(command: &mut Command) -> Result<()> {
    windows::configure_command(command)
}

#[cfg(not(any(unix, windows)))]
pub fn configure_command(_: &mut Command) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn track_child(child: &Child) -> Result<ProcessTree> {
    Ok(ProcessTree(unix::track_child(child)?))
}

#[cfg(windows)]
pub fn track_child(child: &Child) -> Result<ProcessTree> {
    Ok(ProcessTree(windows::track_child(child)?))
}

#[cfg(not(any(unix, windows)))]
pub fn track_child(_: &Child) -> Result<ProcessTree> {
    Ok(ProcessTree)
}

#[cfg(unix)]
pub async fn terminate(child: &mut Child, process_tree: &ProcessTree, grace_ms: u64) -> Result<()> {
    unix::terminate(child, &process_tree.0, grace_ms).await
}

#[cfg(unix)]
pub async fn cleanup_after_exit(process_tree: &ProcessTree, grace_ms: u64) -> Result<()> {
    unix::cleanup_after_exit(&process_tree.0, grace_ms).await
}

#[cfg(windows)]
pub async fn terminate(child: &mut Child, process_tree: &ProcessTree, grace_ms: u64) -> Result<()> {
    windows::terminate(child, &process_tree.0, grace_ms).await
}

#[cfg(windows)]
pub async fn cleanup_after_exit(_: &ProcessTree, _: u64) -> Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub async fn terminate(child: &mut Child, _: &ProcessTree, _: u64) -> Result<()> {
    let _ = child.kill().await;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub async fn cleanup_after_exit(_: &ProcessTree, _: u64) -> Result<()> {
    Ok(())
}

pub async fn wait_for_shutdown() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = interrupt.recv() => Ok(()),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}

pub fn exit_code(status: ExitStatus) -> Option<i32> {
    status.code()
}
