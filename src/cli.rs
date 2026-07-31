use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    hook::{
        input::PreToolUseInput,
        pre_tool_use::{handle_pre_tool_use, now_ms},
    },
    paths::AppPaths,
    protocol::{
        EnvironmentPolicy, ExecutionMode, JobSpecification, NativeString, ShellMode, sha256_hex,
    },
    receipt::{ReceiptPayload, ReceiptSigner},
    store::Store,
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
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(ExecutionArgs),
    Submit(SubmitArgs),
    RunShell(ShellArgs),
    SubmitShell(SubmitShellArgs),
    Wait(JobArgs),
    Status(JobArgs),
    List(ListArgs),
    Logs(LogsArgs),
    Cancel(CancelArgs),
    Gc(GcArgs),
    Init(InitArgs),
    Uninstall(UninstallArgs),
    Doctor(JsonArgs),
    Daemon(DaemonArgs),
    Service(ServiceArgs),
    #[command(hide = true)]
    Internal(InternalArgs),
    #[command(hide = true)]
    Hook(HookArgs),
    Mcp,
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Run(_) => "run",
            Self::Submit(_) => "submit",
            Self::RunShell(_) => "run-shell",
            Self::SubmitShell(_) => "submit-shell",
            Self::Wait(_) => "wait",
            Self::Status(_) => "status",
            Self::List(_) => "list",
            Self::Logs(_) => "logs",
            Self::Cancel(_) => "cancel",
            Self::Gc(_) => "gc",
            Self::Init(_) => "init",
            Self::Uninstall(_) => "uninstall",
            Self::Doctor(_) => "doctor",
            Self::Daemon(_) => "daemon",
            Self::Service(_) => "service",
            Self::Internal(_) => "internal",
            Self::Hook(_) => "hook",
            Self::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModeArg {
    Embedded,
    Durable,
}

#[derive(Debug, Args)]
pub struct ExecutionArgs {
    #[arg(long, value_name = "DURATION")]
    pub timeout: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub permission_profile: Option<String>,
    #[arg(long = "env-pass", value_name = "NAME")]
    pub env_pass: Vec<String>,
    #[arg(long, value_enum)]
    pub mode: Option<ModeArg>,
    #[arg(long)]
    pub json: bool,
    #[arg(
        last = true,
        required = true,
        allow_hyphen_values = true,
        value_name = "PROGRAM ARG..."
    )]
    pub program: Vec<OsString>,
}

#[derive(Debug, Args)]
pub struct SubmitArgs {
    #[command(flatten)]
    pub execution: ExecutionArgs,
    #[arg(long, hide = true, value_name = "TOKEN")]
    pub hook_token: Option<String>,
}

#[derive(Debug, Args)]
pub struct ShellArgs {
    #[arg(long, value_name = "SCRIPT")]
    pub script: String,
    #[arg(long, value_name = "DURATION")]
    pub timeout: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub permission_profile: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SubmitShellArgs {
    #[command(flatten)]
    pub shell: ShellArgs,
    #[arg(long, hide = true, value_name = "TOKEN")]
    pub hook_token: Option<String>,
}

#[derive(Debug, Args)]
pub struct JobArgs {
    pub job_id: Uuid,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long)]
    pub state: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    pub job_id: Uuid,
    #[arg(long)]
    pub follow: bool,
    #[arg(long)]
    pub stderr: bool,
}

