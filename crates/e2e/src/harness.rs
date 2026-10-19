use std::path::Path;

use anyhow::{Context, Result};
use sies_chain::rebuilder::Rebuilder;
use sies_chain::smt::SmtStateStore;
use sies_storage::{FileLedger, Ledger, LedgerEntry};
use tempfile::TempDir;

/// Self-contained test node: owns a temp directory, a file-backed ledger,
/// and an in-memory SMT rebuilt from that ledger.
pub struct TestNode {
    /// Kept alive so the temp directory is not deleted while the node lives.
    _dir: TempDir,
    dir_path: std::path::PathBuf,
    ledger: FileLedger,
    smt: SmtStateStore,
}

impl TestNode {
    /// Cold start: empty ledger → rebuild produces genesis SMT.
    pub fn start() -> Result<Self> {
        let dir = TempDir::new().context("create temp dir")?;
        let ledger = FileLedger::open(dir.path()).context("open ledger")?;
        let smt = Rebuilder::new(&ledger)
            .rebuild()
            .map_err(|e| anyhow::anyhow!("cold-start rebuild: {e}"))?;
        Ok(Self {
            dir_path: dir.path().to_path_buf(),
            _dir: dir,
            ledger,
            smt,
        })
    }

    /// Restart: re-open the same ledger directory and rebuild SMT from scratch.
    /// Simulates a node crash + recovery cycle.
    pub fn restart(&mut self) -> Result<()> {
        let new_ledger = FileLedger::open(&self.dir_path).context("reopen ledger on restart")?;
        let new_smt = Rebuilder::new(&new_ledger)
            .rebuild()
            .map_err(|e| anyhow::anyhow!("restart rebuild: {e}"))?;
        self.ledger = new_ledger;
        self.smt = new_smt;
        Ok(())
    }

    /// Append entries one-by-one (each gets its own seq assigned by the ledger).
    pub fn append_entries(&mut self, entries: &[LedgerEntry]) -> Result<()> {
        for entry in entries {
            self.ledger
                .append(entry)
                .map_err(|e| anyhow::anyhow!("append seq error: {e}"))?;
        }
        Ok(())
    }

    /// Rebuild SMT from current ledger state without restarting.
    /// Useful after appending to verify incremental consistency.
    pub fn rebuild_smt(&mut self) -> Result<()> {
        self.smt = Rebuilder::new(&self.ledger)
            .rebuild()
            .map_err(|e| anyhow::anyhow!("incremental rebuild: {e}"))?;
        Ok(())
    }

    pub fn last_seq(&self) -> Result<u64> {
        self.ledger
            .last_seq()
            .map_err(|e| anyhow::anyhow!("last_seq: {e}"))
    }

    #[must_use]
    pub fn smt_root(&self) -> [u8; 32] {
        self.smt.root_hash()
    }

    #[must_use]
    pub fn smt(&self) -> &SmtStateStore {
        &self.smt
    }

    #[must_use]
    pub fn ledger(&self) -> &FileLedger {
        &self.ledger
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir_path
    }
}
