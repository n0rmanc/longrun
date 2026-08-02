use std::{fs, path::Path};

use longrun::{
    metrics::{self, MetricOutcome},
    paths::AppPaths,
    protocol::{
        CapturedOutput, EnvironmentPolicy, NativeString, PROTOCOL_VERSION, ResultEnvelope,
        TargetSpec, TerminalReason,
    },
    runner::ExecutionMode,
};
use uuid::Uuid;

fn paths(root: &Path) -> AppPaths {
    AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        runtime_dir: root.join("runtime"),
        handoff_dir: root.join("runtime/handoffs"),
        integration_dir: root.join("data/codex"),
    }
}

fn target(program: &str, args: &[&str]) -> TargetSpec {
    TargetSpec {
        protocol_version: PROTOCOL_VERSION,
        program: NativeString::from_os_string(program.into()),
        args: args
            .iter()
            .map(|argument| NativeString::from_os_string((*argument).into()))
            .collect(),
        cwd: NativeString::from_os_string("/private/test-cwd".into()),
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:test".into(),
    }
}

fn result(reason: TerminalReason, exit_code: Option<i32>, duration_ms: u64) -> ResultEnvelope {
    ResultEnvelope {
        protocol_version: PROTOCOL_VERSION,
        terminal_reason: reason,
        exit_code,
        signal: None,
        duration_ms,
        stdout: CapturedOutput {
            total_bytes: 0,
            tail_base64url: String::new(),
            truncated: false,
            sha256: "sha256:empty".into(),
        },
        stderr: CapturedOutput {
            total_bytes: 0,
            tail_base64url: String::new(),
            truncated: false,
            sha256: "sha256:empty".into(),
        },
    }
}

fn root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("longrun-metrics-{}", Uuid::now_v7()))
}

#[test]
fn records_terminal_outcomes_and_aggregates_without_command_details() {
    let root = root();
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let target = target("/usr/bin/cargo", &["test", "--secret-argument"]);

    for (reason, exit_code, duration_ms) in [
        (TerminalReason::Exited, Some(0), 100),
        (TerminalReason::Exited, Some(7), 200),
        (TerminalReason::TimedOut, None, 300),
        (TerminalReason::Cancelled, None, 400),
        (TerminalReason::OwnerShutdown, None, 500),
    ] {
        metrics::record(
            &paths,
            &target,
            ExecutionMode::Direct,
            &result(reason, exit_code, duration_ms),
        )
        .expect("record metric");
    }

    let report = metrics::read_report(&paths).expect("read report");
    assert_eq!(report.recorded_executions, 5);
    assert_eq!(report.total_duration_ms, 1_500);
    assert_eq!(report.average_duration_ms, 300);
    assert_eq!(report.outcomes.completed, 1);
    assert_eq!(report.outcomes.failed, 1);
    assert_eq!(report.outcomes.timed_out, 1);
    assert_eq!(report.outcomes.cancelled, 1);
    assert_eq!(report.outcomes.owner_shutdown, 1);
    assert_eq!(report.by_program.len(), 1);
    assert_eq!(report.by_program[0].program, "cargo");
    assert_eq!(report.by_program[0].count, 5);
    assert_eq!(report.by_program[0].total_duration_ms, 1_500);
    assert_eq!(report.by_program[0].average_duration_ms, 300);

    let metrics_dir = paths.data_dir.join("metrics");
    let records = fs::read_dir(metrics_dir)
        .expect("metrics directory")
        .map(|entry| fs::read_to_string(entry.expect("record entry").path()).expect("record"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 5);
    for record in records {
        assert!(record.contains("\"program\":\"cargo\""));
        assert!(record.contains("\"mode\":\"direct\""));
        assert!(!record.contains("--secret-argument"));
        assert!(!record.contains("/private/test-cwd"));
    }

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn invalid_and_temporary_records_are_ignored() {
    let root = root();
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let metrics_dir = paths.data_dir.join("metrics");
    fs::create_dir_all(&metrics_dir).expect("metrics directory");
    fs::write(metrics_dir.join("broken.json"), b"{}").expect("broken record");
    fs::write(metrics_dir.join(".unfinished.tmp"), b"{not json").expect("temp record");

    let report = metrics::read_report(&paths).expect("read report");
    assert_eq!(report.recorded_executions, 0);
    assert!(report.by_program.is_empty());

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn clear_removes_metrics_only() {
    let root = root();
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    fs::write(paths.config_dir.join("config.toml"), b"[execution]\n").expect("config");
    fs::write(paths.handoff_dir.join("keep"), b"handoff").expect("handoff");

    metrics::record(
        &paths,
        &target("gh", &["run", "watch"]),
        ExecutionMode::CodexHook,
        &result(TerminalReason::Exited, Some(0), 42),
    )
    .expect("record metric");
    metrics::clear(&paths).expect("clear metrics");

    assert_eq!(
        metrics::read_report(&paths)
            .expect("read cleared report")
            .recorded_executions,
        0
    );
    assert!(paths.config_dir.join("config.toml").exists());
    assert!(paths.handoff_dir.join("keep").exists());

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn outcome_classification_matches_terminal_result() {
    assert_eq!(
        metrics::classify(&result(TerminalReason::Exited, Some(0), 1)),
        MetricOutcome::Completed
    );
    assert_eq!(
        metrics::classify(&result(TerminalReason::Exited, Some(1), 1)),
        MetricOutcome::Failed
    );
    assert_eq!(
        metrics::classify(&result(TerminalReason::TimedOut, None, 1)),
        MetricOutcome::TimedOut
    );
    assert_eq!(
        metrics::classify(&result(TerminalReason::Cancelled, None, 1)),
        MetricOutcome::Cancelled
    );
    assert_eq!(
        metrics::classify(&result(TerminalReason::OwnerShutdown, None, 1)),
        MetricOutcome::OwnerShutdown
    );
}
