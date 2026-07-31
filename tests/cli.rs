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

    fn command(root: &std::path::Path) -> Command {
        let path = format!(
            "{}:{}",
            root.join("bin").display(),
            std::env::var("PATH").expect("PATH")
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_longrun"));
        command.env("HOME", root.join("home")).env("PATH", path);
        command
    }

    fn run(
        root: &std::path::Path,
        arguments: impl IntoIterator<Item = OsString>,
    ) -> std::process::Output {
        command(root).args(arguments).output().expect("run longrun")
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
    fn direct_run_inherits_only_explicitly_allowed_environment() {
        let (root, _) = setup();
        let allowed = format!("LONGRUN_ALLOWED_TOKEN_{}", Uuid::now_v7().simple());
        let blocked = format!("LONGRUN_BLOCKED_SECRET_{}", Uuid::now_v7().simple());
        let script =
            format!("printf '%s|%s' \"${{{allowed}:-missing}}\" \"${{{blocked}:-missing}}\"");
        let output = command(&root)
            .args([
                OsString::from("run"),
                OsString::from("--env-pass"),
                OsString::from(&allowed),
                OsString::from("--"),
                OsString::from("/bin/sh"),
                OsString::from("-c"),
                OsString::from(script),
            ])
            .env(&allowed, "allowed")
            .env(&blocked, "blocked")
            .output()
            .expect("run longrun");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"allowed|missing");
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

    #[test]
    fn completed_jobs_support_status_list_wait_and_byte_safe_logs() {
        let (root, _) = setup();
        let result = run(
            &root,
            [
                "run",
                "--json",
                "--",
                "/bin/sh",
                "-c",
                "printf out; printf err >&2",
            ]
            .map(OsString::from),
        );
        assert_eq!(result.status.code(), Some(0));
        let job_id = serde_json::from_slice::<serde_json::Value>(&result.stdout)
            .expect("result json")["job_id"]
            .as_str()
            .expect("job id")
            .to_owned();

        let status = run(&root, ["status", "--json", &job_id].map(OsString::from));
        assert_eq!(status.status.code(), Some(0));
        let status = serde_json::from_slice::<serde_json::Value>(&status.stdout).expect("status");
        assert_eq!(status["execution_state"], "succeeded");
        assert_eq!(status["delivery_state"], "delivered_in_turn");

        let list = run(&root, ["list", "--json"].map(OsString::from));
        assert_eq!(list.status.code(), Some(0));
        assert!(
            serde_json::from_slice::<serde_json::Value>(&list.stdout).expect("list")[0]["job_id"]
                == job_id
        );

        let wait = run(&root, ["wait", "--json", &job_id].map(OsString::from));
        assert_eq!(wait.status.code(), Some(0));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&wait.stdout).expect("wait")["job_id"],
            job_id
        );
        assert_eq!(
            run(&root, ["logs", &job_id].map(OsString::from)).stdout,
            b"out"
        );
        assert_eq!(
            run(
                &root,
                ["logs", "--stderr", "--follow", &job_id].map(OsString::from)
            )
            .stdout,
            b"err"
        );

        let binary = run(
            &root,
            ["run", "--json", "--", "/bin/sh", "-c", "printf '\\377'"].map(OsString::from),
        );
        assert_eq!(binary.status.code(), Some(0));
        let binary_id = serde_json::from_slice::<serde_json::Value>(&binary.stdout)
            .expect("binary result")["job_id"]
            .as_str()
            .expect("binary job id")
            .to_owned();
        assert_eq!(
            run(&root, ["logs", &binary_id].map(OsString::from)).stdout,
            [0xff]
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
