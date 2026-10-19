//! Append-only file-backed [`Ledger`] implementation.
//!
//! ## On-disk format (per record)
//! ```text
//! [4B payload_len LE][payload_len B bincode-encoded LedgerEntry][4B CRC32 LE]
//! ```
//!
//! ## Index file (`ledger.idx`)
//! ```text
//! [8B file_offset LE] × N   (one entry per ledger entry, in sequence order)
//! ```

use crate::{Ledger, LedgerEntry, RawLedgerEntry, Result, StorageError};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct FileLedger {
    log_path: PathBuf,
    idx_path: PathBuf,
    log_file: File,
    offsets: Vec<u64>,
    write_pos: u64,
}

impl FileLedger {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).map_err(StorageError::Io)?;

        let log_path = dir.join("ledger.dat");
        let idx_path = dir.join("ledger.idx");

        let mut log_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&log_path)
            .map_err(StorageError::Io)?;

        let write_pos = log_file.seek(SeekFrom::End(0)).map_err(StorageError::Io)?;

        let offsets = if idx_path.exists() {
            Self::load_index(&idx_path)?
        } else {
            Self::rebuild_offsets(&log_path, write_pos)?
        };

        Ok(Self {
            log_path,
            idx_path,
            log_file,
            offsets,
            write_pos,
        })
    }

    fn load_index(idx_path: &Path) -> Result<Vec<u64>> {
        let data = fs::read(idx_path).map_err(StorageError::Io)?;
        if data.len() % 8 != 0 {
            return Err(StorageError::Backend(
                "corrupt index file: length not multiple of 8".into(),
            ));
        }
        Ok(data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect())
    }

    fn save_index(&self) -> Result<()> {
        let mut buf = Vec::with_capacity(self.offsets.len() * 8);
        for &off in &self.offsets {
            buf.extend_from_slice(&off.to_le_bytes());
        }
        fs::write(&self.idx_path, buf).map_err(StorageError::Io)
    }

    fn rebuild_offsets(log_path: &Path, file_size: u64) -> Result<Vec<u64>> {
        if file_size == 0 {
            return Ok(Vec::new());
        }
        let mut f = File::open(log_path).map_err(StorageError::Io)?;
        let mut offsets = Vec::new();
        let mut pos: u64 = 0;
        while pos < file_size {
            offsets.push(pos);
            let mut len_buf = [0u8; 4];
            f.read_exact(&mut len_buf).map_err(StorageError::Io)?;
            let payload_len = u64::from(u32::from_le_bytes(len_buf));
            f.seek(SeekFrom::Current((payload_len + 4).cast_signed()))
                .map_err(StorageError::Io)?;
            pos += 4 + payload_len + 4;
        }
        Ok(offsets)
    }

    fn read_record_at(&self, offset: u64) -> Result<LedgerEntry> {
        let mut f = File::open(&self.log_path).map_err(StorageError::Io)?;
        f.seek(SeekFrom::Start(offset)).map_err(StorageError::Io)?;

        let mut len_buf = [0u8; 4];
        f.read_exact(&mut len_buf).map_err(StorageError::Io)?;
        let payload_len = u32::from_le_bytes(len_buf) as usize;

        let mut payload = vec![0u8; payload_len];
        f.read_exact(&mut payload).map_err(StorageError::Io)?;

        let mut crc_buf = [0u8; 4];
        f.read_exact(&mut crc_buf).map_err(StorageError::Io)?;
        let stored_crc = u32::from_le_bytes(crc_buf);
        let computed_crc = crc32fast::hash(&payload);
        if stored_crc != computed_crc {
            return Err(StorageError::Backend(format!(
                "CRC mismatch at offset {offset}: stored={stored_crc:#x}, computed={computed_crc:#x}"
            )));
        }

        let raw: RawLedgerEntry =
            postcard::from_bytes(&payload).map_err(|e| StorageError::Deserialize(e.to_string()))?;
        Ok(LedgerEntry::from(raw))
    }

    fn write_record(&mut self, entry: &LedgerEntry) -> Result<u64> {
        let raw = RawLedgerEntry::from(entry);
        let payload =
            postcard::to_allocvec(&raw).map_err(|e| StorageError::Serialize(e.to_string()))?;
        let crc = crc32fast::hash(&payload);
        let offset = self.write_pos;
        self.offsets.push(offset);

        #[allow(clippy::cast_possible_truncation)] // protocol guarantees payload ≤ 4 GiB
        self.log_file
            .write_all(&(payload.len() as u32).to_le_bytes())
            .map_err(StorageError::Io)?;
        self.log_file
            .write_all(&payload)
            .map_err(StorageError::Io)?;
        self.log_file
            .write_all(&crc.to_le_bytes())
            .map_err(StorageError::Io)?;
        self.log_file.flush().map_err(StorageError::Io)?;

        self.write_pos += 4 + payload.len() as u64 + 4;
        Ok(self.offsets.len() as u64)
    }
}

