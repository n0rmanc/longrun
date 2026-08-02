use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    paths::AppPaths,
    protocol::sha256_hex,
};

const MARKETPLACE_NAME: &str = "longrun-local";
const PLUGIN_SELECTOR: &str = "longrun@longrun-local";
const INVENTORY_FILE: &str = ".longrun-installation.json";
const PLUGIN_MANIFEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/codex/plugin.json"
));
const HOOKS_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/codex/hooks.json"
));
const MARKETPLACE_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/codex/marketplace.json"
));
const SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/codex/skills/longrun/SKILL.md"
));

const OWNED_FILES: &[&str] = &[
    ".agents/plugins/marketplace.json",
    "plugins/longrun/.codex-plugin/plugin.json",
    "plugins/longrun/hooks.json",
    "plugins/longrun/skills/longrun/SKILL.md",
];

#[derive(Debug, Clone, Serialize)]
pub struct InitReport {
    pub generated_root: PathBuf,
    pub plugin_selector: &'static str,
    pub manifest_hash: String,
    pub repaired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UninstallReport {
    pub generated_root: PathBuf,
    pub removed_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: &'static str,
    pub ok: bool,
    pub required: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallationInventory {
    version: u8,
    integration: String,
    binary_path: String,
    marketplace_name: String,
    plugin_selector: String,
    manifest_hash: String,
    owned_files: Vec<String>,
}

pub fn init(paths: &AppPaths, executable: &Path, repaired: bool) -> Result<InitReport> {
    let executable = resolved_executable(executable)?;
    let assets = rendered_assets(&executable)?;
    for (relative, content) in &assets {
        write_atomic(&owned_path(&paths.integration_dir, relative)?, content)?;
    }
    let manifest_hash = hash_assets(&assets);
    let inventory = InstallationInventory {
        version: 2,
        integration: "codex".into(),
        binary_path: utf8_path(&executable)?.into(),
        marketplace_name: MARKETPLACE_NAME.into(),
        plugin_selector: PLUGIN_SELECTOR.into(),
        manifest_hash: manifest_hash.clone(),
        owned_files: OWNED_FILES.iter().map(|path| (*path).into()).collect(),
    };
    write_atomic(
        &paths.integration_dir.join(INVENTORY_FILE),
        &serde_json::to_vec_pretty(&inventory)?,
    )?;

    run_codex(&[
        "plugin".into(),
        "marketplace".into(),
        "add".into(),
        paths.integration_dir.clone().into_os_string(),
    ])?;
    run_codex(&["plugin".into(), "add".into(), PLUGIN_SELECTOR.into()])?;

    Ok(InitReport {
        generated_root: paths.integration_dir.clone(),
        plugin_selector: PLUGIN_SELECTOR,
        manifest_hash,
        repaired,
    })
}

pub fn uninstall(paths: &AppPaths) -> Result<UninstallReport> {
    let Some(inventory) = read_inventory(paths)? else {
        return Ok(UninstallReport {
            generated_root: paths.integration_dir.clone(),
            removed_files: 0,
        });
    };
    if inventory.integration != "codex"
        || inventory.marketplace_name != MARKETPLACE_NAME
        || inventory.plugin_selector != PLUGIN_SELECTOR
    {
        return Err(Error::InvalidInput(
            "integration inventory does not identify Longrun-owned Codex assets".into(),
        ));
    }
    run_codex_allow_absent(&[
        "plugin".into(),
        "remove".into(),
        inventory.plugin_selector.clone().into(),
    ])?;
    run_codex_allow_absent(&[
        "plugin".into(),
        "marketplace".into(),
        "remove".into(),
        inventory.marketplace_name.clone().into(),
    ])?;

    let mut removed_files = 0;
    for relative in inventory.owned_files.iter().rev() {
        let path = owned_path(&paths.integration_dir, relative)?;
        match fs::remove_file(&path) {
            Ok(()) => {
                removed_files += 1;
                prune_empty_parents(&paths.integration_dir, &path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    match fs::remove_file(paths.integration_dir.join(INVENTORY_FILE)) {
        Ok(()) => removed_files += 1,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    Ok(UninstallReport {
        generated_root: paths.integration_dir.clone(),
        removed_files,
    })
}

pub async fn doctor(paths: &AppPaths, config: &Config) -> DoctorReport {
    let executable = match std::env::current_exe().and_then(fs::canonicalize) {
        Ok(path) => path,
        Err(error) => {
            return DoctorReport {
                healthy: false,
                checks: vec![check(
                    "executable",
                    false,
                    true,
                    format!("cannot resolve current executable: {error}"),
                )],
            };
        }
    };
    let checks = vec![
        check(
            "executable",
            executable.is_file(),
            true,
            format!(
                "{} (Longrun {})",
                utf8_path(&executable)
                    .map(str::to_owned)
                    .unwrap_or_else(|error| error.to_string()),
                env!("CARGO_PKG_VERSION")
            ),
        ),
        state_directory_check(paths),
        legacy_state_check(paths),
        handoff_directory_check(paths),
        codex_version_check(),
        codex_plugin_commands_check(),
        codex_plugin_activation_check(),
        integration_check(paths, &executable),
        hooks_check(paths, &executable),
        sandbox_profile_check(config),
        timeout_margin_check(config),
        platform_process_control_check(),
    ];
    let healthy = checks.iter().all(|check| !check.required || check.ok);
    DoctorReport { healthy, checks }
}

pub fn write_doctor(report: &DoctorReport, json: bool) -> Result<()> {
    if json {
        serde_json::to_writer(std::io::stdout(), report)?;
        std::io::stdout().write_all(b"\n")?;
    } else {
        for check in &report.checks {
            let status = if check.ok {
                "OK"
            } else if check.required {
                "FAIL"
            } else {
                "WARN"
            };
            println!("{status}\t{}\t{}", check.name, check.detail);
        }
    }
    Ok(())
}

fn rendered_assets(executable: &Path) -> Result<Vec<(&'static str, Vec<u8>)>> {
    Ok(vec![
        (
            ".agents/plugins/marketplace.json",
            MARKETPLACE_TEMPLATE.as_bytes().to_vec(),
        ),
        (
            "plugins/longrun/.codex-plugin/plugin.json",
            PLUGIN_MANIFEST.as_bytes().to_vec(),
        ),
        (
            "plugins/longrun/hooks.json",
            render_hooks(executable)?.into_bytes(),
        ),
        (
            "plugins/longrun/skills/longrun/SKILL.md",
            render_skill(executable)?.into_bytes(),
        ),
    ])
}

fn render_skill(executable: &Path) -> Result<String> {
    render_skill_for(executable, cfg!(windows))
}

fn render_skill_for(executable: &Path, windows: bool) -> Result<String> {
    let executable = utf8_path(executable)?;
    Ok(SKILL.replace("__LONGRUN_EXECUTABLE__", &shell_quote(executable, windows)))
}

fn render_hooks(executable: &Path) -> Result<String> {
    let executable = utf8_path(executable)?;
    let mut hooks: Value = serde_json::from_str(HOOKS_TEMPLATE)?;
    replace_template_strings(
        &mut hooks,
        &[
            (
                "__LONGRUN_UNIX_PRE_TOOL_USE__",
                hook_command(executable, "pre-tool-use", false),
            ),
            (
                "__LONGRUN_WINDOWS_PRE_TOOL_USE__",
                hook_command(executable, "pre-tool-use", true),
            ),
            (
                "__LONGRUN_UNIX_POST_TOOL_USE__",
                hook_command(executable, "post-tool-use", false),
            ),
            (
                "__LONGRUN_WINDOWS_POST_TOOL_USE__",
                hook_command(executable, "post-tool-use", true),
            ),
        ],
    );
    Ok(format!("{}\n", serde_json::to_string_pretty(&hooks)?))
}

fn replace_template_strings(value: &mut Value, replacements: &[(&str, String)]) {
    match value {
        Value::String(string) => {
            if let Some((_, replacement)) = replacements
                .iter()
                .find(|(placeholder, _)| *placeholder == string)
            {
                *string = replacement.clone();
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_template_strings(value, replacements);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                replace_template_strings(value, replacements);
            }
        }
        _ => {}
    }
}

fn hook_command(executable: &str, event: &str, windows: bool) -> String {
    let executable = shell_quote(executable, windows);
    if windows {
        format!("{executable} hook codex {event}")
    } else {
        format!("exec {executable} hook codex {event}")
    }
}

fn shell_quote(value: &str, windows: bool) -> String {
    if windows {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn resolved_executable(executable: &Path) -> Result<PathBuf> {
    let executable = if executable.is_absolute() {
        executable.to_path_buf()
    } else {
        std::env::current_dir()?.join(executable)
    };
    Ok(fs::canonicalize(executable)?)
}

fn utf8_path(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::InvalidInput("Codex integration paths must be UTF-8".into()))
}

fn hash_assets(assets: &[(&str, Vec<u8>)]) -> String {
    let mut data = Vec::new();
    for (path, content) in assets {
        data.extend_from_slice(path.as_bytes());
        data.push(0);
        data.extend_from_slice(content);
        data.push(0);
    }
    sha256_hex(&data)
}

fn read_inventory(paths: &AppPaths) -> Result<Option<InstallationInventory>> {
    let path = paths.integration_dir.join(INVENTORY_FILE);
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn owned_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.components().any(|component| {
        !matches!(component, Component::Normal(_)) || matches!(component, Component::ParentDir)
    }) {
        return Err(Error::InvalidInput(
            "integration inventory contains an unsafe owned path".into(),
        ));
    }
    Ok(root.join(path))
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidInput("integration asset has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .ok_or_else(|| Error::InvalidInput("integration asset has no filename".into()))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{filename}.{}.tmp", Uuid::now_v7()));
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

fn prune_empty_parents(root: &Path, path: &Path) -> Result<()> {
    let mut current = path.parent();
    while let Some(directory) = current {
        if directory == root {
            break;
        }
        match fs::remove_dir(directory) {
            Ok(()) => current = directory.parent(),
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                current = directory.parent()
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn run_codex(arguments: &[std::ffi::OsString]) -> Result<()> {
    let output = Command::new("codex")
        .args(arguments)
        .output()
        .map_err(|error| Error::Unavailable(format!("cannot run Codex CLI: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(codex_failure(arguments, &output))
}

fn run_codex_allow_absent(arguments: &[std::ffi::OsString]) -> Result<()> {
    let output = Command::new("codex")
        .args(arguments)
        .output()
        .map_err(|error| Error::Unavailable(format!("cannot run Codex CLI: {error}")))?;
    if output.status.success() || reports_absence(&output) {
        Ok(())
    } else {
        Err(codex_failure(arguments, &output))
    }
}

fn codex_failure(arguments: &[std::ffi::OsString], output: &std::process::Output) -> Error {
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    let detail = if detail.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        detail.to_owned()
    };
    Error::Unavailable(format!(
        "`codex {}` failed: {}",
        arguments
            .iter()
            .map(|argument| argument.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        detail
    ))
}

fn reports_absence(output: &std::process::Output) -> bool {
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    [
        "not found",
        "not installed",
        "does not exist",
        "unknown marketplace",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn check(name: &'static str, ok: bool, required: bool, detail: String) -> DoctorCheck {
    DoctorCheck {
        name,
        ok,
        required,
        detail,
    }
}

fn state_directory_check(paths: &AppPaths) -> DoctorCheck {
    match fs::metadata(&paths.state_dir) {
        Ok(metadata) if metadata.is_dir() => check(
            "state_directory",
            private_directory(&metadata),
            true,
            format!("{} exists", paths.state_dir.display()),
        ),
        Ok(_) => check(
            "state_directory",
            false,
            true,
            format!("{} is not a directory", paths.state_dir.display()),
        ),
        Err(error) => check("state_directory", false, true, error.to_string()),
    }
}

fn legacy_state_check(paths: &AppPaths) -> DoctorCheck {
    let path = paths.state_dir.join("longrun.sqlite");
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => check(
            "legacy_state",
            true,
            false,
            format!(
                "{} is ignored by the ephemeral runtime; remove it with `longrun uninstall --codex --purge-data` if desired",
                path.display()
            ),
        ),
        Ok(_) => check(
            "legacy_state",
            false,
            false,
            format!("{} is not a regular file", path.display()),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => check(
            "legacy_state",
            true,
            false,
            "no legacy durable job state found".into(),
        ),
        Err(error) => check("legacy_state", false, false, error.to_string()),
    }
}

fn handoff_directory_check(paths: &AppPaths) -> DoctorCheck {
    match fs::metadata(&paths.handoff_dir) {
        Ok(metadata) if metadata.is_dir() => check(
            "handoff_directory",
            private_directory(&metadata),
            true,
            format!("{} is private", paths.handoff_dir.display()),
        ),
        Ok(_) => check(
            "handoff_directory",
            false,
            true,
            format!("{} is not a directory", paths.handoff_dir.display()),
        ),
        Err(error) => check("handoff_directory", false, true, error.to_string()),
    }
}

#[cfg(unix)]
fn private_directory(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o077 == 0
}

#[cfg(not(unix))]
fn private_directory(_: &fs::Metadata) -> bool {
    true
}

fn codex_version_check() -> DoctorCheck {
    match probe_codex(&["--version"]) {
        Ok(version) => check("codex_version", true, true, version),
        Err(error) => check("codex_version", false, true, error),
    }
}

fn codex_plugin_commands_check() -> DoctorCheck {
    let commands = [
        &["plugin", "marketplace", "--help"][..],
        &["plugin", "add", "--help"][..],
        &["plugin", "remove", "--help"][..],
        &["plugin", "marketplace", "remove", "--help"][..],
    ];
    match commands
        .iter()
        .find_map(|command| probe_codex(command).err())
    {
        Some(error) => check("codex_plugin_commands", false, true, error),
        None => check(
            "codex_plugin_commands",
            true,
            true,
            "marketplace add/remove and plugin add/remove available".into(),
        ),
    }
}

fn codex_plugin_activation_check() -> DoctorCheck {
    match probe_codex(&["plugin", "list"]) {
        Ok(output) if output.contains(PLUGIN_SELECTOR) => check(
            "codex_plugin_activation",
            true,
            true,
            format!("{PLUGIN_SELECTOR} is installed"),
        ),
        Ok(_) => check(
            "codex_plugin_activation",
            false,
            true,
            format!("{PLUGIN_SELECTOR} is not installed"),
        ),
        Err(error) => check("codex_plugin_activation", false, true, error),
    }
}

fn sandbox_profile_check(config: &Config) -> DoctorCheck {
    let profile = &config.execution.permission_profile;
    if !config.permits_permission_profile(profile) {
        return check(
            "sandbox_profile",
            false,
            true,
            format!("{profile} is not enabled in Longrun configuration"),
        );
    }
    match probe_codex(&["sandbox", "--help"]) {
        Ok(_) => check(
            "sandbox_profile",
            true,
            true,
            format!("{profile} is configured and Codex sandbox is available"),
        ),
        Err(error) => check("sandbox_profile", false, true, error),
    }
}

fn timeout_margin_check(config: &Config) -> DoctorCheck {
    match config.validate() {
        Ok(()) => check(
            "timeout_margin",
            true,
            true,
            format!(
                "PostToolUse timeout {} ms covers target and cleanup margins",
                config.execution.post_tool_use_timeout_ms
            ),
        ),
        Err(error) => check("timeout_margin", false, true, error.to_string()),
    }
}

fn platform_process_control_check() -> DoctorCheck {
    #[cfg(unix)]
    {
        check(
            "platform_process_control",
            true,
            true,
            "Unix process-group termination is available; hard owner death is best effort".into(),
        )
    }
    #[cfg(windows)]
    {
        check(
            "platform_process_control",
            true,
            true,
            "Windows Job Object termination is available".into(),
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        check(
            "platform_process_control",
            false,
            true,
            "this platform has no supported process-tree control implementation".into(),
        )
    }
}

fn integration_check(paths: &AppPaths, executable: &Path) -> DoctorCheck {
    let Ok(Some(inventory)) = read_inventory(paths) else {
        return check(
            "integration",
            false,
            true,
            "Longrun Codex integration is not installed".into(),
        );
    };
    let expected_binary = utf8_path(executable).unwrap_or_default();
    let assets = match rendered_assets(executable) {
        Ok(assets) => assets,
        Err(error) => return check("integration", false, true, error.to_string()),
    };
    let files_match = assets.iter().all(|(relative, content)| {
        owned_path(&paths.integration_dir, relative)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .is_some_and(|actual| actual == *content)
    });
    let hash_matches = inventory.manifest_hash == hash_assets(&assets);
    let binary_matches = inventory.binary_path == expected_binary;
    check(
        "integration",
        files_match && hash_matches && binary_matches,
        true,
        if files_match && hash_matches && binary_matches {
            format!("{} is installed", PLUGIN_SELECTOR)
        } else {
            "repair required: generated files, manifest hash, or binary path differ".into()
        },
    )
}

fn hooks_check(paths: &AppPaths, executable: &Path) -> DoctorCheck {
    let path = paths.integration_dir.join("plugins/longrun/hooks.json");
    let expected = match utf8_path(executable) {
        Ok(executable) => [
            hook_command(executable, "pre-tool-use", cfg!(windows)),
            hook_command(executable, "post-tool-use", cfg!(windows)),
        ],
        Err(error) => return check("hooks", false, true, error.to_string()),
    };
    match fs::read_to_string(path) {
        Ok(hooks) if hooks_include(&hooks, &expected) && !hooks.contains("SessionStart") => check(
            "hooks",
            true,
            true,
            "absolute PreToolUse and PostToolUse hooks match this binary".into(),
        ),
        Ok(_) => check(
            "hooks",
            false,
            true,
            "generated hooks do not match this executable; run `longrun init --codex --repair`"
                .into(),
        ),
        Err(error) => check("hooks", false, true, error.to_string()),
    }
}

fn hooks_include(hooks: &str, expected: &[String]) -> bool {
    expected
        .iter()
        .all(|command| serde_json::to_string(command).is_ok_and(|encoded| hooks.contains(&encoded)))
}

fn probe_codex(arguments: &[&str]) -> std::result::Result<String, String> {
    let output = Command::new("codex")
        .args(arguments)
        .output()
        .map_err(|error| format!("cannot run `codex {}`: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "`codex {}` failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let output = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(if output.is_empty() {
        format!("`codex {}` succeeded", arguments.join(" "))
    } else {
        output
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{hook_command, hooks_include, render_hooks, render_skill_for};

    #[test]
    fn rendered_hooks_use_only_the_two_active_wait_hooks() {
        let hooks = render_hooks(Path::new("/opt/longrun")).expect("render hooks");
        let expected = [
            hook_command("/opt/longrun", "pre-tool-use", false),
            hook_command("/opt/longrun", "post-tool-use", false),
        ];
        assert!(hooks_include(&hooks, &expected));
        assert!(!hooks.contains("SessionStart"));
        assert!(hooks.contains("\"additionalContextLimit\": 0"));
    }

    #[test]
    fn rendered_hooks_match_verbatim_windows_paths_after_json_escaping() {
        let executable = r"\\?\C:\Longrun\longrun.exe";
        let hooks = render_hooks(Path::new(executable)).expect("render hooks");
        let expected = [
            hook_command(executable, "pre-tool-use", true),
            hook_command(executable, "post-tool-use", true),
        ];
        assert!(hooks_include(&hooks, &expected));
    }

    #[test]
    fn rendered_skill_uses_windows_command_quoting() {
        let skill = render_skill_for(
            Path::new(r#"C:\Program Files\Longrun"bin\longrun.exe"#),
            true,
        )
        .expect("render skill");
        assert!(skill.contains(r#""C:\Program Files\Longrun\"bin\longrun.exe""#));
    }
}