#[derive(Debug, Args)]
pub struct CancelArgs {
    pub job_id: Uuid,
    #[arg(long, value_name = "DURATION")]
    pub grace: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct GcArgs {
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub json: bool,
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
pub struct DaemonArgs {
    #[arg(long)]
    pub foreground: bool,
}

#[derive(Debug, Args)]
pub struct ServiceArgs {
    #[command(subcommand)]
    pub command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
}

#[derive(Debug, Args)]
pub struct InternalArgs {
    #[command(subcommand)]
    pub command: InternalCommand,
}

#[derive(Debug, Subcommand)]
pub enum InternalCommand {
    Worker { job_id: Uuid },
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
    SessionStart,
}

pub fn dispatch(cli: Cli, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    match cli.command {
        Command::Submit(arguments) => submit(arguments, paths, config),
        Command::SubmitShell(arguments) => submit_shell(arguments, paths, config),
        Command::Hook(arguments) => hook(arguments, paths),
        command => Err(Error::Unavailable(format!(
            "`longrun {}` is not available until the runtime is initialized",
            command.name()
        ))),
    }
}

fn submit(arguments: SubmitArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    let token = arguments
        .hook_token
        .ok_or_else(|| Error::Denied("`submit` requires a hook-issued token".into()))?;
    let cwd = NativeString::from_os_string(std::env::current_dir()?.into_os_string());
    let program = NativeString::from_os_string(
        arguments
            .execution
            .program
            .first()
            .cloned()
            .ok_or_else(|| Error::InvalidInput("missing submitted program".into()))?,
    );
    let args = arguments
        .execution
        .program
        .into_iter()
        .skip(1)
        .map(NativeString::from_os_string)
        .collect::<Vec<_>>();
    let job = JobSpecification {
        protocol_version: crate::protocol::PROTOCOL_VERSION,
        job_id: Uuid::now_v7(),
        program,
        args,
        cwd,
        execution_mode: arguments.execution.mode.map_or(
            ExecutionMode::Embedded,
            |mode| match mode {
                ModeArg::Embedded => ExecutionMode::Embedded,
                ModeArg::Durable => ExecutionMode::Durable,
            },
        ),
        shell_mode: ShellMode::Direct,
        timeout_ms: arguments
            .execution
            .timeout
            .as_deref()
            .map(parse_duration_ms)
            .transpose()?
            .unwrap_or(config.execution.timeout_ms),
        permission_profile: arguments
            .execution
            .permission_profile
            .unwrap_or_else(|| config.execution.permission_profile.clone()),
        environment_policy: EnvironmentPolicy {
            pass: config
                .environment
                .pass
                .iter()
                .chain(arguments.execution.env_pass.iter())
                .cloned()
                .collect(),
            deny_patterns: config.environment.deny_patterns.clone(),
        },
        created_at_ms: now_ms()?,
        command_hash: String::new(),
    };
    issue_submission(token, job, paths)
}

fn submit_shell(arguments: SubmitShellArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    let token = arguments
        .hook_token
        .ok_or_else(|| Error::Denied("`submit-shell` requires a hook-issued token".into()))?;
    let cwd = NativeString::from_os_string(std::env::current_dir()?.into_os_string());
    let job = JobSpecification {
        protocol_version: crate::protocol::PROTOCOL_VERSION,
        job_id: Uuid::now_v7(),
        program: NativeString {
            encoding: crate::protocol::NativeEncoding::Utf8,
            value: "longrun-shell".into(),
        },
        args: vec![NativeString {
            encoding: crate::protocol::NativeEncoding::Utf8,
            value: arguments.shell.script,
        }],
        cwd,
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::ExplicitShell,
        timeout_ms: arguments
            .shell
            .timeout
            .as_deref()
            .map(parse_duration_ms)
            .transpose()?
            .unwrap_or(config.execution.timeout_ms),
        permission_profile: arguments
            .shell
            .permission_profile
            .unwrap_or_else(|| config.execution.permission_profile.clone()),
        environment_policy: EnvironmentPolicy {
            pass: config.environment.pass.clone(),
            deny_patterns: config.environment.deny_patterns.clone(),
        },
        created_at_ms: now_ms()?,
        command_hash: String::new(),
    };
    issue_submission(token, job, paths)
}

fn issue_submission(
    token: String,
    mut job: JobSpecification,
    paths: &AppPaths,
) -> Result<ExitCode> {
    let mut store = Store::open(paths.state_dir.join("longrun.sqlite"))?;
    let pending = store.claim_pending_by_token(&sha256_hex(token.as_bytes()), now_ms()?)?;
    job.command_hash = pending.command_hash.clone();
    if !pending.matches_job(&job) {
        return Err(Error::Denied(
            "submitted command does not match the hook-approved request".into(),
        ));
    }
    let signer = ReceiptSigner::load_or_create(&paths.state_dir.join("receipt.key"))?;
    let issued = OffsetDateTime::now_utc();
    let payload = ReceiptPayload::from_job(
        job,
        pending.session_id,
        pending.turn_id,
        pending.tool_use_id,
        issued
            .format(&Rfc3339)
            .map_err(|error| Error::Unavailable(format!("cannot format receipt time: {error}")))?,
        (issued + Duration::minutes(5))
            .format(&Rfc3339)
            .map_err(|error| {
                Error::Unavailable(format!("cannot format receipt expiry: {error}"))
            })?,
        ReceiptSigner::random_nonce()?,
    );
    let line = signer.issue(&payload)?.to_line();
    std::io::stdout().write_all(line.as_bytes())?;
    std::io::stdout().write_all(b"\n")?;
    Ok(ExitCode::SUCCESS)
}

fn hook(arguments: HookArgs, paths: &AppPaths) -> Result<ExitCode> {
    match arguments.command {
        HookCommand::Codex(arguments) => match arguments.command {
            CodexHookCommand::PreToolUse => {
                let mut source = String::new();
                std::io::stdin().read_to_string(&mut source)?;
                let input: PreToolUseInput = serde_json::from_str(&source)?;
                let mut store = Store::open(paths.state_dir.join("longrun.sqlite"))?;
                if let Some(output) =
                    handle_pre_tool_use(&input, &std::env::current_exe()?, &mut store, now_ms()?)?
                {
                    serde_json::to_writer(std::io::stdout(), &output)?;
                    std::io::stdout().write_all(b"\n")?;
                }
                Ok(ExitCode::SUCCESS)
            }
            CodexHookCommand::PostToolUse | CodexHookCommand::SessionStart => {
                Err(Error::Unavailable(
                    "Codex post-tool-use recovery runtime is not initialized".into(),
                ))
            }
        },
    }
}

fn parse_duration_ms(value: &str) -> Result<u64> {
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
