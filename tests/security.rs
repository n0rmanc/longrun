#[cfg(unix)]
mod unix {
    use std::{ffi::OsString, fs, os::unix::fs::PermissionsExt, process::Command};

    use uuid::Uuid;

    fn setup() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("longrun-security-{}", Uuid::now_v7()));
        fs::create_dir_all(root.join("bin")).expect("root");
        let codex = root.join("bin/codex");
        fs::write(
            &codex,
            "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
        )
        .expect("sandbox");
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("mode");
        root
    }

    fn longrun(root: &std::path::Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_longrun"));
        command.env("HOME", root.join("home")).env(
            "PATH",
            format!(
                "{}:{}",
                root.join("bin").display(),
                std::env::var("PATH").expect("PATH")
            ),
        );
        command
    }

    #[test]
    fn explicit_secret_pass_overrides_the_default_deny_pattern_but_unlisted_secrets_stay_hidden() {
        let root = setup();
        let allowed = format!("LONGRUN_ALLOWED_TOKEN_{}", Uuid::now_v7().simple());
        let blocked = format!("LONGRUN_BLOCKED_SECRET_{}", Uuid::now_v7().simple());
        let script =
            format!("printf '%s|%s' \"${{{allowed}:-missing}}\" \"${{{blocked}:-missing}}\"");
        let output = longrun(&root)
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
    fn sandbox_denial_does_not_fall_back_to_direct_execution() {
        let root = setup();
        fs::write(
            root.join("bin/codex"),
            "#!/bin/sh\nprintf 'sandbox denied\\n' >&2\nexit 42\n",
        )
        .expect("replace sandbox");
        let target_marker = root.join("requested-command-ran");
        let output = longrun(&root)
            .args([
                OsString::from("run"),
                OsString::from("--"),
                OsString::from("/bin/sh"),
                OsString::from("-c"),
                OsString::from(format!("touch {}", target_marker.display())),
            ])
            .output()
            .expect("run longrun");

        assert_eq!(output.status.code(), Some(42));
        assert!(output.stderr.starts_with(b"sandbox denied\n"));
        assert!(!target_marker.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn danger_full_access_requires_both_the_command_request_and_config_opt_in() {
        let root = setup();
        let output = longrun(&root)
            .args([
                OsString::from("run"),
                OsString::from("--permission-profile"),
                OsString::from(":danger-full-access"),
                OsString::from("--"),
                OsString::from("/bin/echo"),
                OsString::from("should-not-run"),
            ])
            .output()
            .expect("run longrun");

        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("danger-full-access requires explicit configuration")
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
