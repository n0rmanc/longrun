use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    hook::{
        input::{PostToolUseInput, PreToolUseInput, SessionStartInput},
        post_tool_use::handle_post_tool_use,
        pre_tool_use::{handle_pre_tool_use, now_ms},
        session_start::handle_session_start,
    },
    output::read_log,
    paths::AppPaths,
    protocol::{
        EnvironmentPolicy, ExecutionMode, JobSpecification, NativeString, ShellMode, sha256_hex,
    },
    receipt::{ReceiptPayload, ReceiptSigner},
    store::Store,
    supervisor::{self, Supervisor},
    worker::run_worker,
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
    Worker {
        job_id: Uuid,
        #[arg(long, hide = true)]
        state_dir: Option<PathBuf>,
        #[arg(long, hide = true)]
        log_dir: Option<PathBuf>,
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
    SessionStart,
}

pub async fn dispatch(
    cli: Cli,
    paths: &AppPaths,
    config: &Config,
    config_path: &std::path::Path,
) -> Result<ExitCode> {
    match cli.command {
        Command::Submit(arguments) => submit(arguments, paths, config),
        Command::SubmitShell(arguments) => submit_shell(arguments, paths, config),
        Command::Hook(arguments) => hook(arguments, paths, config).await,
        Command::Run(arguments) => run(arguments, paths, config).await,
        Command::RunShell(arguments) => run_shell(arguments, paths, config).await,
        Command::Internal(arguments) => internal(arguments, paths, config).await,
        Command::Wait(arguments) => wait(arguments, paths).await,
        Command::Status(arguments) => status(arguments, paths),
        Command::List(arguments) => list(arguments, paths),
        Command::Logs(arguments) => logs(arguments, paths).await,
        Command::Cancel(arguments) => cancel(arguments, paths, config),
        Command::Gc(arguments) => gc(arguments, paths, config),
        Command::Daemon(arguments) => daemon(arguments, paths, config, config_path).await,
        command => Err(Error::Unavailable(format!(
            "`longrun {}` is not available until the runtime is initialized",
            command.name()
        ))),
    }
}

fn status(arguments: JobArgs, paths: &AppPaths) -> Result<ExitCode> {
    let status = Store::open(paths.state_dir.join("longrun.sqlite"))?.status(arguments.job_id)?;
    write_status(&status, arguments.json)?;
    Ok(ExitCode::SUCCESS)
}

