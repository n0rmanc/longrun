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

#[cfg(unix)]
mod integration {
    use std::{
        ffi::OsString,
        fs,
        os::unix::{ffi::OsStringExt, fs::PermissionsExt},
        process::Command,
    };
    use uuid::Uuid;

    fn setup() -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("longrun-cli-{}", Uuid::now_v7()));
        fs::create_dir_all(root.join("bin")).expect("root");
        let codex = root.join("bin/codex");
        fs::write(
            &codex,
            "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
        )
        .expect("script");
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("mode");
        (root, codex)
    }

    fn run(
        root: &std::path::Path,
        arguments: impl IntoIterator<Item = OsString>,
    ) -> std::process::Output {
        let path = format!(
            "{}:{}",
            root.join("bin").display(),
            std::env::var("PATH").expect("PATH")
        );
        Command::new(env!("CARGO_BIN_EXE_longrun"))
            .args(arguments)
            .env("HOME", root.join("home"))
            .env("PATH", path)
            .output()
            .expect("run longrun")
    }

    #[test]
    fn direct_run_preserves_exit_status_and_streams() {
        let (root, _) = setup();
        let output = run(
            &root,
            [
                "run",
                "--",
                "/bin/sh",
                "-c",
                "printf out; printf err >&2; exit 7",
            ]
            .map(OsString::from),
        );
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn direct_run_times_out_and_preserves_non_utf8_arguments() {
        let (root, _) = setup();
        let timeout = run(
            &root,
            ["run", "--timeout", "25", "--", "/bin/sh", "-c", "sleep 1"].map(OsString::from),
        );
        assert_eq!(timeout.status.code(), Some(124));

        let output = run(
            &root,
            vec![
                OsString::from("run"),
                OsString::from("--"),
                OsString::from("/bin/echo"),
                OsString::from_vec(vec![0xff]),
            ],
        );
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, vec![0xff, b'\n']);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
