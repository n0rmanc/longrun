use std::{io, os::unix::process::CommandExt};

use nix::{
    errno::Errno,
    sys::signal::{Signal, killpg},
    unistd::{Pid, getpgid, setpgid},
};
use tokio::{
    process::{Child, Command},
    time::{Duration, sleep, timeout},
};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub struct ProcessGroup {
    pgid: Pid,
}

pub fn configure_command(command: &mut Command) -> Result<()> {
    unsafe {
        command.as_std_mut().pre_exec(|| {
            setpgid(Pid::from_raw(0), Pid::from_raw(0))
                .map_err(|error| io::Error::from_raw_os_error(error as i32))
        });
    }
    Ok(())
}

pub fn track_child(child: &Child) -> Result<ProcessGroup> {
    let pid = child
        .id()
        .ok_or_else(|| Error::Unavailable("child exited before process-group tracking".into()))?;
    let pid = Pid::from_raw(pid as i32);
    Ok(ProcessGroup {
        pgid: getpgid(Some(pid)).unwrap_or(pid),
    })
}

pub async fn terminate(
    child: &mut Child,
    process_group: &ProcessGroup,
    grace_ms: u64,
) -> Result<()> {
    signal_group(process_group.pgid, Signal::SIGTERM)?;
    match timeout(Duration::from_millis(grace_ms), child.wait()).await {
        Ok(status) => {
            let _ = status?;
        }
        Err(_) => {
            signal_group(process_group.pgid, Signal::SIGKILL)?;
            let _ = child.wait().await?;
        }
    }
    Ok(())
}

pub async fn cleanup_after_exit(process_group: &ProcessGroup, grace_ms: u64) -> Result<()> {
    if !signal_group(process_group.pgid, Signal::SIGTERM)? {
        return Ok(());
    }
    sleep(Duration::from_millis(grace_ms)).await;
    let _ = signal_group(process_group.pgid, Signal::SIGKILL)?;
    Ok(())
}

fn signal_group(group: Pid, signal: Signal) -> Result<bool> {
    match killpg(group, signal) {
        Ok(()) => Ok(true),
        Err(Errno::ESRCH) => Ok(false),
        Err(error) => Err(Error::Io(io::Error::from_raw_os_error(error as i32))),
    }
}
