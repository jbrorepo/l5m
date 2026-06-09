//! Write-ahead log for durable real-time writes.
//!
//! Each mutation (insert/delete) is appended as a JSON line and fsync'd before
//! the call returns, so acknowledged writes survive a crash/restart. On open,
//! the store replays the WAL to rebuild its in-memory delta and tombstones.
//! Compaction folds the live state into a base segment and truncates the WAL.
//!
//! Recovery is idempotent: replaying an insert re-replaces by id; replaying a
//! delete re-tombstones. So a crash between "checkpoint written" and "WAL
//! truncated" still converges to a consistent state.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WalOp {
    /// A capsule in the JSON shape `compiler::capsule_from_json` accepts.
    Insert {
        capsule: serde_json::Value,
    },
    Delete {
        id: String,
    },
}

pub struct Wal {
    file: File,
    path: PathBuf,
}

impl Wal {
    /// Open (creating if absent) a WAL for appending.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { file, path })
    }

    /// Append + durably flush a single operation.
    pub fn append(&mut self, op: &WalOp) -> Result<()> {
        let line = serde_json::to_string(op)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Read all operations recorded so far (for replay on startup).
    pub fn replay(path: impl AsRef<Path>) -> Result<Vec<WalOp>> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut ops = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            ops.push(serde_json::from_str(&line)?);
        }
        Ok(ops)
    }

    /// Truncate the log to empty (after a successful checkpoint/compaction).
    pub fn truncate(&mut self) -> Result<()> {
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        // Re-open in append mode for subsequent writes.
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        Ok(())
    }
}
