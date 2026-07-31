use std::{ffi::OsString, path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand, ValueEnum};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    paths::AppPaths,
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

pub fn dispatch(cli: Cli, _paths: &AppPaths, _config: &Config) -> Result<ExitCode> {
    Err(Error::Unavailable(format!(
        "`longrun {}` is not available until the runtime is initialized",
        cli.command.name()
    )))
}