impl Ledger for FileLedger {
    fn append(&mut self, entry: &LedgerEntry) -> Result<u64> {
        let seq = self.write_record(entry)?;
        self.save_index()?;
        Ok(seq)
    }

    fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<LedgerEntry>> {
        // sequence numbers fit in usize on all supported targets
        #[allow(clippy::cast_possible_truncation)]
        let start_idx = if start_seq == 0 {
            0
        } else {
            (start_seq - 1) as usize
        };
        // end_seq 是排他上界（半开区间），对应 index = end_seq - 1
        // 但当 end_seq == 0 时不应下溢
        let end_idx = if end_seq == 0 {
            0
        } else {
            #[allow(clippy::cast_possible_truncation)]
            ((end_seq - 1) as usize).min(self.offsets.len())
        };
        (start_idx..end_idx)
            .map(|i| self.read_record_at(self.offsets[i]))
            .collect()
    }

    fn last_seq(&self) -> Result<u64> {
        Ok(self.offsets.len() as u64)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use sies_core::belief::Belief;
    use tempfile::TempDir;

    fn sample_entry(round: u64, user_id: u64) -> LedgerEntry {
        LedgerEntry::new(
            1, // seq
            round,
            user_id,
            Belief::new(vec![Decimal::new(7, 1), Decimal::new(3, 1)]).unwrap(),
            Belief::new(vec![Decimal::ONE, Decimal::ZERO]).unwrap(),
            Decimal::from(2u64),                   // new_alpha
            Decimal::ONE,                          // new_beta
            Decimal::new(-356_674_943_938i64, 12), // score
        )
    }

    #[test]
    fn empty_ledger() {
        let dir = TempDir::new().unwrap();
        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 0);
        // assert!(ledger.read_range(0, 10).unwrap().is_empty());
        assert_eq!(ledger.read_range(0, 10).unwrap(), [] as [LedgerEntry; 0]);
    }

    #[test]
    fn append_and_read_back() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        let seq = ledger.append(&sample_entry(1, 42)).unwrap();
        assert_eq!(seq, 1);
        let got = ledger.read_range(1, 2).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].user_id, 42);
    }

    #[test]
    fn range_query() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        for r in 1..=10 {
            ledger.append(&sample_entry(r, 1)).unwrap();
        }
        let range = ledger.read_range(3, 7).unwrap();
        assert_eq!(range.len(), 4);
        assert_eq!(range[0].round, 3);
        assert_eq!(range[3].round, 6);
    }

    #[test]
    fn persistence_across_reopen() {
        let dir = TempDir::new().unwrap();
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            ledger.append(&sample_entry(1, 10)).unwrap();
            ledger.append(&sample_entry(2, 20)).unwrap();
        }
        {
            let ledger = FileLedger::open(dir.path()).unwrap();
            assert_eq!(ledger.last_seq().unwrap(), 2);
            let all = ledger.read_range(1, 3).unwrap();
            assert_eq!(all.len(), 2);
            assert_eq!(all[0].user_id, 10);
            assert_eq!(all[1].user_id, 20);
        }
    }

    #[test]
    fn out_of_range_returns_partial() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        ledger.append(&sample_entry(1, 1)).unwrap();
        assert_eq!(ledger.read_range(1, 100).unwrap().len(), 1);
    }
}
