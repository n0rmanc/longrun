use std::ffi::OsString;

use clap::Parser;
use longrun::cli::{Cli, Command, InternalCommand};

#[test]
fn generic_target_can_start_with_an_explicit_separator() {
    let cli = Cli::try_parse_from(["longrun", "--", "/bin/echo", "--literal", "value"])
        .expect("parse generic target");

    let Command::Target(words) = cli.command else {
        panic!("generic target must not be parsed as a management command");
    };
    assert_eq!(
        words,
        vec![
            OsString::from("/bin/echo"),
            OsString::from("--literal"),
            OsString::from("value"),
        ]
    );
}

#[test]
fn generic_target_without_separator_preserves_target_flags() {
    let cli = Cli::try_parse_from([
        "longrun",
        "gh",
        "run",
        "watch",
        "123",
        "--repo",
        "owner/repo",
        "--exit-status",
    ])
    .expect("parse target");

    let Command::Target(words) = cli.command else {
        panic!("expected external target");
    };
    assert_eq!(words[0], "gh");
    assert_eq!(words.last(), Some(&OsString::from("--exit-status")));
}

#[test]
fn hook_and_receipt_commands_are_hidden_management_paths() {
    let hook =
        Cli::try_parse_from(["longrun", "hook", "codex", "pre-tool-use"]).expect("hook parse");
    assert!(matches!(hook.command, Command::Hook(_)));

    let receipt = Cli::try_parse_from(["longrun", "internal", "receipt", "--handoff-id", "abc123"])
        .expect("receipt parse");
    assert!(matches!(receipt.command, Command::Internal(_)));
    assert!(matches!(
        receipt.command,
        Command::Internal(longrun::cli::InternalArgs {
            command: InternalCommand::Receipt { .. }
        })
    ));
}

#[test]
fn gain_is_a_management_command_and_explicit_separator_keeps_target_form() {
    let gain = Cli::try_parse_from(["longrun", "gain", "--json"]).expect("gain parse");
    assert!(matches!(gain.command, Command::Gain(_)));

    let global_json =
        Cli::try_parse_from(["longrun", "--json", "gain"]).expect("global json gain parse");
    assert!(global_json.json);
    assert!(matches!(global_json.command, Command::Gain(_)));

    let target =
        Cli::try_parse_from(["longrun", "--", "gain", "arg"]).expect("explicit target parse");
    let Command::Target(words) = target.command else {
        panic!("explicit separator must preserve target form");
    };
    assert_eq!(words, vec![OsString::from("gain"), OsString::from("arg")]);
}

#[cfg(unix)]
mod integration {
    use std::{
        ffi::OsString, fs, os::unix::fs::PermissionsExt, process::Command as ProcessCommand,
    };

    use uuid::Uuid;