fn list(arguments: ListArgs, paths: &AppPaths) -> Result<ExitCode> {
    let state = arguments.state.as_deref().map(str::parse).transpose()?;
    let jobs = Store::open(paths.state_dir.join("longrun.sqlite"))?.list(state)?;
    if arguments.json {
        serde_json::to_writer(std::io::stdout(), &jobs)?;
        std::io::stdout().write_all(b"\n")?;
    } else {
        for job in jobs {
            println!(
                "{}\t{}\t{}",
                job.job_id,
                job.execution_state.as_str(),
                job.delivery_state.as_str()
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn wait(arguments: JobArgs, paths: &AppPaths) -> Result<ExitCode> {
    loop {
        let status =
            Store::open(paths.state_dir.join("longrun.sqlite"))?.status(arguments.job_id)?;
        if status.execution_state.is_terminal() {
            write_status(&status, arguments.json)?;
            return Ok(status
                .result
                .as_ref()
                .map_or(ExitCode::from(70), result_exit_code));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

async fn logs(arguments: LogsArgs, paths: &AppPaths) -> Result<ExitCode> {
    let suffix = if arguments.stderr { "stderr" } else { "stdout" };
    let path = paths
        .log_dir
        .join(format!("{}.{}.log", arguments.job_id, suffix));
    let mut offset = 0usize;
    loop {
        let bytes = read_log(&path).await?;
        if bytes.len() > offset {
            std::io::stdout().write_all(&bytes[offset..])?;
            offset = bytes.len();
        }
        if !arguments.follow
            || Store::open(paths.state_dir.join("longrun.sqlite"))?
                .status(arguments.job_id)?
                .execution_state
                .is_terminal()
        {
            return Ok(ExitCode::SUCCESS);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

fn cancel(arguments: CancelArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    let grace_ms = arguments
        .grace
        .as_deref()
        .map(parse_duration_ms)
        .transpose()?
        .unwrap_or(config.execution.termination_grace_ms);
    let requested = Store::open(paths.state_dir.join("longrun.sqlite"))?.request_cancellation(
        arguments.job_id,
        grace_ms,
        now_ms()?,
    )?;
    if arguments.json {
        serde_json::to_writer(
            std::io::stdout(),
            &serde_json::json!({
                "job_id": arguments.job_id,
                "cancellation_requested": requested,
            }),
        )?;
        std::io::stdout().write_all(b"\n")?;
    } else if requested {
        println!("Cancellation requested for {}", arguments.job_id);
    } else {
        println!("Job {} is already terminal or cancelling", arguments.job_id);
    }
    Ok(ExitCode::SUCCESS)
}

fn gc(arguments: GcArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    let mut store = Store::open(paths.state_dir.join("longrun.sqlite"))?;
    let candidates = store.retention_candidates(
        now_ms()?,
        config.retention.max_age_days,
        config.retention.max_log_bytes,
    )?;
    if !arguments.dry_run {
        for candidate in &candidates {
            for path in [&candidate.stdout_log, &candidate.stderr_log] {
                let path = PathBuf::from(path.to_os_string()?);
                if !path.starts_with(&paths.log_dir) {
                    return Err(Error::InvalidInput(
                        "job log path is outside Longrun state".into(),
                    ));
                }
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        store.delete_jobs(&candidates.iter().map(|job| job.job_id).collect::<Vec<_>>())?;
    }
    if arguments.json {
        serde_json::to_writer(
            std::io::stdout(),
            &serde_json::json!({
                "dry_run": arguments.dry_run,
                "job_ids": candidates.iter().map(|job| job.job_id).collect::<Vec<_>>(),
            }),
        )?;
        std::io::stdout().write_all(b"\n")?;
    } else if arguments.dry_run {
        println!("Would remove {} job(s)", candidates.len());
    } else {
        println!("Removed {} job(s)", candidates.len());
    }
    Ok(ExitCode::SUCCESS)
}

fn write_status(status: &crate::store::JobStatus, json: bool) -> Result<()> {
    if json {
        serde_json::to_writer(std::io::stdout(), status)?;
        std::io::stdout().write_all(b"\n")?;
    } else {
        println!(
            "Job: {}\nExecution: {}\nDelivery: {}",
            status.job_id,
            status.execution_state.as_str(),
            status.delivery_state.as_str()
        );
    }
    Ok(())
}

async fn run(arguments: ExecutionArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    let cwd = NativeString::from_os_string(std::env::current_dir()?.into_os_string());
    let program = NativeString::from_os_string(
        arguments
            .program
            .first()
            .cloned()
            .ok_or_else(|| Error::InvalidInput("missing program".into()))?,
    );
    let args = arguments
        .program
        .into_iter()
        .skip(1)
        .map(NativeString::from_os_string)
        .collect::<Vec<_>>();
    let mode = arguments
        .mode
        .map_or(ExecutionMode::Embedded, |mode| match mode {
            ModeArg::Embedded => ExecutionMode::Embedded,
            ModeArg::Durable => ExecutionMode::Durable,
        });
    let timeout_ms = arguments
        .timeout
        .as_deref()
        .map(parse_duration_ms)
        .transpose()?
        .unwrap_or(config.execution.timeout_ms);
    let permission_profile = arguments
        .permission_profile
        .unwrap_or_else(|| config.execution.permission_profile.clone());
    let environment_policy = EnvironmentPolicy {
        pass: config
            .environment
            .pass
            .iter()
            .chain(arguments.env_pass.iter())
            .cloned()
            .collect(),
        deny_patterns: config.environment.deny_patterns.clone(),
    };
    let command_hash = sha256_hex(&serde_json::to_vec(&(
        &program,
        &args,
        &cwd,
        timeout_ms,
        &permission_profile,
        &environment_policy,
    ))?);
    let job = JobSpecification {
        protocol_version: crate::protocol::PROTOCOL_VERSION,
        job_id: Uuid::now_v7(),
        program,
        args,
        cwd,
        execution_mode: mode,
        shell_mode: ShellMode::Direct,
        timeout_ms,
        permission_profile,
        environment_policy,
        created_at_ms: now_ms()?,
        command_hash,
    };
    if mode == ExecutionMode::Durable {
        execute_durable(job, paths, arguments.json).await
    } else {
        execute_direct(job, paths, config, arguments.json).await
    }
}

async fn run_shell(arguments: ShellArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    if !config.execution.allow_shell {
        return Err(Error::Denied(
            "shell execution requires execution.allow_shell = true".into(),
        ));
    }
    let cwd = NativeString::from_os_string(std::env::current_dir()?.into_os_string());
    let program = NativeString {
        encoding: crate::protocol::NativeEncoding::Utf8,
        value: "longrun-shell".into(),
    };
    let args = vec![NativeString {
        encoding: crate::protocol::NativeEncoding::Utf8,
        value: arguments.script,
    }];
    let timeout_ms = arguments
        .timeout
        .as_deref()
        .map(parse_duration_ms)
        .transpose()?
        .unwrap_or(config.execution.timeout_ms);
    let permission_profile = arguments
        .permission_profile
        .unwrap_or_else(|| config.execution.permission_profile.clone());
    let environment_policy = EnvironmentPolicy {
        pass: config.environment.pass.clone(),
        deny_patterns: config.environment.deny_patterns.clone(),
    };
    let command_hash = sha256_hex(&serde_json::to_vec(&(
        &program,
        &args,
        &cwd,
        timeout_ms,
        &permission_profile,
        &environment_policy,
    ))?);
    let job = JobSpecification {
        protocol_version: crate::protocol::PROTOCOL_VERSION,
        job_id: Uuid::now_v7(),
        program,
        args,
        cwd,
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::ExplicitShell,
        timeout_ms,
        permission_profile,
        environment_policy,
        created_at_ms: now_ms()?,
        command_hash,
    };
    execute_direct(job, paths, config, arguments.json).await
}

async fn execute_direct(
    job: JobSpecification,
    paths: &AppPaths,
    config: &Config,
    json: bool,
) -> Result<ExitCode> {
    let database = paths.state_dir.join("longrun.sqlite");
    Store::open(&database)?.create_job(&job)?;
    let result = run_worker(job.job_id, &database, config, paths).await?;
    let mut store = Store::open(&database)?;
    store.transition_delivery(job.job_id, crate::protocol::DeliveryState::HookLeased)?;
    store.transition_delivery(job.job_id, crate::protocol::DeliveryState::DeliveredInTurn)?;
    render_direct_result(&result, json).await
}

async fn execute_durable(job: JobSpecification, paths: &AppPaths, json: bool) -> Result<ExitCode> {
    supervisor::submit(paths, &job).await?;
    let status = supervisor::wait(paths, job.job_id).await?;
    let result = status
        .result
        .ok_or_else(|| Error::Unavailable("durable worker completed without a result".into()))?;
    let mut store = Store::open(paths.state_dir.join("longrun.sqlite"))?;
    store.transition_delivery(job.job_id, crate::protocol::DeliveryState::HookLeased)?;
    store.transition_delivery(job.job_id, crate::protocol::DeliveryState::DeliveredInTurn)?;
    render_direct_result(&result, json).await
}

async fn internal(arguments: InternalArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
    match arguments.command {
        InternalCommand::Worker {
            job_id,
            state_dir,
            log_dir,
        } => {
            let mut worker_paths = paths.clone();
            match (state_dir, log_dir) {
                (Some(state_dir), Some(log_dir)) => {
                    worker_paths.state_dir = state_dir;
                    worker_paths.log_dir = log_dir;
                }
                (None, None) => {}
                _ => {
                    return Err(Error::InvalidInput(
                        "internal worker requires both --state-dir and --log-dir".into(),
                    ));
                }
            }
            let result = run_worker(
                job_id,
                &worker_paths.state_dir.join("longrun.sqlite"),
                config,
                &worker_paths,
            )
            .await?;
            Ok(result_exit_code(&result))
        }
    }
}

async fn daemon(
    _arguments: DaemonArgs,
    paths: &AppPaths,
    config: &Config,
    config_path: &std::path::Path,
) -> Result<ExitCode> {
    let worker_path = std::env::var_os("PATH")
        .ok_or_else(|| Error::Unavailable("PATH is unavailable for Longrun workers".into()))?;
    Supervisor::new(
        paths.clone(),
        config,
        std::env::current_exe()?,
        config_path.into(),
        worker_path,
    )?
    .run()
    .await?;
    Ok(ExitCode::SUCCESS)
}

async fn render_direct_result(result: &crate::protocol::JobResult, json: bool) -> Result<ExitCode> {
    if json {
        serde_json::to_writer(std::io::stdout(), result)?;
        std::io::stdout().write_all(b"\n")?;
    } else {
        std::io::stdout().write_all(&tokio::fs::read(result.stdout_log.to_os_string()?).await?)?;
        std::io::stderr().write_all(&tokio::fs::read(result.stderr_log.to_os_string()?).await?)?;
    }
    Ok(result_exit_code(result))
}

fn result_exit_code(result: &crate::protocol::JobResult) -> ExitCode {
    match result.terminal_state {
        crate::protocol::ExecutionState::Succeeded => ExitCode::SUCCESS,
        crate::protocol::ExecutionState::Failed => {
            ExitCode::from(result.exit_code.unwrap_or(70).clamp(1, 255) as u8)
        }
        crate::protocol::ExecutionState::TimedOut => ExitCode::from(124),
        crate::protocol::ExecutionState::Cancelled => ExitCode::from(130),
        _ => ExitCode::from(70),
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
    let expires = OffsetDateTime::from_unix_timestamp_nanos(
        i128::from(pending.expires_at_ms).saturating_mul(1_000_000),
    )
    .map_err(|error| Error::Unavailable(format!("invalid pending expiry: {error}")))?;
    if expires <= issued {
        return Err(Error::Denied("hook token has expired".into()));
    }
    let payload = ReceiptPayload::from_job(
        job,
        pending.session_id,
        pending.turn_id,
        pending.tool_use_id,
        issued
            .format(&Rfc3339)
            .map_err(|error| Error::Unavailable(format!("cannot format receipt time: {error}")))?,
        expires.format(&Rfc3339).map_err(|error| {
            Error::Unavailable(format!("cannot format receipt expiry: {error}"))
        })?,
        ReceiptSigner::random_nonce()?,
    );
    let line = signer.issue(&payload)?.to_line();
    std::io::stdout().write_all(line.as_bytes())?;
    std::io::stdout().write_all(b"\n")?;
    Ok(ExitCode::SUCCESS)
}

async fn hook(arguments: HookArgs, paths: &AppPaths, config: &Config) -> Result<ExitCode> {
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
            CodexHookCommand::PostToolUse => {
                let mut source = String::new();
                std::io::stdin().read_to_string(&mut source)?;
                let input: PostToolUseInput = serde_json::from_str(&source)?;
                if let Some(output) =
                    handle_post_tool_use(&input, paths, config, &crate::runner::Runner::new())
                        .await?
                {
                    serde_json::to_writer(std::io::stdout(), &output)?;
                    std::io::stdout().write_all(b"\n")?;
                }
                Ok(ExitCode::SUCCESS)
            }
            CodexHookCommand::SessionStart => {
                let mut source = String::new();
                std::io::stdin().read_to_string(&mut source)?;
                let input: SessionStartInput = serde_json::from_str(&source)?;
                if let Some(delivery) = handle_session_start(
                    &input,
                    &std::env::current_exe()?,
                    paths,
                    config,
                    now_ms()?,
                )? {
                    let mut stdout = std::io::stdout();
                    serde_json::to_writer(&mut stdout, &delivery.output)?;
                    stdout.write_all(b"\n")?;
                    stdout.flush()?;
                    Store::open(paths.state_dir.join("longrun.sqlite"))?.finish_delivery(
                        delivery.job_id,
                        delivery.lease_id,
                        crate::protocol::DeliveryState::DeliveredOnStart,
                        now_ms()?,
                    )?;
                }
                Ok(ExitCode::SUCCESS)
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
