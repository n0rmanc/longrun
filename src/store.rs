use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryLease {
    pub job_id: Uuid,
    pub lease_id: Uuid,
    pub session_id: String,
    pub state: DeliveryState,
    pub expires_at_ms: i64,
    pub idempotency_key: String,
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
                session_id TEXT,
                state TEXT NOT NULL,
                lease_id TEXT,
                lease_owner TEXT,
                lease_expires_at_ms INTEGER,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                idempotency_key TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS leases (
                lease_id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
                owner TEXT NOT NULL,
                expires_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS session_locks (
                session_id TEXT PRIMARY KEY,
                owner TEXT NOT NULL,
                expires_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS integrations (
                integration TEXT PRIMARY KEY,
                manifest_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
             );",
        )?;
        let mut version: i64 =
            transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let fresh_schema = version == 0;
        if version < 2 {
            if version == 1 {
                transaction.execute_batch(
                    "ALTER TABLE executions ADD COLUMN cancel_requested_at_ms INTEGER;
                     ALTER TABLE executions ADD COLUMN cancel_grace_ms INTEGER;",
                )?;
            }
            version = 2;
        }
        if version < 3 {
            if !fresh_schema {
                transaction.execute_batch(
                    "ALTER TABLE deliveries ADD COLUMN session_id TEXT;
                     ALTER TABLE deliveries ADD COLUMN lease_owner TEXT;
                     ALTER TABLE deliveries ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            version = 3;
        }
        transaction.execute(
            "UPDATE deliveries
             SET idempotency_key = lower(hex(randomblob(16)))
             WHERE idempotency_key IS NULL",
            [],
        )?;
        if version > 0 {
            transaction.pragma_update(None, "user_version", version)?;
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
        self.create_job_for_session(specification, None)
    }

    pub fn create_job_for_session(
        &mut self,
        specification: &JobSpecification,
        session_id: Option<&str>,
    ) -> Result<()> {
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
            "INSERT INTO deliveries (job_id, session_id, state, idempotency_key)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                job_id,
                session_id,
                DeliveryState::Undelivered.as_str(),
                Uuid::now_v7().to_string(),
            ],
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
            "INSERT INTO deliveries (job_id, session_id, state, idempotency_key)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                job.job_id.to_string(),
                &pending.session_id,
                DeliveryState::Undelivered.as_str(),
                Uuid::now_v7().to_string(),
            ],
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

    pub fn retention_candidates(
        &self,
        now_ms: i64,
        max_age_days: u32,
        max_log_bytes: u64,
    ) -> Result<Vec<JobResult>> {
        let cutoff_ms = now_ms.saturating_sub(i64::from(max_age_days).saturating_mul(86_400_000));
        let mut statement = self.connection.prepare(
            "SELECT results.result_json
             FROM jobs
             JOIN executions USING (job_id)
             JOIN deliveries USING (job_id)
             JOIN results USING (job_id)
             WHERE executions.state IN ('succeeded', 'failed', 'timed_out', 'cancelled')
               AND deliveries.state IN ('delivered_in_turn', 'delivered_on_start', 'delivered_by_resume')
               AND NOT EXISTS (
                   SELECT 1 FROM leases
                   WHERE leases.job_id = jobs.job_id AND leases.expires_at_ms > ?1
               )
             ORDER BY results.created_at_ms ASC",
        )?;
        let candidates = statement
            .query_map([now_ms], |row| row.get::<_, String>(0))?
            .map(|row| {
                let result = row?;
                Ok(serde_json::from_str(&result)?)
            })
            .collect::<Result<Vec<JobResult>>>()?;
        let mut total_bytes =
            candidates
                .iter()
                .map(result_log_bytes)
                .try_fold(0_u64, |total, bytes| {
                    bytes.and_then(|bytes| {
                        total
                            .checked_add(bytes)
                            .ok_or_else(|| Error::Unavailable("retained logs exceed u64".into()))
                    })
                })?;
        let mut selected = Vec::new();
        for candidate in candidates {
            let bytes = result_log_bytes(&candidate)?;
            if candidate.completed_at_ms < cutoff_ms || total_bytes > max_log_bytes {
                total_bytes = total_bytes.saturating_sub(bytes);
                selected.push(candidate);
            }
        }
        Ok(selected)
    }

    pub fn delete_jobs(&mut self, jobs: &[Uuid]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for job_id in jobs {
            transaction.execute("DELETE FROM jobs WHERE job_id = ?1", [job_id.to_string()])?;
        }
        transaction.commit()?;
        Ok(())
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

    pub fn expire_delivery_leases(&mut self, now_ms: i64) -> Result<usize> {
        let transaction = self.connection.transaction()?;
        let expired = expire_delivery_leases(&transaction, now_ms)?;
        transaction.commit()?;
        Ok(expired)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_delivery(
        &mut self,
        job_id: Uuid,
        session_id: &str,
        state: DeliveryState,
        owner: &str,
        now_ms: i64,
        lease_ms: i64,
        retry_budget: u32,
    ) -> Result<DeliveryLease> {
        if !is_lease_state(state) {
            return Err(Error::InvalidInput(
                "delivery claim requires a leased state".into(),
            ));
        }
        if lease_ms <= 0 {
            return Err(Error::InvalidInput(
                "delivery lease duration must be positive".into(),
            ));
        }
        let expires_at_ms = now_ms
            .checked_add(lease_ms)
            .ok_or_else(|| Error::InvalidInput("delivery lease expiry is out of range".into()))?;
        let transaction = self.connection.transaction()?;
        expire_delivery_leases(&transaction, now_ms)?;
        let (stored_session, stored_state, attempts, idempotency_key, has_result): (
            Option<String>,
            String,
            i64,
            String,
            bool,
        ) = transaction.query_row(
            "SELECT session_id, state, attempt_count, idempotency_key,
                    EXISTS(SELECT 1 FROM results WHERE results.job_id = deliveries.job_id)
             FROM deliveries WHERE job_id = ?1",
            [job_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        if stored_session.as_deref() != Some(session_id) {
            return Err(Error::Denied(
                "delivery target does not match the requested session".into(),
            ));
        }
        if stored_state.parse::<DeliveryState>()? != DeliveryState::Undelivered {
            return Err(Error::Denied(
                "delivery is already owned or complete".into(),
            ));
        }
        if state != DeliveryState::HookLeased && !has_result {
            return Err(Error::Denied(
                "recovery delivery requires a completed job result".into(),
            ));
        }
        let attempts: u32 = attempts
            .try_into()
            .map_err(|_| Error::InvalidInput("invalid delivery attempt count".into()))?;
        if state == DeliveryState::ResumeLeased && attempts >= retry_budget {
            return Err(Error::Denied("resume retry budget is exhausted".into()));
        }

        let lease_id = Uuid::now_v7();
        if needs_session_lock(state)
            && transaction.execute(
                "INSERT INTO session_locks (session_id, owner, expires_at_ms)
                 VALUES (?1, ?2, ?3) ON CONFLICT(session_id) DO NOTHING",
                params![session_id, lease_id.to_string(), expires_at_ms],
            )? != 1
        {
            return Err(Error::Denied(
                "session recovery is already owned by another delivery".into(),
            ));
        }
        let next_attempts = attempts
            .checked_add(u32::from(state == DeliveryState::ResumeLeased))
            .ok_or_else(|| Error::InvalidInput("delivery attempt count is out of range".into()))?;
        let changed = transaction.execute(
            "UPDATE deliveries
             SET state = ?1, lease_id = ?2, lease_owner = ?3, lease_expires_at_ms = ?4,
                 attempt_count = ?5
             WHERE job_id = ?6 AND state = 'undelivered'",
            params![
                state.as_str(),
                lease_id.to_string(),
                owner,
                expires_at_ms,
                next_attempts as i64,
                job_id.to_string(),
            ],
        )?;
        if changed != 1 {
            return Err(Error::Denied("delivery could not be claimed".into()));
        }
        transaction.execute(
            "INSERT INTO leases (lease_id, job_id, owner, expires_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                lease_id.to_string(),
                job_id.to_string(),
                owner,
                expires_at_ms
            ],
        )?;
        transaction.commit()?;
        Ok(DeliveryLease {
            job_id,
            lease_id,
            session_id: session_id.into(),
            state,
            expires_at_ms,
            idempotency_key,
        })
    }

    pub fn finish_delivery(
        &mut self,
        job_id: Uuid,
        lease_id: Uuid,
        next: DeliveryState,
        now_ms: i64,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        expire_delivery_leases(&transaction, now_ms)?;
        let (current, stored_lease, session_id): (String, Option<String>, Option<String>) =
            transaction.query_row(
                "SELECT state, lease_id, session_id FROM deliveries WHERE job_id = ?1",
                [job_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        let current: DeliveryState = current.parse()?;
        if stored_lease.as_deref() != Some(&lease_id.to_string())
            || !matches!(
                (current, next),
                (DeliveryState::HookLeased, DeliveryState::DeliveredInTurn)
                    | (
                        DeliveryState::SessionStartLeased,
                        DeliveryState::DeliveredOnStart
                    )
            )
        {
            return Err(Error::Denied(
                "delivery lease cannot mark this result delivered".into(),
            ));
        }
        transaction.execute(
            "UPDATE deliveries
             SET state = ?1, lease_id = NULL, lease_owner = NULL, lease_expires_at_ms = NULL
             WHERE job_id = ?2 AND lease_id = ?3",
            params![next.as_str(), job_id.to_string(), lease_id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM leases WHERE lease_id = ?1",
            [lease_id.to_string()],
        )?;
        if let Some(session_id) = session_id {
            transaction.execute(
                "DELETE FROM session_locks WHERE session_id = ?1 AND owner = ?2",
                params![session_id, lease_id.to_string()],
            )?;
        }
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

fn expire_delivery_leases(transaction: &Transaction<'_>, now_ms: i64) -> Result<usize> {
    let expired = transaction.execute(
        "UPDATE deliveries
         SET state = 'undelivered', lease_id = NULL, lease_owner = NULL, lease_expires_at_ms = NULL
         WHERE state IN ('hook_leased', 'session_start_leased', 'resume_leased')
           AND lease_expires_at_ms <= ?1",
        [now_ms],
    )?;
    transaction.execute("DELETE FROM leases WHERE expires_at_ms <= ?1", [now_ms])?;
    transaction.execute(
        "DELETE FROM session_locks WHERE expires_at_ms <= ?1",
        [now_ms],
    )?;
    Ok(expired)
}

const fn is_lease_state(state: DeliveryState) -> bool {
    matches!(
        state,
        DeliveryState::HookLeased | DeliveryState::SessionStartLeased | DeliveryState::ResumeLeased
    )
}

const fn needs_session_lock(state: DeliveryState) -> bool {
    matches!(
        state,
        DeliveryState::SessionStartLeased | DeliveryState::ResumeLeased
    )
}

fn result_log_bytes(result: &JobResult) -> Result<u64> {
    [
        result.stdout_log.to_os_string()?,
        result.stderr_log.to_os_string()?,
    ]
    .into_iter()
    .try_fold(0_u64, |total, path| match fs::metadata(path) {
        Ok(metadata) => total
            .checked_add(metadata.len())
            .ok_or_else(|| Error::Unavailable("job log bytes exceed u64".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(total),
        Err(error) => Err(error.into()),
    })
}
