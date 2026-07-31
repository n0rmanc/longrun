use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use rusqlite::{Connection, params};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::{DeliveryState, ExecutionState, JobSpecification, PendingState, PendingSubmission},
};

pub struct Store {
    connection: Connection,
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
                finished_at_ms INTEGER
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
             );
             PRAGMA user_version = 1;",
        )?;
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

    pub fn claim_pending_by_token(
        &mut self,
        token_hash: &str,
        now_ms: i64,
    ) -> Result<PendingSubmission> {
        let transaction = self.connection.transaction()?;
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

    pub fn execution_state(&self, job_id: Uuid) -> Result<ExecutionState> {
        let state: String = self.connection.query_row(
            "SELECT state FROM executions WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )?;
        state.parse()
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
