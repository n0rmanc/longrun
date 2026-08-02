use longrun::config::Config;

#[test]
fn defaults_are_safe_and_bounded() {
    let config = Config::default();

    assert_eq!(config.handoff.ttl_ms, 300_000);
    assert!(
        config.execution.post_tool_use_timeout_ms
            >= config
                .minimum_post_tool_use_timeout_ms()
                .expect("timeout arithmetic")
    );
    assert!(config.output.model_max_bytes > 0);
}

#[test]
fn toml_overrides_are_validated() {
    let config = Config::from_toml(
        r#"
        [execution]
        timeout_ms = 12_000
        [output]
        model_max_bytes = 4096

        [handoff]
        ttl_ms = 120000
        "#,
    )
    .expect("valid config");

    assert_eq!(config.execution.timeout_ms, 12_000);
    assert_eq!(config.output.model_max_bytes, 4096);
    assert_eq!(config.handoff.ttl_ms, 120000);
    assert!(Config::from_toml("[execution]\ntimeout_ms = 0").is_err());
}

#[test]
fn post_tool_use_timeout_must_cover_cleanup_and_serialization_margins() {
    assert!(Config::from_toml("[execution]\npost_tool_use_timeout_ms = 1").is_err());
}

#[test]
fn handoff_ttl_has_a_fifteen_minute_ceiling() {
    assert!(Config::from_toml("[handoff]\nttl_ms = 900001").is_err());
}
