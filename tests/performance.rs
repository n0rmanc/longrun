use std::time::{Duration, Instant};

use longrun::{
    hook::{
        input::{CodexCommonInput, PreToolUseInput},
        output::bounded_result_context,
        pre_tool_use::handle_pre_tool_use,
    },
    protocol::{
        EnvironmentPolicy, ExecutionMode, ExecutionState, JobResult, JobSpecification,
        NativeString, ShellMode,
    },
    store::Store,
};
use uuid::Uuid;

const P95_LIMIT: Duration = Duration::from_millis(100);

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[(samples.len() * 95).div_ceil(100) - 1]
}

fn common() -> CodexCommonInput {
    CodexCommonInput {
        session_id: "session".into(),
        agent_id: None,
        agent_type: None,
        transcript_path: None,
        cwd: std::env::current_dir().expect("cwd"),
        hook_event_name: "PreToolUse".into(),
        model: "test".into(),
        permission_mode: "workspace-write".into(),
    }
}

#[test]
fn submit_hook_noop_status_and_completion_context_meet_local_p95_budget() {
    let executable = std::env::current_exe().expect("executable");
    let mut store = Store::open_in_memory().expect("store");
    let mut submits = Vec::new();
    let mut noops = Vec::new();
    for index in 0..20 {
        let input = PreToolUseInput {
            common: common(),
            turn_id: format!("turn-{index}"),
            tool_use_id: format!("submit-{index}"),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({
                "command": format!("\"{}\" submit -- /bin/true", executable.display()),
            }),
        };
        let start = Instant::now();
        assert!(
            handle_pre_tool_use(&input, &executable, &mut store, 1_000 + index)
                .expect("submit hook")
                .is_some()
        );
        submits.push(start.elapsed());

        let mut unrelated = input.clone();
        unrelated.tool_use_id = format!("noop-{index}");
        unrelated.tool_input = serde_json::json!({"command": "printf unrelated"});
        let start = Instant::now();
        assert!(
            handle_pre_tool_use(&unrelated, &executable, &mut store, 2_000 + index)
                .expect("noop hook")
                .is_none()
        );
        noops.push(start.elapsed());
    }

    let job = JobSpecification {
        protocol_version: 1,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string("/bin/true".into()),
        args: Vec::new(),
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:performance".into(),
    };
    store.create_job(&job).expect("job");
    let statuses = (0..20)
        .map(|_| {
            let start = Instant::now();
            store.status(job.job_id).expect("status");
            start.elapsed()
        })
        .collect();
    let result = JobResult {
        job_id: job.job_id,
        terminal_state: ExecutionState::Succeeded,
        exit_code: Some(0),
        signal: None,
        duration_ms: 1,
        stdout_log: NativeString::from_os_string("stdout.log".into()),
        stderr_log: NativeString::from_os_string("stderr.log".into()),
        stdout_tail: "x".repeat(32 * 1024),
        stderr_tail: "y".repeat(32 * 1024),
        stdout_truncated: false,
        stderr_truncated: false,
        result_hash: "sha256:performance".into(),
        completed_at_ms: 1,
    };
    let contexts = (0..20)
        .map(|_| {
            let start = Instant::now();
            let context = bounded_result_context(&result, 1024);
            assert!(context.len() <= 4_096);
            start.elapsed()
        })
        .collect();

    for (name, samples) in [
        ("submit", submits),
        ("hook no-op", noops),
        ("status", statuses),
        ("completion context", contexts),
    ] {
        assert!(
            p95(samples) < P95_LIMIT,
            "{name} p95 exceeded {P95_LIMIT:?}"
        );
    }
}
