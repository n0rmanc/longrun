#[cfg(unix)]
mod codex {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::{Command, Output},
    };

    use serde_json::{Value, json};
    use uuid::Uuid;

    fn setup() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("longrun-codex-{}", Uuid::now_v7()));
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("create bin");
        let log = root.join("codex.log");
        let codex = bin.join("codex");
        fs::write(
            &codex,
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" >> \"$CODEX_LOG\"\n\
             if [ \"$1\" = \"--version\" ]; then printf 'codex 0.1.0\\n'; fi\n\
             if [ \"$1\" = \"plugin\" ] && [ \"$2\" = \"list\" ]; then printf 'longrun@longrun-local installed\\n'; fi\n\
             exit 0\n",
        )
        .expect("write fake codex");
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("make executable");
        (root, log)
    }

    fn command(root: &Path, log: &Path, executable: impl AsRef<Path>) -> Command {
        let path = format!(
            "{}:{}",
            root.join("bin").display(),
            std::env::var("PATH").expect("PATH")
        );
        let mut command = Command::new(executable.as_ref());
        command
            .env("HOME", root.join("home"))
            .env("CODEX_HOME", root.join("codex-home"))
            .env("CODEX_LOG", log)
            .env("PATH", path);
        command
    }

    fn run(root: &Path, log: &Path, arguments: &[&str]) -> Output {
        command(root, log, env!("CARGO_BIN_EXE_longrun"))
            .args(arguments)
            .output()
            .expect("run longrun")
    }

    fn json(output: &Output) -> Value {
        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("JSON report")
    }

    #[test]
    fn init_renders_the_longrun_plugin_hook_skill_and_marketplace() {
        let (root, log) = setup();
        let report = json(&run(&root, &log, &["init", "--codex", "--json"]));
        let generated_root = PathBuf::from(report["generated_root"].as_str().expect("root"));

        let manifest: Value = serde_json::from_slice(
            &fs::read(generated_root.join("plugins/longrun/.codex-plugin/plugin.json"))
                .expect("plugin manifest"),
        )
        .expect("manifest JSON");
        assert_eq!(manifest["name"], "longrun");
        assert_eq!(
            fs::read_to_string(generated_root.join("plugins/longrun/.codex-plugin/plugin.json"))
                .expect("rendered manifest"),
            include_str!("../assets/codex/plugin.json")
        );

        let marketplace: Value = serde_json::from_slice(
            &fs::read(generated_root.join(".agents/plugins/marketplace.json"))
                .expect("marketplace"),
        )
        .expect("marketplace JSON");
        assert_eq!(marketplace["name"], "longrun-local");
        assert_eq!(
            marketplace["plugins"][0]["source"]["path"],
            "./plugins/longrun"
        );
        assert_eq!(
            fs::read_to_string(generated_root.join(".agents/plugins/marketplace.json"))
                .expect("rendered marketplace"),
            include_str!("../assets/codex/marketplace.json")
        );

        let hooks =
            fs::read_to_string(generated_root.join("plugins/longrun/hooks.json")).expect("hooks");
        let unix = env!("CARGO_BIN_EXE_longrun").replace('\'', "'\"'\"'");
        let windows = env!("CARGO_BIN_EXE_longrun").replace('"', "\\\"");
        let mut expected_hooks: Value =
            serde_json::from_str(include_str!("../assets/codex/hooks.json")).expect("hook fixture");
        for (event, suffix) in [
            ("SessionStart", "session-start"),
            ("PreToolUse", "pre-tool-use"),
            ("PostToolUse", "post-tool-use"),
        ] {
            let handler = &mut expected_hooks["hooks"][event][0]["hooks"][0];
            handler["command"] = json!(format!("'{unix}' hook codex {suffix}"));
            handler["commandWindows"] = json!(format!("\"{windows}\" hook codex {suffix}"));
        }
        assert_eq!(
            serde_json::from_str::<Value>(&hooks).expect("rendered hooks JSON"),
            expected_hooks
        );
        assert!(hooks.contains(env!("CARGO_BIN_EXE_longrun")));
        assert!(hooks.contains("session-start"));
        assert!(hooks.contains("pre-tool-use"));
        assert!(hooks.contains("post-tool-use"));
        assert!(hooks.contains("\"timeout\": 86400"));

        let skill =
            fs::read_to_string(generated_root.join("plugins/longrun/skills/longrun/SKILL.md"))
                .expect("skill");
        assert_eq!(
            skill,
            include_str!("../assets/codex/skills/longrun/SKILL.md")
        );
        assert!(skill.contains("without model polling"));
        assert!(skill.contains("longrun submit"));

        let codex_log = fs::read_to_string(&log).expect("codex log");
        assert!(codex_log.contains("marketplace\nadd"));
        assert!(codex_log.contains("longrun@longrun-local"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn init_is_idempotent_repair_updates_a_moved_binary_and_uninstall_preserves_unowned_files() {
        let (root, log) = setup();
        let first = json(&run(&root, &log, &["init", "--codex", "--json"]));
        let generated_root = PathBuf::from(first["generated_root"].as_str().expect("root"));
        assert!(
            run(&root, &log, &["init", "--codex", "--json"])
                .status
                .success()
        );

        let moved = root.join("moved-longrun");
        fs::copy(env!("CARGO_BIN_EXE_longrun"), &moved).expect("copy longrun");
        fs::set_permissions(&moved, fs::Permissions::from_mode(0o755))
            .expect("make moved binary executable");
        let repaired = command(&root, &log, &moved)
            .args(["init", "--codex", "--repair", "--json"])
            .output()
            .expect("repair integration");
        assert!(
            repaired.status.success(),
            "{}",
            String::from_utf8_lossy(&repaired.stderr)
        );
        let hooks = fs::read_to_string(generated_root.join("plugins/longrun/hooks.json"))
            .expect("repaired hooks");
        assert!(hooks.contains(moved.to_str().expect("UTF-8 moved path")));

        let sentinel = generated_root.join("unrelated-user-file");
        fs::write(&sentinel, "keep").expect("write sentinel");
        assert!(
            run(&root, &log, &["uninstall", "--codex", "--json"])
                .status
                .success()
        );
        assert!(sentinel.exists());
        assert!(!generated_root.join("plugins/longrun").exists());

        let codex_log = fs::read_to_string(&log).expect("codex log");
        assert!(codex_log.contains("remove\nlongrun@longrun-local"));
        assert!(codex_log.contains("marketplace\nremove\nlongrun-local"));

        json(&run(&root, &log, &["init", "--codex", "--json"]));
        fs::remove_file(generated_root.join(".longrun-installation.json"))
            .expect("remove ownership inventory");
        fs::write(&log, "").expect("clear codex log");
        assert!(
            run(&root, &log, &["uninstall", "--codex", "--json"])
                .status
                .success()
        );
        assert!(generated_root.join("plugins/longrun").exists());
        assert!(sentinel.exists());
        assert!(fs::read_to_string(&log).expect("read codex log").is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn doctor_reports_codex_plugin_hooks_sandbox_store_and_optional_supervisor() {
        let (root, log) = setup();
        json(&run(&root, &log, &["init", "--codex", "--json"]));
        let report = json(&run(&root, &log, &["doctor", "--json"]));
        let checks = report["checks"].as_array().expect("checks");
        for name in [
            "executable",
            "state_directory",
            "state_store",
            "codex_version",
            "codex_plugin_commands",
            "codex_plugin_activation",
            "hooks",
            "sandbox_profile",
            "platform_process_control",
            "supervisor",
        ] {
            assert!(
                checks.iter().any(|check| check["name"] == name),
                "missing {name}: {checks:?}"
            );
        }
        assert_eq!(report["healthy"], true);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
