use tokio::process::{Child, Command};

use crate::error::Result;

pub fn configure_command(_: &mut Command) -> Result<()> {
    Ok(())
}

pub async fn terminate(child: &mut Child, _: u64) -> Result<std::process::ExitStatus> {
    child.kill().await?;
    Ok(child.wait().await?)
}
