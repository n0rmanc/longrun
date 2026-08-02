use std::time::{Duration, Instant};

use longrun::{
    config::Config,
    hook::{
        input::{CodexCommonInput, PreToolUseInput},
        output::bounded_result_context,
        pre_tool_use::handle_pre_tool_use,
    },
    paths::AppPaths,
    protocol::{CapturedOutput, ResultEnvelope, TerminalReason},
};

const P95_LIMIT: Duration = Duration::from_millis(100);

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[(samples.len() * 95).div_ceil(100) - 1]
}

fn paths(root: &std::path::Path) -> AppPaths {
    AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        runtime_dir: root.join("runtime"),
        handoff_dir: root.join("runtime/handoffs"),
        integration_dir: root.join("integration"),
    }
}

#[test]
fn pre_tool_use_and_completion_context_stay_within_local_budget() {
    let root = std::env::temp_dir().join(format!("longrun-performance-{}", uuid::Uuid::now_v7()));
    let paths = paths(&root);
    paths.ensure_private_state().expect("state");
    let executable = std::env::current_exe().expect("executable");
    let mut pre_times = Vec::new();
    let config = Config::default();
    for index in 0..20 {
        let input = PreToolUseInput {
            common: CodexCommonInput {
                session_id: "session".into(),
                agent_id: None,
                agent_type: None,
                transcript_path: None,
                cwd: std::env::current_dir().expect("cwd"),
                hook_event_name: "PreToolUse".into(),
                model: "test".into(),
                permission_mode: "workspace-write".into(),
            },
            turn_id: format!("turn-{index}"),
            tool_use_id: format!("tool-{index}"),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({
                "command": format!("{} -- /bin/true", executable.display()),
            }),
        };
        let start = Instant::now();
        assert!(
            handle_pre_tool_use(&input, &executable, &paths, &config, 1_000 + index)
                .expect("pre hook")
                .is_some()
        );
        pre_times.push(start.elapsed());
    }

    let result = ResultEnvelope {
        protocol_version: 2,
        terminal_reason: TerminalReason::Exited,
        exit_code: Some(0),
        signal: None,
        duration_ms: 1,
        stdout: CapturedOutput {
            total_bytes: 1,
            tail_base64url: "eA".into(),
            truncated: false,
            sha256: "sha256:x".into(),
        },
        stderr: CapturedOutput {
            total_bytes: 0,
            tail_base64url: String::new(),
            truncated: false,
            sha256: "sha256:empty".into(),
        },
    };
    let context_times = (0..20)
        .map(|_| {
            let start = Instant::now();
            let context = bounded_result_context(&result, 4_096);
            assert!(context.len() <= 4_096);
            start.elapsed()
        })
        .collect();

    assert!(p95(pre_times) < P95_LIMIT);
    assert!(p95(context_times) < P95_LIMIT);
    let _ = std::fs::remove_dir_all(root);
}
