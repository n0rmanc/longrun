use std::process::ExitCode;

use clap::Parser;
use longrun::{
    cli::{self, Cli},
    config::Config,
    paths::AppPaths,
};

fn init_tracing(filter: &str) {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| filter.to_owned());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .init();
}

async fn run() -> longrun::error::Result<ExitCode> {
    let cli = Cli::parse();
    let paths = AppPaths::discover()?;
    paths.ensure_private_state()?;
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| paths.config_dir.join("config.toml"));
    let config = Config::load(&config_path)?;
    init_tracing(&config.diagnostics.log_level);
    cli::dispatch(cli, &paths, &config).await
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("longrun: {error}");
            error.exit_code()
        }
    }
}
