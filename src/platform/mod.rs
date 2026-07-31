#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::process::ExitStatus;

use tokio::process::{Child, Command};

use crate::error::Result;

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
pub async fn terminate(child: &mut Child, grace_ms: u64) -> Result<ExitStatus> {
    unix::terminate(child, grace_ms).await
}

#[cfg(windows)]
pub async fn terminate(child: &mut Child, grace_ms: u64) -> Result<ExitStatus> {
    windows::terminate(child, grace_ms).await
}

#[cfg(not(any(unix, windows)))]
pub async fn terminate(child: &mut Child, _: u64) -> Result<ExitStatus> {
    Ok(child.wait().await?)
}
