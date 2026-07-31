use std::ffi::OsString;

use clap::Parser;
use longrun::cli::{Cli, Command};

#[test]
fn direct_program_arguments_remain_literal_after_separator() {
    let cli = Cli::try_parse_from([
        "longrun",
        "run",
        "--timeout",
        "1000",
        "--",
        "echo",
        "--not-an-option",
    ])
    .expect("parse command");

    let Command::Run(arguments) = cli.command else {
        panic!("expected run command");
    };
    assert_eq!(
        arguments.program,
        vec![OsString::from("echo"), OsString::from("--not-an-option")]
    );
    assert_eq!(arguments.timeout.as_deref(), Some("1000"));
}

#[test]
fn hook_and_internal_worker_commands_are_parsed_but_hidden_from_normal_workflows() {
    let hook = Cli::try_parse_from(["longrun", "hook", "codex", "pre-tool-use"])
        .expect("parse hook command");
    assert!(matches!(hook.command, Command::Hook(_)));

    let worker = Cli::try_parse_from([
        "longrun",
        "internal",
        "worker",
        "018ef4f8-0000-7000-8000-000000000001",
    ])
    .expect("parse worker command");
    assert!(matches!(worker.command, Command::Internal(_)));
}

#[test]
fn every_documented_command_has_a_parse_shape() {
    let job = "018ef4f8-0000-7000-8000-000000000001";
    let cases: &[&[&str]] = &[
        &["longrun", "submit", "--", "echo", "ok"],
        &["longrun", "run-shell", "--script", "echo ok"],
        &["longrun", "submit-shell", "--script", "echo ok"],
        &["longrun", "wait", job],
        &["longrun", "status", job],
        &["longrun", "list", "--state", "running"],
        &["longrun", "logs", job, "--stderr"],
        &["longrun", "cancel", job, "--grace", "1s"],
        &["longrun", "gc", "--dry-run"],
        &["longrun", "init", "--codex"],
        &["longrun", "uninstall", "--codex"],
        &["longrun", "doctor", "--json"],
        &["longrun", "daemon", "--foreground"],
        &["longrun", "service", "status"],
        &["longrun", "mcp"],
    ];

    for arguments in cases {
        Cli::try_parse_from(*arguments).unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
    }
}
