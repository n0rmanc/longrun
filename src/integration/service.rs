use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    error::{Error, Result},
    paths::AppPaths,
    protocol::sha256_hex,
};

pub const SERVICE_LABEL: &str = "dev.longrun.supervisor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
}

pub fn install(paths: &AppPaths, executable: &Path, config_path: &Path) -> Result<()> {
    let path = artifact_path(paths)?;
    write_artifact(&path, executable, config_path)?;
    #[cfg(target_os = "macos")]
    {
        let domain = launchd_domain()?;
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{domain}/{SERVICE_LABEL}")])
            .status();
        run(
            "launchctl",
            &[
                "bootstrap",
                &domain,
                path.to_str()
                    .ok_or_else(|| Error::InvalidInput("service path must be UTF-8".into()))?,
            ],
        )?;
    }
    #[cfg(target_os = "linux")]
    {
        run("systemctl", &["--user", "daemon-reload"])?;
        run("systemctl", &["--user", "enable", SERVICE_LABEL])?;
    }
    #[cfg(windows)]
    {
        run(
            "reg",
            &[
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Longrun",
                "/t",
                "REG_SZ",
                "/d",
                path.to_str()
                    .ok_or_else(|| Error::InvalidInput("service path must be UTF-8".into()))?,
                "/f",
            ],
        )?;
    }
    Ok(())
}

pub fn uninstall(paths: &AppPaths) -> Result<()> {
    let path = artifact_path(paths)?;
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("launchctl")
            .args([
                "bootout",
                &format!("{}/{}", launchd_domain()?, SERVICE_LABEL),
            ])
            .status();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", SERVICE_LABEL])
            .status();
        run("systemctl", &["--user", "daemon-reload"])?;
    }
    #[cfg(windows)]
    {
        let _ = Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Longrun",
                "/f",
            ])
            .status();
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn start(paths: &AppPaths) -> Result<()> {
    let path = artifact_path(paths)?;
    if !path.exists() {
        return Err(Error::NotFound(
            "Longrun service is not installed; run `longrun service install` first".into(),
        ));
    }
    #[cfg(target_os = "macos")]
    run(
        "launchctl",
        &[
            "kickstart",
            "-k",
            &format!("{}/{}", launchd_domain()?, SERVICE_LABEL),
        ],
    )?;
    #[cfg(target_os = "linux")]
    run("systemctl", &["--user", "start", SERVICE_LABEL])?;
    #[cfg(windows)]
    {
        Command::new(path).spawn()?;
    }
    Ok(())
}

