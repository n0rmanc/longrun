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
    protocol::{DeliveryState, ExecutionState, JobSpecification},
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
