use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::{
        DeliveryState, ExecutionState, JobResult, JobSpecification, PendingState, PendingSubmission,
    },
};

pub struct Store {
    connection: Connection,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobStatus {
    pub job_id: Uuid,
    pub execution_state: ExecutionState,
    pub delivery_state: DeliveryState,
    pub result: Option<JobResult>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut store = Self {
            connection: Connection::open(path)?,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let mut store = Self {
            connection: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn migrate(&mut self) -> Result<()> {
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;",
        )?;
        let transaction = self.connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_submissions (
                tool_use_id TEXT PRIMARY KEY,
                state TEXT NOT NULL,
                expires_at_ms INTEGER NOT NULL,
                payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS consumed_receipts (
                nonce TEXT PRIMARY KEY,
                consumed_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS jobs (
                job_id TEXT PRIMARY KEY,
                spec_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS executions (
                job_id TEXT PRIMARY KEY REFERENCES jobs(job_id) ON DELETE CASCADE,
                state TEXT NOT NULL,
                execution_claim TEXT,
                worker_id TEXT,
                pid INTEGER,
                started_at_ms INTEGER,
                finished_at_ms INTEGER,
                cancel_requested_at_ms INTEGER,
                cancel_grace_ms INTEGER
             );
             CREATE TABLE IF NOT EXISTS results (
                job_id TEXT PRIMARY KEY REFERENCES jobs(job_id) ON DELETE CASCADE,
                result_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS deliveries (
                job_id TEXT PRIMARY KEY REFERENCES jobs(job_id) ON DELETE CASCADE,
                state TEXT NOT NULL,
                lease_id TEXT,
                lease_expires_at_ms INTEGER,
                idempotency_key TEXT
             );
             CREATE TABLE IF NOT EXISTS leases (
                lease_id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
                owner TEXT NOT NULL,
                expires_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS integrations (
                integration TEXT PRIMARY KEY,
                manifest_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
             );",
        )?;
        let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 2 {
            if version == 1 {
                transaction.execute_batch(
                    "ALTER TABLE executions ADD COLUMN cancel_requested_at_ms INTEGER;
                     ALTER TABLE executions ADD COLUMN cancel_grace_ms INTEGER;",
                )?;
            }
            transaction.pragma_update(None, "user_version", 2)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    pub fn journal_mode(&self) -> Result<String> {
        Ok(self
            .connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
    }

    pub fn create_job(&mut self, specification: &JobSpecification) -> Result<()> {
        if specification.protocol_version != crate::protocol::PROTOCOL_VERSION {
            return Err(Error::InvalidInput(
                "unsupported job protocol version".into(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let job_id = specification.job_id.to_string();
        let created_at_ms = specification.created_at_ms;
        let specification = serde_json::to_string(specification)?;
        transaction.execute(
            "INSERT INTO jobs (job_id, spec_json, created_at_ms) VALUES (?1, ?2, ?3)",
            params![job_id, specification, created_at_ms],
        )?;
        transaction.execute(
            "INSERT INTO executions (job_id, state) VALUES (?1, ?2)",
            params![job_id, ExecutionState::Accepted.as_str()],
        )?;
        transaction.execute(
            "INSERT INTO deliveries (job_id, state) VALUES (?1, ?2)",
            params![job_id, DeliveryState::Undelivered.as_str()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn consume_receipt_once(&mut self, nonce: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO consumed_receipts (nonce, consumed_at_ms) VALUES (?1, unixepoch() * 1000)",
            [nonce],
        )?;
        Ok(())
    }

    pub fn save_pending(&mut self, pending: &PendingSubmission) -> Result<()> {
        self.connection.execute(
            "INSERT INTO pending_submissions (tool_use_id, state, expires_at_ms, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                pending.tool_use_id,
                pending.state.as_str(),
                pending.expires_at_ms,
                serde_json::to_string(pending)?
            ],
        )?;
        Ok(())
    }

    pub fn cleanup_expired_pending(&mut self, now_ms: i64) -> Result<usize> {
        Ok(self.connection.execute(
            "DELETE FROM pending_submissions WHERE expires_at_ms <= ?1",
            [now_ms],
        )?)
    }

    pub fn claim_pending_by_token(
        &mut self,
        token_hash: &str,
        now_ms: i64,
    ) -> Result<PendingSubmission> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM pending_submissions WHERE expires_at_ms <= ?1",
            [now_ms],
        )?;
        let mut statement = transaction.prepare(
            "SELECT tool_use_id, payload_json FROM pending_submissions
             WHERE state = 'pending' AND expires_at_ms > ?1",
        )?;
        let rows = statement.query_map([now_ms], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut matched = None;
        for row in rows {
            let (tool_use_id, payload) = row?;
            let pending: PendingSubmission = serde_json::from_str(&payload)?;
            if pending.hook_token_hash == token_hash {
                matched = Some((tool_use_id, pending));
                break;
            }
        }
        drop(statement);
        let (tool_use_id, mut pending) =
            matched.ok_or_else(|| Error::Denied("invalid or expired hook token".into()))?;
        let changed = transaction.execute(
            "UPDATE pending_submissions SET state = 'claimed' WHERE tool_use_id = ?1 AND state = 'pending'",
            [tool_use_id],
        )?;
        if changed != 1 {
            return Err(Error::Denied("hook token has already been claimed".into()));
        }
        pending.state = PendingState::Claimed;
        transaction.execute(
            "UPDATE pending_submissions SET payload_json = ?1 WHERE tool_use_id = ?2",
            params![serde_json::to_string(&pending)?, pending.tool_use_id],
        )?;
        transaction.commit()?;
        Ok(pending)
    }

    pub fn pending(&self, tool_use_id: &str) -> Result<PendingSubmission> {
        let payload: String = self.connection.query_row(
            "SELECT payload_json FROM pending_submissions WHERE tool_use_id = ?1",
            [tool_use_id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&payload)?)
    }

    pub fn consume_pending_and_create_job(
        &mut self,
        tool_use_id: &str,
        nonce: &str,
        job: &JobSpecification,
        now_ms: i64,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let payload: String = transaction.query_row(
            "SELECT payload_json FROM pending_submissions WHERE tool_use_id = ?1",
            [tool_use_id],
            |row| row.get(0),
        )?;
        let mut pending: PendingSubmission = serde_json::from_str(&payload)?;
        if pending.state != PendingState::Claimed {
            return Err(Error::Denied(
                "pending submission has not been claimed exactly once".into(),
            ));
        }
        if pending.expires_at_ms <= now_ms {
            return Err(Error::Denied("pending submission has expired".into()));
        }
        if !pending.matches_job(job) {
            return Err(Error::Denied(
                "receipt job does not match pending submission".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO consumed_receipts (nonce, consumed_at_ms) VALUES (?1, unixepoch() * 1000)",
            [nonce],
        )?;
        transaction.execute(
            "INSERT INTO jobs (job_id, spec_json, created_at_ms) VALUES (?1, ?2, ?3)",
            params![
                job.job_id.to_string(),
                serde_json::to_string(job)?,
                job.created_at_ms
            ],
        )?;
        transaction.execute(
            "INSERT INTO executions (job_id, state) VALUES (?1, ?2)",
            params![job.job_id.to_string(), ExecutionState::Accepted.as_str()],
        )?;
        transaction.execute(
            "INSERT INTO deliveries (job_id, state) VALUES (?1, ?2)",
            params![job.job_id.to_string(), DeliveryState::Undelivered.as_str()],
        )?;
        pending.state = PendingState::Consumed;
        transaction.execute(
            "UPDATE pending_submissions SET state = 'consumed', payload_json = ?1
             WHERE tool_use_id = ?2 AND state = 'claimed'",
            params![serde_json::to_string(&pending)?, tool_use_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn execution_state(&self, job_id: Uuid) -> Result<ExecutionState> {
        let state: String = self.connection.query_row(
            "SELECT state FROM executions WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )?;
        state.parse()
    }

    pub fn job(&self, job_id: Uuid) -> Result<JobSpecification> {
        let specification: String = self.connection.query_row(
            "SELECT spec_json FROM jobs WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&specification)?)
    }

    pub fn claim_execution(&mut self, job_id: Uuid, claim: &str) -> Result<JobSpecification> {
        let transaction = self.connection.transaction()?;
        let job_id = job_id.to_string();
        let specification: String = transaction.query_row(
            "SELECT spec_json FROM jobs WHERE job_id = ?1",
            [job_id.as_str()],
            |row| row.get(0),
        )?;
        let changed = transaction.execute(
            "UPDATE executions
             SET state = 'starting', execution_claim = ?1, worker_id = ?2
             WHERE job_id = ?2 AND state = 'accepted' AND execution_claim IS NULL",
            params![claim, job_id],
        )?;
        if changed != 1 {
            return Err(Error::Denied(
                "job is already claimed or is no longer executable".into(),
            ));
        }
        transaction.commit()?;
        Ok(serde_json::from_str(&specification)?)
    }

    pub fn mark_running(&mut self, job_id: Uuid, claim: &str) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE executions SET state = 'running', started_at_ms = unixepoch() * 1000
             WHERE job_id = ?1 AND state = 'starting' AND execution_claim = ?2",
            params![job_id.to_string(), claim],
        )?;
        if changed != 1 {
            return Err(Error::Denied("execution claim is not active".into()));
        }
        Ok(())
    }

    pub fn request_cancellation(
        &mut self,
        job_id: Uuid,
        grace_ms: u64,
        now_ms: i64,
    ) -> Result<bool> {
        let job_id = job_id.to_string();
        let state: String = self.connection.query_row(
            "SELECT state FROM executions WHERE job_id = ?1",
            [job_id.as_str()],
            |row| row.get(0),
        )?;
        let state: ExecutionState = state.parse()?;
        if state.is_terminal() {
            return Ok(false);
        }
        let changed = self.connection.execute(
            "UPDATE executions
             SET cancel_requested_at_ms = ?1, cancel_grace_ms = ?2
             WHERE job_id = ?3
               AND state IN ('starting', 'running')
               AND cancel_requested_at_ms IS NULL",
            params![now_ms, grace_ms as i64, job_id],
        )?;
        Ok(changed == 1)
    }

    pub fn cancellation_grace(&self, job_id: Uuid) -> Result<Option<u64>> {
        self.connection
            .query_row(
                "SELECT cancel_grace_ms FROM executions
                 WHERE job_id = ?1 AND cancel_requested_at_ms IS NOT NULL",
                [job_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|grace| {
                grace
                    .try_into()
                    .map_err(|_| Error::InvalidInput("invalid stored cancellation grace".into()))
            })
            .transpose()
    }

    pub fn finish_execution(&mut self, result: &JobResult, claim: &str) -> Result<()> {
        if !result.terminal_state.is_terminal() {
            return Err(Error::InvalidInput("result must be terminal".into()));
        }
        let transaction = self.connection.transaction()?;
        let job_id = result.job_id.to_string();
        let changed = transaction.execute(
            "UPDATE executions
             SET state = ?1, finished_at_ms = ?2
             WHERE job_id = ?3 AND state = 'running' AND execution_claim = ?4",
            params![
                result.terminal_state.as_str(),
                result.completed_at_ms,
                job_id,
                claim
            ],
        )?;
        if changed != 1 {
            return Err(Error::Denied(
                "execution claim cannot finish this job".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO results (job_id, result_json, created_at_ms) VALUES (?1, ?2, ?3)",
            params![
                job_id,
                serde_json::to_string(result)?,
                result.completed_at_ms
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn result(&self, job_id: Uuid) -> Result<JobResult> {
        let result: String = self.connection.query_row(
            "SELECT result_json FROM results WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&result)?)
    }

    pub fn status(&self, job_id: Uuid) -> Result<JobStatus> {
        let (execution, delivery): (String, String) = self.connection.query_row(
            "SELECT executions.state, deliveries.state
             FROM executions JOIN deliveries USING (job_id) WHERE job_id = ?1",
            [job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let result = self
            .connection
            .query_row(
                "SELECT result_json FROM results WHERE job_id = ?1",
                [job_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| serde_json::from_str(&value))
            .transpose()?;
        Ok(JobStatus {
            job_id,
            execution_state: execution.parse()?,
            delivery_state: delivery.parse()?,
            result,
        })
    }

    pub fn list(&self, state: Option<ExecutionState>) -> Result<Vec<JobStatus>> {
        let sql = "SELECT jobs.job_id, executions.state, deliveries.state, results.result_json
                   FROM jobs
                   JOIN executions USING (job_id)
                   JOIN deliveries USING (job_id)
                   LEFT JOIN results USING (job_id)
                   WHERE (?1 IS NULL OR executions.state = ?1)
                   ORDER BY jobs.created_at_ms DESC";
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([state.map(ExecutionState::as_str)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (job_id, execution, delivery, result) = row?;
            Ok(JobStatus {
                job_id: Uuid::parse_str(&job_id).map_err(|error| {
                    Error::InvalidInput(format!("invalid stored job id: {error}"))
                })?,
                execution_state: execution.parse()?,
                delivery_state: delivery.parse()?,
                result: result
                    .map(|value| serde_json::from_str(&value))
                    .transpose()?,
            })
        })
        .collect()
    }

    pub fn transition_execution(&mut self, job_id: Uuid, next: ExecutionState) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let job_id = job_id.to_string();
        let state: String = transaction.query_row(
            "SELECT state FROM executions WHERE job_id = ?1",
            [job_id.as_str()],
            |row| row.get(0),
        )?;
        let current: ExecutionState = state.parse()?;
        if !current.can_transition_to(next) {
            return Err(Error::InvalidInput(format!(
                "invalid execution transition {} -> {}",
                current.as_str(),
                next.as_str()
            )));
        }
        transaction.execute(
            "UPDATE executions SET state = ?1 WHERE job_id = ?2",
            params![next.as_str(), job_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn transition_delivery(&mut self, job_id: Uuid, next: DeliveryState) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let job_id = job_id.to_string();
        let state: String = transaction.query_row(
            "SELECT state FROM deliveries WHERE job_id = ?1",
            [job_id.as_str()],
            |row| row.get(0),
        )?;
        let current: DeliveryState = state.parse()?;
        if !current.can_transition_to(next) {
            return Err(Error::InvalidInput(format!(
                "invalid delivery transition {} -> {}",
                current.as_str(),
                next.as_str()
            )));
        }
        transaction.execute(
            "UPDATE deliveries SET state = ?1 WHERE job_id = ?2",
            params![next.as_str(), job_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn write_immutable_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let parent = path.parent().ok_or_else(|| {
            Error::InvalidInput("JSON destination has no parent directory".into())
        })?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(
            ".{}.{}.tmp",
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::InvalidInput(
                    "JSON destination has no UTF-8 file name".into()
                ))?,
            Uuid::now_v7()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        if let Err(error) = fs::hard_link(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        fs::remove_file(temporary)?;
        Ok(())
    }
}
