use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    paths::AppPaths,
    protocol::{ResultEnvelope, TargetSpec, TerminalReason},
    runner::ExecutionMode,
};

pub const SCHEMA_VERSION: u32 = 1;

const METRICS_DIR: &str = "metrics";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricOutcome {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    OwnerShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricMode {
    Direct,
    CodexHook,
}

impl From<ExecutionMode> for MetricMode {
    fn from(mode: ExecutionMode) -> Self {
        match mode {
            ExecutionMode::Direct => Self::Direct,
            ExecutionMode::CodexHook => Self::CodexHook,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionMetric {
    pub schema_version: u32,
    pub program: String,
    pub duration_ms: u64,
    pub outcome: MetricOutcome,
    pub exit_code: Option<i32>,
    pub mode: MetricMode,
    pub completed_at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeCounts {
    pub completed: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub cancelled: u64,
    pub owner_shutdown: u64,
}

impl OutcomeCounts {
    fn add(&mut self, outcome: MetricOutcome) {
        match outcome {
            MetricOutcome::Completed => self.completed += 1,
            MetricOutcome::Failed => self.failed += 1,
            MetricOutcome::TimedOut => self.timed_out += 1,
            MetricOutcome::Cancelled => self.cancelled += 1,
            MetricOutcome::OwnerShutdown => self.owner_shutdown += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramSummary {
    pub program: String,
    pub count: u64,
    pub total_duration_ms: u64,
    pub average_duration_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GainReport {
    pub recorded_executions: u64,
    pub total_duration_ms: u64,
    pub average_duration_ms: u64,
    pub outcomes: OutcomeCounts,
    pub by_program: Vec<ProgramSummary>,
}

pub fn record(
    paths: &AppPaths,
    target: &TargetSpec,
    mode: ExecutionMode,
    result: &ResultEnvelope,
) -> Result<()> {
    let metric = ExecutionMetric {
        schema_version: SCHEMA_VERSION,
        program: executable_name(target)?,
        duration_ms: result.duration_ms,
        outcome: classify(result),
        exit_code: result.exit_code,
        mode: mode.into(),
        completed_at_ms: now_ms()?,
    };
    write_metric(paths, &metric)
}

pub fn classify(result: &ResultEnvelope) -> MetricOutcome {
    match result.terminal_reason {
        TerminalReason::Exited if result.exit_code == Some(0) => MetricOutcome::Completed,
        TerminalReason::Exited | TerminalReason::SpawnFailed => MetricOutcome::Failed,
        TerminalReason::TimedOut => MetricOutcome::TimedOut,
        TerminalReason::Cancelled => MetricOutcome::Cancelled,
        TerminalReason::OwnerShutdown => MetricOutcome::OwnerShutdown,
    }
}

pub fn read_report(paths: &AppPaths) -> Result<GainReport> {
    let directory = metrics_dir(paths);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(GainReport::default());
        }
        Err(error) => return Err(error.into()),
    };

    let mut metrics = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let metric = match serde_json::from_slice::<ExecutionMetric>(&bytes) {
            Ok(metric) if valid_metric(&metric) => metric,
            _ => continue,
        };
        metrics.push(metric);
    }

    Ok(aggregate(metrics))
}

pub fn aggregate(metrics: impl IntoIterator<Item = ExecutionMetric>) -> GainReport {
    let mut report = GainReport::default();
    let mut programs = BTreeMap::<String, (u64, u64)>::new();

    for metric in metrics {
        report.recorded_executions = report.recorded_executions.saturating_add(1);
        report.total_duration_ms = report.total_duration_ms.saturating_add(metric.duration_ms);
        report.outcomes.add(metric.outcome);

        let entry = programs.entry(metric.program).or_default();
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(metric.duration_ms);
    }

    report.average_duration_ms = report
        .total_duration_ms
        .checked_div(report.recorded_executions)
        .unwrap_or_default();
    report.by_program = programs
        .into_iter()
        .map(|(program, (count, total_duration_ms))| ProgramSummary {
            program,
            count,
            total_duration_ms,
            average_duration_ms: total_duration_ms.checked_div(count).unwrap_or_default(),
        })
        .collect();
    report
}

pub fn clear(paths: &AppPaths) -> Result<()> {
    match fs::remove_dir_all(metrics_dir(paths)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn format_duration(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    let milliseconds = duration_ms % 1_000;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else if seconds > 0 {
        format!("{seconds}s")
    } else {
        format!("{milliseconds}ms")
    }
}

pub fn write_human_report(report: &GainReport, output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "Longrun Execution Metrics")?;
    writeln!(
        output,
        "════════════════════════════════════════════════════════════"
    )?;
    writeln!(
        output,
        "Recorded executions: {}",
        report.recorded_executions
    )?;
    writeln!(
        output,
        "Total wait:           {}",
        format_duration(report.total_duration_ms)
    )?;
    writeln!(
        output,
        "Average duration:     {}",
        format_duration(report.average_duration_ms)
    )?;
    writeln!(output)?;
    writeln!(output, "By Outcome")?;
    writeln!(
        output,
        "────────────────────────────────────────────────────────────"
    )?;
    writeln!(output, "  completed:      {}", report.outcomes.completed)?;
    writeln!(output, "  failed:         {}", report.outcomes.failed)?;
    writeln!(output, "  timed_out:      {}", report.outcomes.timed_out)?;
    writeln!(output, "  cancelled:      {}", report.outcomes.cancelled)?;
    writeln!(
        output,
        "  owner_shutdown: {}",
        report.outcomes.owner_shutdown
    )?;
    writeln!(output)?;
    writeln!(output, "By Program")?;
    writeln!(
        output,
        "────────────────────────────────────────────────────────────"
    )?;
    if report.by_program.is_empty() {
        writeln!(output, "  (none)")?;
    } else {
        writeln!(output, "  Program          Count  Total       Average")?;
        for summary in &report.by_program {
            writeln!(
                output,
                "  {:<16} {:>5}  {:<10} {}",
                summary.program,
                summary.count,
                format_duration(summary.total_duration_ms),
                format_duration(summary.average_duration_ms)
            )?;
        }
    }
    Ok(())
}

fn write_metric(paths: &AppPaths, metric: &ExecutionMetric) -> Result<()> {
    let directory = metrics_dir(paths);
    fs::create_dir_all(&directory)?;

    let id = Uuid::now_v7().to_string();
    let temporary = directory.join(format!(".{id}.tmp"));
    let final_path = directory.join(format!("{id}.json"));
    let bytes = serde_json::to_vec(metric)?;

    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        fs::rename(&temporary, &final_path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn valid_metric(metric: &ExecutionMetric) -> bool {
    metric.schema_version == SCHEMA_VERSION
        && !metric.program.is_empty()
        && !metric.program.contains(['/', '\\'])
}

fn executable_name(target: &TargetSpec) -> Result<String> {
    let program = target.program.to_os_string()?;
    let path = Path::new(&program);
    let name = path.file_name().unwrap_or(path.as_os_str());
    let name = name.to_string_lossy().into_owned();
    if name.is_empty() {
        return Err(Error::InvalidInput("target program name is empty".into()));
    }
    Ok(name)
}

fn metrics_dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir.join(METRICS_DIR)
}

fn now_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Unavailable(format!("system clock before epoch: {error}")))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Unavailable("system clock is out of range".into()))
}
