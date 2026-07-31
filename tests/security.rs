#[cfg(unix)]
mod unix {
    use std::{ffi::OsString, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

    use longrun::{
        hook::{
            input::{CodexCommonInput, PreToolUseInput},
            pre_tool_use::handle_pre_tool_use,
        },
        store::Store,
    };
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

    fn pre_tool_use(command: &str) -> PreToolUseInput {
        PreToolUseInput {
            common: CodexCommonInput {
                session_id: "session".into(),
                agent_id: None,
                agent_type: None,
                transcript_path: None,
                cwd: std::env::current_dir().expect("cwd"),
                hook_event_name: "PreToolUse".into(),
                model: "gpt-test".into(),
                permission_mode: "default".into(),
            },
            turn_id: "turn".into(),
            tool_use_id: "tool".into(),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({ "command": command }),
        }
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

    #[test]
    fn hook_requires_the_absolute_installed_binary_and_rejects_path_shadowing() {
        let mut store = Store::open_in_memory().expect("store");
        let expected = Path::new("/opt/longrun");

        assert!(
            handle_pre_tool_use(
                &pre_tool_use("longrun submit -- /bin/echo shadowed"),
                expected,
                &mut store,
                1,
            )
            .expect("hook")
            .is_none()
        );
        assert!(
            handle_pre_tool_use(
                &pre_tool_use("\"/tmp/longrun\" submit -- /bin/echo shadowed"),
                expected,
                &mut store,
                1,
            )
            .expect("hook")
            .is_none()
        );
        assert!(
            handle_pre_tool_use(
                &pre_tool_use("longrun submit -- /bin/echo relative"),
                Path::new("longrun"),
                &mut store,
                1,
            )
            .expect("hook")
            .is_none()
        );
    }

    #[test]
    fn direct_arguments_with_shell_metacharacters_remain_literal() {
        let root = setup();
        let target_marker = root.join("metacharacter-ran");
        let argument = format!("literal; touch {}", target_marker.display());
        let output = longrun(&root)
            .args([
                OsString::from("run"),
                OsString::from("--"),
                OsString::from("/usr/bin/printf"),
                OsString::from(&argument),
            ])
            .output()
            .expect("run longrun");

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, argument.as_bytes());
        assert!(!target_marker.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn consumed_receipt_nonce_cannot_be_replayed() {
        let mut store = Store::open_in_memory().expect("store");
        store
            .consume_receipt_once("nonce")
            .expect("first consumption");
        assert!(store.consume_receipt_once("nonce").is_err());
    }
}
