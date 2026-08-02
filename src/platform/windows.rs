use std::{ffi::c_void, mem::size_of, os::windows::process::CommandExt};

use tokio::{
    process::{Child, Command},
    time::{Duration, timeout},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent},
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject,
        },
        Threading::{CREATE_NEW_PROCESS_GROUP, OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
    },
};

use crate::{
    error::Error::Io,
    error::{Error, Result},
};

pub struct JobObject(HANDLE);

impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn configure_command(command: &mut Command) -> Result<()> {
    command
        .as_std_mut()
        .creation_flags(CREATE_NEW_PROCESS_GROUP);
    Ok(())
}

pub fn track_child(child: &Child) -> Result<JobObject> {
    let job = JobObject(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
    if job.0.is_null() {
        return Err(Io(std::io::Error::last_os_error()));
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(Io(std::io::Error::last_os_error()));
    }
    let pid = child
        .id()
        .ok_or_else(|| Error::Unavailable("child exited before Job Object assignment".into()))?;
    let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
    if process.is_null() {
        return Err(Io(std::io::Error::last_os_error()));
    }
    let assigned = unsafe { AssignProcessToJobObject(job.0, process) };
    unsafe {
        CloseHandle(process);
    }
    if assigned == 0 {
        return Err(Io(std::io::Error::last_os_error()));
    }
    Ok(job)
}

pub async fn terminate(child: &mut Child, job: &JobObject, grace_ms: u64) -> Result<()> {
    if let Some(pid) = child.id()
        && unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) } != 0
        && let Ok(status) = timeout(Duration::from_millis(grace_ms), child.wait()).await
    {
        let _ = status?;
        return Ok(());
    }
    if unsafe { TerminateJobObject(job.0, 1) } == 0 {
        return Err(Io(std::io::Error::last_os_error()));
    }
    let _ = child.wait().await?;
    Ok(())
}