    fn setup() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("longrun-cli-{}", Uuid::now_v7()));
        fs::create_dir_all(root.join("bin")).expect("root");
        root
    }

    fn command(root: &std::path::Path) -> ProcessCommand {
        let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_longrun"));
        command
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").expect("PATH")
                ),
            );
        command
    }

    fn run(
        root: &std::path::Path,
        arguments: impl IntoIterator<Item = OsString>,
    ) -> std::process::Output {
        command(root).args(arguments).output().expect("run longrun")
    }

    #[test]
    fn generic_target_returns_the_target_exit_status_and_preserves_arguments() {
        let root = setup();
        let output = run(
            &root,
            [
                "--",
                "/bin/sh",
                "-c",
                "printf '%s|%s' \"$1\" \"$2\"; exit 7",
                "ignored",
                "--literal",
                "value",
            ]
            .map(OsString::from),
        );
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"--literal|value");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn direct_target_can_timeout_without_a_persistent_job() {
        let root = setup();
        let legacy = root.join("data/state/longrun.sqlite");
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy state");
        fs::write(&legacy, b"legacy job state").expect("legacy state");
        let output = run(
            &root,
            ["--timeout", "25", "--", "/bin/sh", "-c", "sleep 1"].map(OsString::from),
        );
        assert_eq!(output.status.code(), Some(124));
        assert_eq!(
            fs::read(&legacy).expect("legacy state"),
            b"legacy job state"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn removed_durable_commands_return_a_migration_error() {
        let root = setup();
        for command in [
            &["run", "--", "echo", "old"][..],
            &["run-shell", "--script", "echo old"][..],
            &["submit", "--", "echo", "old"][..],
            &["submit-shell", "--script", "echo old"][..],
            &["wait", "018ef4f8-0000-7000-8000-000000000001"][..],
            &["status", "018ef4f8-0000-7000-8000-000000000001"][..],
            &["list"][..],
            &["logs", "018ef4f8-0000-7000-8000-000000000001"][..],
            &["cancel", "018ef4f8-0000-7000-8000-000000000001"][..],
            &["gc"][..],
            &["daemon"][..],
            &["service", "status"][..],
            &["mcp"][..],
        ] {
            let output = run(&root, command.iter().copied().map(OsString::from));
            assert_eq!(
                output.status.code(),
                Some(2),
                "{command:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("removed"),
                "{command:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn direct_target_copies_only_requested_environment() {
        let root = setup();
        let allowed = format!("LONGRUN_ALLOWED_{}", Uuid::now_v7().simple());
        let blocked = format!("LONGRUN_BLOCKED_SECRET_{}", Uuid::now_v7().simple());
        let script =
            format!("printf '%s|%s' \"${{{allowed}:-missing}}\" \"${{{blocked}:-missing}}\"");
        let output = command(&root)
            .args(["--env-pass", &allowed, "--", "/bin/sh", "-c", &script])
            .env(&allowed, "allowed")
            .env(&blocked, "blocked")
            .output()
            .expect("run longrun");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"allowed|missing");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn direct_arguments_with_shell_metacharacters_remain_literal() {
        let root = setup();
        let marker = root.join("should-not-exist");
        let argument = format!("literal; touch {}", marker.display());
        let output = run(
            &root,
            ["/usr/bin/printf", "%s", &argument]
                .into_iter()
                .map(OsString::from),
        );
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, argument.as_bytes());
        assert!(!marker.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn gain_reports_direct_terminal_results_and_outcome_counts() {
        let root = setup();

        let success = run(&root, ["--", "/bin/sh", "-c", "exit 0"].map(OsString::from));
        assert_eq!(success.status.code(), Some(0));
        let failure = run(&root, ["--", "/bin/sh", "-c", "exit 7"].map(OsString::from));
        assert_eq!(failure.status.code(), Some(7));
        let timeout = run(
            &root,
            ["--timeout", "25", "--", "/bin/sh", "-c", "sleep 1"].map(OsString::from),
        );
        assert_eq!(timeout.status.code(), Some(124));

        let report = run(&root, ["gain", "--json"].map(OsString::from));
        assert_eq!(report.status.code(), Some(0));
        let report: serde_json::Value = serde_json::from_slice(&report.stdout).expect("gain JSON");
        assert_eq!(report["recorded_executions"], 3);
        assert_eq!(report["outcomes"]["completed"], 1);
        assert_eq!(report["outcomes"]["failed"], 1);
        assert_eq!(report["outcomes"]["timed_out"], 1);
        assert_eq!(
            report["outcomes"]
                .as_object()
                .expect("outcomes")
                .values()
                .map(|value| value.as_u64().expect("count"))
                .sum::<u64>(),
            report["recorded_executions"].as_u64().expect("total")
        );
        assert_eq!(report["by_program"][0]["program"], "sh");

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn gain_human_and_json_reports_share_per_program_values() {
        let root = setup();
        let sh = run(&root, ["/bin/sh", "-c", "exit 0"].map(OsString::from));
        assert_eq!(sh.status.code(), Some(0));
        let printf = run(
            &root,
            ["/usr/bin/printf", "%s", "literal"].map(OsString::from),
        );
        assert_eq!(printf.status.code(), Some(0));

        let human = run(&root, ["gain"].map(OsString::from));
        assert_eq!(human.status.code(), Some(0));
        let human = String::from_utf8_lossy(&human.stdout);
        assert!(human.contains("By Program"));
        assert!(human.contains("sh"));
        assert!(human.contains("printf"));
        assert!(!human.contains("literal"));

        let json = run(&root, ["--json", "gain"].map(OsString::from));
        assert_eq!(json.status.code(), Some(0));
        let json: serde_json::Value = serde_json::from_slice(&json.stdout).expect("gain JSON");
        assert_eq!(json["recorded_executions"], 2);
        assert_eq!(json["by_program"].as_array().expect("programs").len(), 2);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn gain_aggregates_one_hundred_direct_records() {
        let root = setup();

        for _ in 0..100 {
            let output = run(&root, ["/usr/bin/true"].map(OsString::from));
            assert_eq!(
                output.status.code(),
                Some(0),
                "stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let report = run(&root, ["gain", "--json"].map(OsString::from));
        assert_eq!(report.status.code(), Some(0));
        let report: serde_json::Value = serde_json::from_slice(&report.stdout).expect("gain JSON");
        assert_eq!(report["recorded_executions"], 100);
        assert_eq!(report["outcomes"]["completed"], 100);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn gain_clear_returns_json_and_preserves_explicit_config() {
        let root = setup();
        let config = root.join("config.toml");
        let config_contents = b"[execution]\ntimeout_ms = 12000\n";
        fs::write(&config, config_contents).expect("config");

        let target = run(
            &root,
            [
                "--config",
                config.to_str().expect("config path"),
                "--",
                "/bin/echo",
                "before-clear",
            ]
            .map(OsString::from),
        );
        assert_eq!(target.status.code(), Some(0));

        let clear = run(
            &root,
            [
                "--config",
                config.to_str().expect("config path"),
                "gain",
                "--clear",
                "--json",
            ]
            .map(OsString::from),
        );
        assert_eq!(clear.status.code(), Some(0));
        let clear: serde_json::Value = serde_json::from_slice(&clear.stdout).expect("clear JSON");
        assert_eq!(clear, serde_json::json!({"cleared": true}));
        assert_eq!(
            fs::read(&config).expect("config after clear"),
            config_contents
        );

        let report = run(
            &root,
            [
                "--config",
                config.to_str().expect("config path"),
                "gain",
                "--json",
            ]
            .map(OsString::from),
        );
        assert_eq!(report.status.code(), Some(0));
        let report: serde_json::Value =
            serde_json::from_slice(&report.stdout).expect("report after clear");
        assert_eq!(report["recorded_executions"], 0);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[allow(dead_code)]
    fn _make_executable(root: &std::path::Path, name: &str, script: &str) {
        let path = root.join("bin").join(name);
        fs::write(&path, script).expect("write executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
    }
}
