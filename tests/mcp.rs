use std::fs;

#[test]
fn mcp_adapter_exposes_only_supervisor_backed_job_operations() {
    let source = fs::read_to_string("src/mcp.rs").expect("MCP adapter");
    for tool in [
        "async fn status",
        "async fn wait",
        "async fn logs",
        "async fn cancel",
    ] {
        assert!(source.contains(tool), "missing MCP tool {tool}");
    }
    for delegation in [
        "supervisor::status",
        "supervisor::wait",
        "supervisor::logs",
        "supervisor::cancel",
    ] {
        assert!(
            source.contains(delegation),
            "missing delegation {delegation}"
        );
    }
    for forbidden in ["Command::new", "run_worker", "runner::", "Supervisor::new"] {
        assert!(
            !source.contains(forbidden),
            "MCP must not own requested-command execution: {forbidden}"
        );
    }
}
