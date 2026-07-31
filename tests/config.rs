use longrun::config::Config;

#[test]
fn defaults_are_safe_and_bounded() {
    let config = Config::default();

    assert_eq!(config.execution.permission_profile, ":workspace");
    assert!(!config.execution.allow_shell);
    assert!(!config.execution.allow_danger_full_access);
    assert!(config.output.model_max_bytes > 0);
    assert!(!config.recovery.auto_resume);
}

#[test]
fn toml_overrides_are_validated() {
    let config = Config::from_toml(
        r#"
        [execution]
        timeout_ms = 12_000
        permission_profile = ":read-only"

        [output]
        model_max_bytes = 4096
        "#,
    )
    .expect("valid config");

    assert_eq!(config.execution.timeout_ms, 12_000);
    assert_eq!(config.execution.permission_profile, ":read-only");
    assert_eq!(config.output.model_max_bytes, 4096);
    assert!(Config::from_toml("[execution]\ntimeout_ms = 0").is_err());
}

#[test]
fn secret_patterns_deny_inheritance_unless_explicitly_allowed() {
    let config = Config::default();

    assert!(config.environment.is_protected("GITHUB_TOKEN"));
    assert!(config.environment.is_protected("db_password"));
    assert!(!config.environment.allows("GITHUB_TOKEN"));
    assert!(!config.environment.allows("PATH"));
}

#[test]
fn danger_full_access_requires_configuration_opt_in() {
    let config = Config::default();
    assert!(!config.permits_permission_profile(":danger-full-access"));
    assert!(config.permits_permission_profile(":workspace"));

    let config =
        Config::from_toml("[execution]\nallow_danger_full_access = true").expect("valid config");
    assert!(config.permits_permission_profile(":danger-full-access"));
}
