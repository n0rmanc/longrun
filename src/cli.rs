use std::{
    ffi::{OsStr, OsString},
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand};

use crate::{
    config::Config,
    error::{Error, Result},
    handoff::HandoffStore,
    hook::{
        input::{PostToolUseInput, PreToolUseInput},
        post_tool_use::handle_post_tool_use,
        pre_tool_use::{handle_pre_tool_use, now_ms},
    },
    integration::codex,
    metrics,
    paths::AppPaths,
    protocol::{NativeString, ResultEnvelope, TargetSpec, TerminalReason, sha256_hex},
    runner::{ExecutionMode, OutputMode, Runner},
};

#[derive(Debug, Parser)]
#[command(
    name = "longrun",
    version,
    about = "Run finite commands without model polling"
)]
#[command(propagate_version = true)]
pub struct Cli {
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,
    #[arg(long, global = true, value_name = "DURATION")]
    pub timeout: Option<String>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init(InitArgs),
    Uninstall(UninstallArgs),
    Doctor(JsonArgs),
    Gain(GainArgs),
    #[command(hide = true)]
    Hook(HookArgs),
    #[command(hide = true)]
    Internal(InternalArgs),
    #[command(name = "run", hide = true)]
    Run(RemovedArgs),
    #[command(name = "run-shell", hide = true)]
    RunShell(RemovedArgs),
    #[command(name = "submit", hide = true)]
    Submit(RemovedArgs),
    #[command(name = "submit-shell", hide = true)]
    SubmitShell(RemovedArgs),
    #[command(name = "wait", hide = true)]
    Wait(RemovedArgs),
    #[command(name = "status", hide = true)]
    Status(RemovedArgs),
    #[command(name = "list", hide = true)]
    List(RemovedArgs),
    #[command(name = "logs", hide = true)]
    Logs(RemovedArgs),
    #[command(name = "cancel", hide = true)]
    Cancel(RemovedArgs),
    #[command(name = "gc", hide = true)]
    Gc(RemovedArgs),
    #[command(name = "daemon", hide = true)]
    Daemon(RemovedArgs),
    #[command(name = "service", hide = true)]
    Service(RemovedArgs),
    #[command(name = "mcp", hide = true)]
    Mcp(RemovedArgs),
    #[command(external_subcommand)]
    Target(Vec<OsString>),
}