pub fn stop(paths: &AppPaths) -> Result<()> {
    let path = artifact_path(paths)?;
    if !path.exists() {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    run(
        "launchctl",
        &[
            "kill",
            "SIGTERM",
            &format!("{}/{}", launchd_domain()?, SERVICE_LABEL),
        ],
    )?;
    #[cfg(target_os = "linux")]
    run("systemctl", &["--user", "stop", SERVICE_LABEL])?;
    #[cfg(windows)]
    {
        Err(Error::Unavailable(
            "Windows service stop requires a running supervisor shutdown endpoint".into(),
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

pub fn status(paths: &AppPaths) -> Result<ServiceStatus> {
    let installed = artifact_path(paths)?.exists();
    if !installed {
        return Ok(ServiceStatus {
            installed: false,
            running: false,
        });
    }
    #[cfg(target_os = "macos")]
    let running = Command::new("launchctl")
        .args(["print", &format!("{}/{}", launchd_domain()?, SERVICE_LABEL)])
        .status()?
        .success();
    #[cfg(target_os = "linux")]
    let running = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", SERVICE_LABEL])
        .status()?
        .success();
    #[cfg(windows)]
    let running = false;
    Ok(ServiceStatus { installed, running })
}

pub fn launchd_plist(executable: &Path, config_path: &Path) -> Result<String> {
    let executable = absolute_utf8(executable)?;
    let config_path = absolute_utf8(config_path)?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{SERVICE_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{executable}</string>
    <string>--config</string>
    <string>{config_path}</string>
    <string>daemon</string>
    <string>--foreground</string>
  </array>
  <key>RunAtLoad</key>
  <false/>
  <key>ProcessType</key>
  <string>Background</string>
</dict>
</plist>
"#,
        executable = xml_escape(executable),
        config_path = xml_escape(config_path),
    ))
}

pub fn systemd_user_unit(executable: &Path, config_path: &Path) -> Result<String> {
    let executable = absolute_utf8(executable)?;
    let config_path = absolute_utf8(config_path)?;
    Ok(format!(
        "[Unit]\nDescription=Longrun durable supervisor\n\n[Service]\nType=simple\nExecStart={} --config {} daemon --foreground\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        systemd_escape(executable),
        systemd_escape(config_path),
    ))
}

pub fn windows_startup_script(executable: &Path, config_path: &Path) -> Result<String> {
    let executable = absolute_utf8(executable)?;
    let config_path = absolute_utf8(config_path)?;
    Ok(format!(
        "@echo off\r\n\"{}\" --config \"{}\" daemon --foreground\r\n",
        batch_escape(executable),
        batch_escape(config_path),
    ))
}

pub fn manifest_hash(executable: &Path, config_path: &Path) -> Result<String> {
    let launchd = launchd_plist(executable, config_path)?;
    let systemd = systemd_user_unit(executable, config_path)?;
    let windows = windows_startup_script(executable, config_path)?;
    Ok(sha256_hex(
        [launchd.as_bytes(), systemd.as_bytes(), windows.as_bytes()]
            .concat()
            .as_slice(),
    ))
}

fn artifact_path(paths: &AppPaths) -> Result<PathBuf> {
    #[cfg(not(windows))]
    let _ = paths;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let base = directories::BaseDirs::new()
        .ok_or_else(|| Error::Unavailable("cannot determine user directories".into()))?;
    #[cfg(target_os = "macos")]
    {
        Ok(base
            .home_dir()
            .join("Library/LaunchAgents")
            .join(format!("{SERVICE_LABEL}.plist")))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(base
            .config_dir()
            .join("systemd/user")
            .join(format!("{SERVICE_LABEL}.service")))
    }
    #[cfg(windows)]
    {
        Ok(paths.integration_dir.join("service/longrun-daemon.cmd"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (paths, base);
        Err(Error::Unavailable(
            "Longrun service is unsupported on this platform".into(),
        ))
    }
}

fn write_artifact(path: &Path, executable: &Path, config_path: &Path) -> Result<()> {
    let contents = if cfg!(target_os = "macos") {
        launchd_plist(executable, config_path)?
    } else if cfg!(target_os = "linux") {
        systemd_user_unit(executable, config_path)?
    } else if cfg!(windows) {
        windows_startup_script(executable, config_path)?
    } else {
        return Err(Error::Unavailable(
            "Longrun service is unsupported on this platform".into(),
        ));
    };
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidInput("service path has no parent".into()))?;
    fs::create_dir_all(parent)?;
    fs::write(path, contents)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn launchd_domain() -> Result<String> {
    let output = Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(Error::Unavailable(
            "cannot determine the current user for launchd".into(),
        ));
    }
    let uid = std::str::from_utf8(&output.stdout)
        .map_err(|_| Error::Unavailable("current user id is not UTF-8".into()))?
        .trim();
    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Unavailable("current user id is invalid".into()));
    }
    Ok(format!("gui/{uid}"))
}

fn run(program: &str, arguments: &[&str]) -> Result<()> {
    let status = Command::new(program).args(arguments).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Unavailable(format!(
            "`{program}` exited with {status}"
        )))
    }
}

fn absolute_utf8(path: &Path) -> Result<&str> {
    if !path.is_absolute() {
        return Err(Error::InvalidInput("service paths must be absolute".into()));
    }
    path.to_str()
        .ok_or_else(|| Error::InvalidInput("service paths must be UTF-8".into()))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn systemd_escape(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn batch_escape(value: &str) -> String {
    value.replace('"', "\"\"")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        SERVICE_LABEL, launchd_plist, manifest_hash, systemd_user_unit, windows_startup_script,
    };

    #[test]
    fn service_artifacts_preserve_absolute_binary_and_config_paths() {
        let executable = Path::new("/opt/Longrun & Co/longrun");
        let config = Path::new("/opt/Longrun & Co/config.toml");

        let launchd = launchd_plist(executable, config).expect("launchd");
        assert!(launchd.contains(SERVICE_LABEL));
        assert!(launchd.contains("/opt/Longrun &amp; Co/longrun"));
        assert!(launchd.contains("<string>daemon</string>"));

        let systemd = systemd_user_unit(executable, config).expect("systemd");
        assert!(systemd.contains("ExecStart=\"/opt/Longrun & Co/longrun\""));
        assert!(systemd.contains("daemon --foreground"));

        let windows = windows_startup_script(executable, config).expect("windows");
        assert!(windows.contains("\"/opt/Longrun & Co/longrun\" --config"));
        assert_ne!(
            manifest_hash(executable, config).expect("hash"),
            manifest_hash(Path::new("/opt/longrun"), config).expect("different hash")
        );
    }

    #[test]
    fn service_artifacts_reject_relative_paths() {
        assert!(launchd_plist(Path::new("longrun"), Path::new("/tmp/config")).is_err());
    }
}
