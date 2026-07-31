use std::{io, os::unix::process::CommandExt};

use nix::{
    errno::Errno,
    sys::signal::{Signal, killpg},
    unistd::{Pid, setpgid},
};
use tokio::{
    process::{Child, Command},
    time::{Duration, timeout},
};

use crate::error::{Error, Result};

pub fn configure_command(command: &mut Command) -> Result<()> {
    unsafe {
        command.as_std_mut().pre_exec(|| {
            setpgid(Pid::from_raw(0), Pid::from_raw(0))
                .map_err(|error| io::Error::from_raw_os_error(error as i32))
        });
    }
    Ok(())
}

pub async fn terminate(child: &mut Child, grace_ms: u64) -> Result<std::process::ExitStatus> {
    let group = child
        .id()
        .map(|pid| Pid::from_raw(pid as i32))
        .ok_or_else(|| Error::Unavailable("child exited before process group cleanup".into()))?;
    signal_group(group, Signal::SIGTERM)?;
    match timeout(Duration::from_millis(grace_ms), child.wait()).await {
        Ok(status) => Ok(status?),
        Err(_) => {
            signal_group(group, Signal::SIGKILL)?;
            Ok(child.wait().await?)
        }
    }
}

fn signal_group(group: Pid, signal: Signal) -> Result<()> {
    match killpg(group, signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(Error::Io(io::Error::from_raw_os_error(error as i32))),
    }
}