#[derive(Debug, Args)]
pub struct RemovedArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<OsString>,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long, required = true)]
    pub codex: bool,
    #[arg(long)]
    pub repair: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct UninstallArgs {
    #[arg(long, required = true)]
    pub codex: bool,
    #[arg(long)]
    pub purge_data: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct JsonArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct GainArgs {
    #[arg(long)]
    pub clear: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct InternalArgs {
    #[command(subcommand)]
    pub command: InternalCommand,
}

#[derive(Debug, Subcommand)]
pub enum InternalCommand {
    Receipt {
        #[arg(long, hide = true)]
        handoff_id: String,
    },
}

#[derive(Debug, Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub command: HookCommand,
}

#[derive(Debug, Subcommand)]
pub enum HookCommand {
    Codex(CodexHookArgs),
}

#[derive(Debug, Args)]
pub struct CodexHookArgs {
    #[command(subcommand)]
    pub command: CodexHookCommand,
}

#[derive(Debug, Subcommand)]
pub enum CodexHookCommand {
    PreToolUse,
    PostToolUse,
}

impl Cli {
    pub fn is_receipt(&self) -> bool {
        matches!(
            &self.command,
            Command::Internal(InternalArgs {
                command: InternalCommand::Receipt { .. }
            })
        )
    }
}

pub async fn dispatch(
    cli: Cli,
    paths: &AppPaths,
    config: &Config,
    _config_path: &std::path::Path,
) -> Result<ExitCode> {
    let global_json = cli.json;
    let globals = GlobalsForTarget {
        timeout: cli.timeout,
    };
    match cli.command {
        Command::Target(words) => {
            let target = target_from_words(words, &globals, config)?;
            execute_target(&target, paths, config, global_json).await
        }
        Command::Init(arguments) => {
            let report = codex::init(paths, &std::env::current_exe()?, arguments.repair)?;
            if arguments.json {
                serde_json::to_writer(std::io::stdout(), &report)?;
                std::io::stdout().write_all(b"\n")?;
            } else {
                println!(
                    "Installed {} at {}. Review and trust the generated hooks in Codex with /hooks.",
                    report.plugin_selector,
                    report.generated_root.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Uninstall(arguments) => {
            let report = codex::uninstall(paths)?;
            if arguments.purge_data {
                match std::fs::remove_dir_all(&paths.data_dir) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if arguments.json {
                serde_json::to_writer(
                    std::io::stdout(),
                    &serde_json::json!({
                        "generated_root": report.generated_root,
                        "removed_files": report.removed_files,
                        "purged_data": arguments.purge_data,
                    }),
                )?;
                std::io::stdout().write_all(b"\n")?;
            } else {
                println!(
                    "Uninstalled Longrun Codex integration (removed {} generated files).",
                    report.removed_files
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor(arguments) => {
            let report = codex::doctor(paths, config).await;
            codex::write_doctor(&report, arguments.json)?;
            Ok(if report.healthy {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Gain(arguments) => gain(arguments, paths, global_json).await,
        Command::Hook(arguments) => hook(arguments, paths, config).await,
        Command::Internal(arguments) => internal(arguments, paths).await,
        Command::Run(_)
        | Command::RunShell(_)
        | Command::Submit(_)
        | Command::SubmitShell(_)
        | Command::Wait(_)
        | Command::Status(_)
        | Command::List(_)
        | Command::Logs(_)
        | Command::Cancel(_)
        | Command::Gc(_)
        | Command::Daemon(_)
        | Command::Service(_)
        | Command::Mcp(_) => Err(Error::InvalidInput(
            "removed command; use `longrun -- PROGRAM ARG...` (or `rtk longrun -- PROGRAM ARG...`)"
                .into(),
        )),
    }
}

pub fn is_management_command(program: &OsStr) -> bool {
    matches!(
        program.to_str(),
        Some(
            "init"
                | "uninstall"
                | "doctor"
                | "gain"
                | "hook"
                | "internal"
                | "run"
                | "run-shell"
                | "submit"
                | "submit-shell"
                | "wait"
                | "status"
                | "list"
                | "logs"
                | "cancel"
                | "gc"
                | "daemon"
                | "service"
                | "mcp"
        )
    )
}

pub fn target_from_words(
    words: Vec<OsString>,
    globals: &GlobalsForTarget,
    config: &Config,
) -> Result<TargetSpec> {
    let cwd = NativeString::from_os_string(
        std::fs::canonicalize(std::env::current_dir()?)?.into_os_string(),
    );
    target_from_words_at_cwd(words, globals, config, cwd, now_ms()?)
}

pub fn target_from_words_at_cwd(
    mut words: Vec<OsString>,
    globals: &GlobalsForTarget,
    config: &Config,
    cwd: NativeString,
    created_at_ms: i64,
) -> Result<TargetSpec> {
    if words.first().is_some_and(|word| word == OsStr::new("--")) {
        words.remove(0);
    }
    let program = words
        .first()
        .cloned()
        .ok_or_else(|| Error::InvalidInput("missing target program".into()))?;
    let args = words
        .into_iter()
        .skip(1)
        .map(NativeString::from_os_string)
        .collect::<Vec<_>>();
    let timeout_ms = globals
        .timeout
        .as_deref()
        .map(parse_duration_ms)
        .transpose()?
        .unwrap_or(config.execution.timeout_ms);
    let command_hash = sha256_hex(&serde_json::to_vec(&(&program, &args, &cwd, timeout_ms))?);
    Ok(TargetSpec {
        protocol_version: crate::protocol::PROTOCOL_VERSION,
        program: NativeString::from_os_string(program),
        args,
        cwd,
        timeout_ms,
        created_at_ms,
        command_hash,
    })
}

#[derive(Debug, Clone, Default)]
pub struct GlobalsForTarget {
    pub timeout: Option<String>,
}

async fn execute_target(
    target: &TargetSpec,
    paths: &AppPaths,
    config: &Config,
    json: bool,
) -> Result<ExitCode> {
    let result = Runner::new()
        .execute(
            target,
            config,
            paths,
            ExecutionMode::Direct,
            if json {
                OutputMode::Capture
            } else {
                OutputMode::Passthrough
            },
        )
        .await?;
    if let Err(error) = metrics::record(paths, target, ExecutionMode::Direct, &result) {
        eprintln!("longrun: warning: could not record metrics: {error}");
    }
    if json {
        serde_json::to_writer(std::io::stdout(), &result)?;
        std::io::stdout().write_all(b"\n")?;
    }
    Ok(result_exit_code(&result))
}

async fn gain(arguments: GainArgs, paths: &AppPaths, global_json: bool) -> Result<ExitCode> {
    let json = global_json || arguments.json;
    if arguments.clear {
        metrics::clear(paths)?;
        if json {
            serde_json::to_writer(std::io::stdout(), &serde_json::json!({"cleared": true}))?;
            std::io::stdout().write_all(b"\n")?;
        } else {
            println!("Cleared Longrun execution metrics.");
        }
        return Ok(ExitCode::SUCCESS);
    }

    let report = metrics::read_report(paths)?;
    if json {
        serde_json::to_writer(std::io::stdout(), &report)?;
        std::io::stdout().write_all(b"\n")?;
    } else {
        metrics::write_human_report(&report, &mut std::io::stdout())?;
    }
    Ok(ExitCode::SUCCESS)
}

fn result_exit_code(result: &ResultEnvelope) -> ExitCode {
    match result.terminal_reason {
        TerminalReason::Exited => {
            ExitCode::from(result.exit_code.unwrap_or(70).clamp(0, 255) as u8)
        }
        TerminalReason::TimedOut => ExitCode::from(124),
        TerminalReason::Cancelled | TerminalReason::OwnerShutdown => ExitCode::from(130),
        TerminalReason::SpawnFailed => ExitCode::from(127),
    }
}

async fn internal(arguments: InternalArgs, paths: &AppPaths) -> Result<ExitCode> {
    match arguments.command {
        InternalCommand::Receipt { handoff_id } => {
            let receipt = HandoffStore::new(paths)
                .arm(&handoff_id, now_ms()?)?
                .ok_or_else(|| {
                    Error::Denied("handoff is missing, expired, or already armed".into())
                })?;
            println!("{receipt}");
            Ok(ExitCode::SUCCESS)
        }
    }
}

async fn hook(arguments: HookArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    match arguments.command {
        HookCommand::Codex(arguments) => match arguments.command {
            CodexHookCommand::PreToolUse => {
                let mut source = String::new();
                std::io::stdin().read_to_string(&mut source)?;
                let input: PreToolUseInput = serde_json::from_str(&source)?;
                if let Some(output) = handle_pre_tool_use(
                    &input,
                    &std::env::current_exe()?,
                    paths,
                    config,
                    now_ms()?,
                )? {
                    serde_json::to_writer(std::io::stdout(), &output)?;
                    std::io::stdout().write_all(b"\n")?;
                }
                Ok(ExitCode::SUCCESS)
            }
            CodexHookCommand::PostToolUse => {
                let mut source = String::new();
                std::io::stdin().read_to_string(&mut source)?;
                let input: PostToolUseInput = serde_json::from_str(&source)?;
                if let Some(output) =
                    handle_post_tool_use(&input, paths, config, &Runner::new()).await?
                {
                    serde_json::to_writer(std::io::stdout(), &output)?;
                    std::io::stdout().write_all(b"\n")?;
                }
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

pub fn globals_from_cli(cli: &Cli) -> GlobalsForTarget {
    GlobalsForTarget {
        timeout: cli.timeout.clone(),
    }
}

pub(crate) fn parse_duration_ms(value: &str) -> Result<u64> {
    let (amount, multiplier) = match value.chars().last() {
        Some('s') => (&value[..value.len() - 1], 1_000),
        Some('m') => (&value[..value.len() - 1], 60_000),
        Some('h') => (&value[..value.len() - 1], 3_600_000),
        _ => (value, 1),
    };
    let amount = amount
        .parse::<u64>()
        .map_err(|_| Error::InvalidInput(format!("invalid duration: {value}")))?;
    amount
        .checked_mul(multiplier)
        .filter(|duration| *duration > 0)
        .ok_or_else(|| Error::InvalidInput(format!("invalid duration: {value}")))
}
