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

    #[test]
    fn explicit_secret_pass_overrides_the_default_deny_pattern_but_unlisted_secrets_stay_hidden() {
        let root = setup();
        let allowed = format!("LONGRUN_ALLOWED_TOKEN_{}", Uuid::now_v7().simple());
        let blocked = format!("LONGRUN_BLOCKED_SECRET_{}", Uuid::now_v7().simple());
        let script =
            format!("printf '%s|%s' \"${{{allowed}:-missing}}\" \"${{{blocked}:-missing}}\"");
        let output = Command::new(env!("CARGO_BIN_EXE_longrun"))
            .args([
                OsString::from("run"),
                OsString::from("--env-pass"),
                OsString::from(&allowed),
                OsString::from("--"),
                OsString::from("/bin/sh"),
                OsString::from("-c"),
                OsString::from(script),
            ])
            .env("HOME", root.join("home"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").expect("PATH")
                ),
            )
            .env(&allowed, "allowed")
            .env(&blocked, "blocked")
            .output()
            .expect("run longrun");

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"allowed|missing");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
