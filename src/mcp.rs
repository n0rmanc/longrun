use base64::Engine;
use rmcp::{
    Json, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    paths::AppPaths,
    supervisor,
};

#[derive(Debug, Deserialize, JsonSchema)]
struct JobRequest {
    job_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LogsRequest {
    job_id: String,
    #[serde(default)]
    stderr: bool,
    #[serde(default)]
    offset: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CancelRequest {
    job_id: String,
    #[serde(default)]
    grace_ms: Option<u64>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LongrunMcp {}

#[derive(Clone)]
struct LongrunMcp {
    paths: AppPaths,
    termination_grace_ms: u64,
    tool_router: ToolRouter<Self>,
}

#[tool_router(router = tool_router)]
impl LongrunMcp {
    fn new(paths: AppPaths, config: &Config) -> Self {
        Self {
            paths,
            termination_grace_ms: config.execution.termination_grace_ms,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Read a Longrun job status through the running supervisor.")]
    async fn status(
        &self,
        Parameters(request): Parameters<JobRequest>,
    ) -> Result<Json<serde_json::Value>, String> {
        let job_id = parse_job_id(&request.job_id)?;
        let status = supervisor::status(&self.paths, job_id)
            .await
            .map_err(|error| error.to_string())?;
        serde_json::to_value(status)
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(description = "Wait for a Longrun job through the running supervisor.")]
    async fn wait(
        &self,
        Parameters(request): Parameters<JobRequest>,
    ) -> Result<Json<serde_json::Value>, String> {
        let job_id = parse_job_id(&request.job_id)?;
        let status = supervisor::wait(&self.paths, job_id)
            .await
            .map_err(|error| error.to_string())?;
        serde_json::to_value(status)
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        description = "Read one bounded, base64url-encoded local log chunk through the running supervisor."
    )]
    async fn logs(
        &self,
        Parameters(request): Parameters<LogsRequest>,
    ) -> Result<Json<serde_json::Value>, String> {
        let job_id = parse_job_id(&request.job_id)?;
        let chunk = supervisor::logs(&self.paths, job_id, request.stderr, request.offset)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Json(json!({
            "bytes_base64url": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(chunk.bytes),
            "next_offset": chunk.next_offset,
            "at_end": chunk.at_end,
            "terminal": chunk.terminal,
            "untrusted": true,
        })))
    }

    #[tool(
        description = "Request Longrun process-tree cancellation through the running supervisor."
    )]
    async fn cancel(
        &self,
        Parameters(request): Parameters<CancelRequest>,
    ) -> Result<Json<serde_json::Value>, String> {
        let job_id = parse_job_id(&request.job_id)?;
        let requested = supervisor::cancel(
            &self.paths,
            job_id,
            request.grace_ms.unwrap_or(self.termination_grace_ms),
        )
        .await
        .map_err(|error| error.to_string())?;
        Ok(Json(json!({
            "job_id": job_id,
            "cancellation_requested": requested,
        })))
    }
}

pub async fn run(paths: &AppPaths, config: &Config) -> Result<()> {
    LongrunMcp::new(paths.clone(), config)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|error| Error::Unavailable(format!("cannot start MCP server: {error}")))?
        .waiting()
        .await
        .map_err(|error| Error::Unavailable(format!("MCP server stopped unexpectedly: {error}")))?;
    Ok(())
}

fn parse_job_id(job_id: &str) -> std::result::Result<Uuid, String> {
    Uuid::parse_str(job_id).map_err(|error| format!("invalid job_id: {error}"))
}
