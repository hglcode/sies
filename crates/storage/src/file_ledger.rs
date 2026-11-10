//! Append-only file-backed [`Ledger`] implementation.
//!
//! ## On-disk format (per record)
//! ```text
//! [4B payload_len LE][payload_len B postcard-encoded RawLedgerEntry][4B CRC32 LE]
//! ```
//!
//! ## Index file (`ledger.idx`)
//! ```text
//! [8B file_offset LE] × N   (one entry per ledger entry, in sequence order)
//! ```
//!
//! ## 崩溃一致性设计（#44 整改，审计 P1-2/3/4）
//!
//! **日志是真相，索引是派生缓存**——索引可随时由日志重建，因此只有
//! 日志写入构成耐久性边界：
//!
//! * 写序: 整批记录单次 `write_all` → `sync_data`(fdatasync，P1-2) →
//!   索引 8B×n 单次追加（**不 fsync**，丢失只牺牲下次启动速度）
//! * 崩溃落在"日志已同步、索引未追加"窗口 → [`FileLedger::open`]
//!   从最后已知完好记录尾部 tail-scan 补齐索引（不丢）
//! * 索引撕裂尾（长度非 8 倍数）→ 截断到最近倍数；结构非法 → 重建
//! * 日志撕裂尾（半写 / 垃圾长度字段）→ 校验失败处截断日志
//!   （未 fsync 的记录本就未提交，截断即"不重"）
//! * 读取复用常驻句柄 + pread 定位读（P1-4），消除逐条
//!   open/seek/close 系统调用风暴
//! * 索引追加式 → 写放大 O(n)（P1-3）；全量重写仅发生于恢复路径，
//!   且经 temp+rename 原子替换
//!
//! 磁盘格式版本化/迁移归 #57；离线重放与校验 CLI 归 #47。

use crate::{Ledger, LedgerEntry, RawLedgerEntry, Result, StorageError};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 单条记录 payload 净荷上限（1 MiB）。
/// `LedgerEntry` 语义尺寸远小于此（数百字节量级）；超限的长度字段
/// 一律按撕裂/损坏头部处理，防止损坏记录触发巨型分配。
const MAX_PAYLOAD_LEN: u64 = 1 << 20;

pub struct FileLedger {
    /// 日志句柄: `O_APPEND` 写入 + pread 读取复用（P1-4）。
    /// 不变式: `write_pos == 文件当前长度`（append 模式写入定位在内核 EOF）。
    log_file: File,
    /// 索引追加句柄（追加式索引，P1-3）。
    idx_file: File,
    /// 每条记录的日志起始偏移（位置即 seq: 第 i 条 → offsets[i-1]）。
    offsets: Vec<u64>,
    write_pos: u64,
}

// ─── 定位读取（pread 语义，P1-4）────────────────────────────────

/// 在 `offset` 处精确读取 `buf` 字节，不移动文件游标，
/// 天然规避共享句柄上的 seek 竞争。
#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    FileExt::read_exact_at(file, buf, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < buf.len() {
        let n = file.seek_read(&mut buf[done..], offset + done as u64)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "positioned read hit EOF",
            ));
        }
        done += n;
    }
    Ok(())
}

/// 读取并校验 `offset` 处的完整记录，返回 (payload, 记录结束偏移)。
/// 任何越界、超限长度或 CRC 失败 → `None`（撕裂/损坏）。
/// 每条记录 2 次 pread: 4B 头 + (payload ‖ 4B CRC)。
fn read_record_bytes(log_file: &File, offset: u64, log_size: u64) -> Option<(Vec<u8>, u64)> {
    let header_end = offset.checked_add(4)?;
    if header_end > log_size {
        return None; // 头部未写全
    }
    let mut len_buf = [0u8; 4];
    read_exact_at(log_file, &mut len_buf, offset).ok()?;
    let payload_len = u64::from(u32::from_le_bytes(len_buf));
    if payload_len > MAX_PAYLOAD_LEN {
        return None; // 撕裂/损坏的长度字段
    }
    #[allow(clippy::cast_possible_truncation)] // bounded by MAX_PAYLOAD_LEN (≤ 1 MiB)
    let payload_len = payload_len as usize;
    let end = header_end.checked_add(payload_len as u64)?.checked_add(4)?;
    if end > log_size {
        return None; // 净荷或 CRC 未写全（半写）
    }
    let mut body = vec![0u8; payload_len + 4];
    read_exact_at(log_file, &mut body, header_end).ok()?;
    let crc_slice = body.split_off(payload_len);
    let payload = body;
    let crc_bytes: [u8; 4] = crc_slice.try_into().ok()?;
    if crc32fast::hash(&payload) != u32::from_le_bytes(crc_bytes) {
        return None; // 内容损坏
    }
    Some((payload, end))
}

/// 探测 `offset` 处是否存在完整且 CRC 正确的记录，返回其结束偏移。
fn probe_record_at(log_file: &File, offset: u64, log_size: u64) -> Option<u64> {
    read_record_bytes(log_file, offset, log_size).map(|(_, end)| end)
}

impl FileLedger {
    /// 打开（或创建）ledger 并执行启动一致性校验（索引 ↔ 日志尾部）。
    ///
    /// 修复动作（日志为真相，索引为派生缓存）:
    /// * 索引撕裂尾（长度非 8 倍数）→ 截断；索引结构非法 → 丢弃重建
    /// * 索引指向的尾部记录校验失败 → 弹出该索引项（防御纵深）
    /// * 日志存在已写入但未入索引的记录（崩溃窗口）→ tail-scan 补齐
    /// * 日志撕裂/垃圾尾 → 截断到最后一条完好记录
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).map_err(StorageError::Io)?;

        let log_path = dir.join("ledger.dat");
        let idx_path = dir.join("ledger.idx");

        // O_APPEND 不影响 pread；读写共用同一句柄（P1-4）
        let log_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&log_path)
            .map_err(StorageError::Io)?;
        let log_size = log_file.metadata().map_err(StorageError::Io)?.len();

        // ── 1. 读取索引（撕裂尾截断为最近 8B 倍数）──
        let index_existed = idx_path.exists();
        let mut offsets: Vec<u64> = Vec::new();
        let mut index_dirty = false;
        if index_existed {
            let data = fs::read(&idx_path).map_err(StorageError::Io)?;
            let good_len = data.len() - data.len() % 8;
            if good_len != data.len() {
                index_dirty = true; // 撕裂的 8B 槽位
            }
            offsets = data
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| u64::from_le_bytes(*c))
                .collect();
            // 位置索引结构不变量: 首条 offset == 0 且严格递增
            if offsets.first().is_some_and(|&o| o != 0) || offsets.windows(2).any(|w| w[0] >= w[1])
            {
                offsets.clear();
                index_dirty = true;
            }
        }
        let loaded_count = offsets.len();

        // ── 2. 防御纵深: 已索引尾部必须映射到完整且 CRC 正确的记录 ──
        // （按写序索引只会滞后不会超前；此处兜底弹出非法尾部项）
        let mut validated_end: Option<u64> = None;
        while let Some(&last) = offsets.last() {
            if let Some(end) = probe_record_at(&log_file, last, log_size) {
                validated_end = Some(end);
                break;
            }
            offsets.pop();
            index_dirty = true;
        }

        // ── 3. tail-scan: 补齐崩溃窗口内已入日志但未入索引的记录；
        //       撕裂/损坏尾 → 截断日志（未 fsync 即未提交）──
        let mut scan_pos = validated_end.unwrap_or(0);
        let mut log_truncated = false;
        while scan_pos < log_size {
            if let Some(end) = probe_record_at(&log_file, scan_pos, log_size) {
                offsets.push(scan_pos);
                scan_pos = end;
            } else {
                log_file.set_len(scan_pos).map_err(StorageError::Io)?;
                log_truncated = true;
                break;
            }
        }
        let write_pos = scan_pos;

        // ── 4. 索引落盘: 干净打开零写入（无写放大）；任何修复 →
        //       temp+rename 原子重写（仅恢复路径）──
        if index_dirty || log_truncated || offsets.len() != loaded_count {
            Self::rewrite_index_atomic(&idx_path, &offsets)?;
        }

        let idx_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&idx_path)
            .map_err(StorageError::Io)?;

        Ok(Self {
            log_file,
            idx_file,
            offsets,
            write_pos,
        })
    }

    /// 全量重写索引，经 temp+rename 原子替换（仅恢复路径使用）。
    /// 索引是派生缓存，即使 rename 未持久化，下次 open 也会重建，
    /// 故不对 tmp 文件或目录做 fsync。
    fn rewrite_index_atomic(idx_path: &Path, offsets: &[u64]) -> Result<()> {
        let mut tmp_name = idx_path.as_os_str().to_os_string();
        tmp_name.push(".tmp");
        let tmp_path = PathBuf::from(tmp_name);

        let mut buf = Vec::with_capacity(offsets.len() * 8);
        for &off in offsets {
            buf.extend_from_slice(&off.to_le_bytes());
        }
        fs::write(&tmp_path, buf).map_err(StorageError::Io)?;
        fs::rename(&tmp_path, idx_path).map_err(StorageError::Io)?;
        Ok(())
    }

    /// 读取 `offset` 处记录（常驻句柄 pread，P1-4）。
    fn read_record_at(&self, offset: u64) -> Result<LedgerEntry> {
        let Some((payload, _end)) = read_record_bytes(&self.log_file, offset, self.write_pos)
        else {
            return Err(StorageError::Backend(format!(
                "no complete, CRC-valid record at offset {offset}"
            )));
        };
        let raw: RawLedgerEntry =
            postcard::from_bytes(&payload).map_err(|e| StorageError::Deserialize(e.to_string()))?;
        Ok(LedgerEntry::from(raw))
    }

    /// 编码单条记录的磁盘字节串: [4B len][payload][4B CRC]。
    fn encode_record(entry: &LedgerEntry) -> Result<Vec<u8>> {
        let raw = RawLedgerEntry::from(entry);
        let payload =
            postcard::to_allocvec(&raw).map_err(|e| StorageError::Serialize(e.to_string()))?;
        let mut buf = Vec::with_capacity(payload.len() + 8);
        #[allow(clippy::cast_possible_truncation)]
        // protocol guarantees payload ≤ 4 GiB (u32 len field)
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&payload);
        buf.extend_from_slice(&crc32fast::hash(&payload).to_le_bytes());
        Ok(buf)
    }

    /// 组提交（P1-2/P1-3）: 整批记录单次 `write_all` + 单次 `sync_data`，
    /// 索引 8B×n 单次追加（无 fsync —— 派生缓存）。
    ///
    /// 写失败（如 ENOSPC 半写）→ best-effort `set_len(write_pos)` 回滚，
    /// 维持 "`write_pos` == 文件长度" 不变式，返回 `Err` 表示本批未入账。
    fn append_batch_impl(&mut self, entries: &[LedgerEntry]) -> Result<Vec<u64>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let mut log_buf = Vec::new();
        let mut rec_lens: Vec<usize> = Vec::with_capacity(entries.len());
        for entry in entries {
            let rec = Self::encode_record(entry)?;
            rec_lens.push(rec.len());
            log_buf.extend_from_slice(&rec);
        }

        // 1. 日志: 单次写 + fdatasync —— 耐久性边界。
        //    File 无用户态缓冲，write_all 直达内核，无需额外 flush
        if let Err(e) = self.log_file.write_all(&log_buf) {
            let _ = self.log_file.set_len(self.write_pos); // 半写回滚
            return Err(StorageError::Io(e));
        }
        if let Err(e) = self.log_file.sync_data() {
            let _ = self.log_file.set_len(self.write_pos); // 同步失败回滚
            return Err(StorageError::Io(e));
        }

        // 2. 索引: 追加 8B×n（无 fsync，可由日志重建）
        let mut idx_buf = Vec::with_capacity(entries.len() * 8);
        let mut new_offsets: Vec<u64> = Vec::with_capacity(entries.len());
        let mut seqs: Vec<u64> = Vec::with_capacity(entries.len());
        let mut offset = self.write_pos;
        let base_seq = self.offsets.len() as u64;
        for (i, len) in rec_lens.iter().enumerate() {
            idx_buf.extend_from_slice(&offset.to_le_bytes());
            new_offsets.push(offset);
            seqs.push(base_seq + i as u64 + 1);
            offset += *len as u64;
        }
        self.idx_file
            .write_all(&idx_buf)
            .map_err(StorageError::Io)?;

        // 3. 提交内存态
        self.offsets.extend_from_slice(&new_offsets);
        self.write_pos = offset;
        Ok(seqs)
    }
}

impl Ledger for FileLedger {
    fn append(&mut self, entry: &LedgerEntry) -> Result<u64> {
        // 单条 append = 批量为 1 的组提交（同样 write_all + sync_data）
        let seqs = self.append_batch_impl(std::slice::from_ref(entry))?;
        match seqs.into_iter().next() {
            Some(seq) => Ok(seq),
            None => Err(StorageError::Backend(
                "append_batch_impl returned no seq for a single entry".into(),
            )),
        }
    }

    fn append_batch(&mut self, entries: &[LedgerEntry]) -> Result<Vec<u64>> {
        self.append_batch_impl(entries)
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

    fn entry_i(i: u64) -> LedgerEntry {
        sample_entry(i, 10_000 + i)
    }

    fn append_bytes(path: &Path, bytes: &[u8]) {
        let mut f = OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    fn truncate_file(path: &Path, len: u64) {
        let f = OpenOptions::new().write(true).open(path).unwrap();
        f.set_len(len).unwrap();
    }

    fn idx_size(dir: &Path) -> u64 {
        fs::metadata(dir.join("ledger.idx")).unwrap().len()
    }

    fn read_offsets(dir: &Path) -> Vec<u64> {
        let data = fs::read(dir.join("ledger.idx")).unwrap();
        data.as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect()
    }

    #[test]
    fn empty_ledger() {
        let dir = TempDir::new().unwrap();
        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 0);
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

    // ─── #44: 组提交 / 崩溃一致性 / 写放大 / 恢复 ────────────────

    #[test]
    fn append_batch_assigns_sequential_seqs() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        let entries: Vec<LedgerEntry> = (1..=5).map(entry_i).collect();
        let seqs = ledger.append_batch(&entries).unwrap();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
        assert_eq!(ledger.last_seq().unwrap(), 5);
        let all = ledger.read_range(1, 6).unwrap();
        assert_eq!(all.len(), 5);
        for (i, e) in all.iter().enumerate() {
            assert_eq!(e.user_id, 10_001 + i as u64);
        }
    }

    #[test]
    fn append_batch_empty_is_noop() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        ledger.append(&sample_entry(1, 1)).unwrap();
        let before = idx_size(dir.path());
        let seqs = ledger.append_batch(&[]).unwrap();
        assert_eq!(seqs, [] as [u64; 0]);
        assert_eq!(idx_size(dir.path()), before);
        assert_eq!(ledger.last_seq().unwrap(), 1);
    }

    #[test]
    fn index_write_amplification_is_linear_and_rebuild_meets_baseline() {
        // 验收: ① 索引写放大 O(n) —— 10 万条 append 后索引恰为 8B×n;
        //       ② 删索引全量重建（CRC 全扫）耗时 < 5s（#12 基线口径）
        const N: u64 = 100_000;
        const CHUNK: usize = 1_000;
        let dir = TempDir::new().unwrap();
        let idx_path = dir.path().join("ledger.idx");

        let t0 = std::time::Instant::now();
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for chunk_start in (0..N).step_by(CHUNK) {
                let entries: Vec<LedgerEntry> = (chunk_start..chunk_start + CHUNK as u64)
                    .map(entry_i)
                    .collect();
                ledger.append_batch(&entries).unwrap();
            }
            // 抽样读取验证批量写入路径的完整性
            let sample = ledger.read_range(50_000, 50_010).unwrap();
            assert_eq!(sample.len(), 10);
            assert_eq!(sample[0].user_id, 59_999);
            assert_eq!(ledger.last_seq().unwrap(), N);
        }
        let append_elapsed = t0.elapsed();

        // ① O(n) 写放大: 索引恰 8B × 100_000（旧实现此处为 ~40GB）
        assert_eq!(fs::metadata(&idx_path).unwrap().len(), 8 * N);

        // ② 全量重建: 删索引后 reopen 触发日志全量 CRC 扫描
        fs::remove_file(&idx_path).unwrap();
        let t1 = std::time::Instant::now();
        let ledger = FileLedger::open(dir.path()).unwrap();
        let rebuild_elapsed = t1.elapsed();
        assert_eq!(ledger.last_seq().unwrap(), N);
        let sample = ledger.read_range(99_990, 100_000).unwrap();
        assert_eq!(sample.len(), 10);
        assert_eq!(sample[0].user_id, 109_989);
        assert_eq!(fs::metadata(&idx_path).unwrap().len(), 8 * N);

        println!("append(100k, batch=1000): {append_elapsed:?}, rebuild: {rebuild_elapsed:?}");
        assert!(
            rebuild_elapsed < std::time::Duration::from_secs(5),
            "full rebuild exceeded #12 baseline (5s): {rebuild_elapsed:?}"
        );
    }

    #[test]
    fn crash_between_log_sync_and_index_write_recovers() {
        // 崩溃窗口: 日志记录已 write+fsync，索引 8B 尚未追加。
        // 模拟: 正常写 2 条后，绕过 ledger 直接把第 3 条记录字节写进日志
        let dir = TempDir::new().unwrap();
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            ledger.append(&entry_i(1)).unwrap();
            ledger.append(&entry_i(2)).unwrap();
        }
        let rec3 = FileLedger::encode_record(&entry_i(3)).unwrap();
        append_bytes(&dir.path().join("ledger.dat"), &rec3);

        // reopen: tail-scan 应补齐第 3 条（不丢），seq 不重复
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 3);
        assert_eq!(idx_size(dir.path()), 24);
        let all = ledger.read_range(1, 4).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].user_id, 10_003);

        // 后续追加不重叠、不跳号（不重）
        let seq = ledger.append(&entry_i(4)).unwrap();
        assert_eq!(seq, 4);
        assert_eq!(ledger.read_range(4, 5).unwrap()[0].user_id, 10_004);
        drop(ledger);
        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 4);
    }

    #[test]
    fn torn_log_tail_truncated_on_reopen() {
        // 半写: 最后一条记录只写了一部分 → 未提交，reopen 截断且不丢前序
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("ledger.dat");
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for i in 1..=3 {
                ledger.append(&entry_i(i)).unwrap();
            }
        }
        let offsets = read_offsets(dir.path());
        assert_eq!(offsets.len(), 3);
        // 截到第 3 条记录净荷中间
        truncate_file(&log_path, offsets[2] + 10);

        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 2); // 第 3 条被截掉
        assert_eq!(ledger.read_range(1, 3).unwrap().len(), 2);
        assert_eq!(ledger.read_range(3, 4).unwrap().len(), 0);
        // 日志截回第 2 条记录边界（= 第 3 条原起始偏移），索引同步修复
        assert_eq!(fs::metadata(&log_path).unwrap().len(), offsets[2]);
        assert_eq!(idx_size(dir.path()), 16);
    }

    #[test]
    fn garbage_log_tail_truncated_on_reopen() {
        use std::io::Write;

        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("ledger.dat");

        // 3 条合法记录落盘
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            ledger.append(&sample_entry(1, 11)).unwrap();
            ledger.append(&sample_entry(2, 22)).unwrap();
            ledger.append(&sample_entry(3, 33)).unwrap();
        }
        let clean_size = std::fs::metadata(&log_path).unwrap().len();

        // 手术：日志末尾追加 10 字节垃圾——头部伪造成 len=2 的"合法长度"，
        // 但 payload 与 CRC 均为垃圾，确保走的是 CRC/反序列化校验失败而非长度越界
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap();
        f.write_all(&[0x02, 0x00, 0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE])
            .unwrap();
        f.sync_all().unwrap();
        drop(f);
        assert!(std::fs::metadata(&log_path).unwrap().len() > clean_size);

        // 重开：垃圾尾部被截断，3 条合法记录不丢不重
        let ledger = FileLedger::open(dir.path()).unwrap();
        let entries = ledger.read_range(1, 4).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].user_id, 11);
        assert_eq!(entries[1].user_id, 22);
        assert_eq!(entries[2].user_id, 33);
        assert_eq!(ledger.last_seq().unwrap(), 3);

        // 日志长度恢复为干净尺寸（垃圾被清除）
        assert_eq!(std::fs::metadata(&log_path).unwrap().len(), clean_size);
        drop(ledger);

        // 二次重开稳定：不再截断、条数不变
        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.read_range(1, 4).unwrap().len(), 3);
        drop(ledger);

        // 修复后的 ledger 仍可追加，seq 连续
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.append(&sample_entry(4, 44)).unwrap(), 4);
        let all = ledger.read_range(1, 5).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[3].user_id, 44);
    }

    #[test]
    fn torn_index_tail_repaired_on_reopen() {
        // 索引 8B 槽位只写了一半 → reopen 截掉残槽，记录不丢不重
        let dir = TempDir::new().unwrap();
        let idx_path = dir.path().join("ledger.idx");
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for i in 1..=3 {
                ledger.append(&entry_i(i)).unwrap();
            }
        }
        assert_eq!(idx_size(dir.path()), 24);
        append_bytes(&idx_path, &[0x11, 0x22, 0x33, 0x44, 0x55]); // 5B 撕裂槽

        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 3);
        assert_eq!(ledger.read_range(1, 4).unwrap().len(), 3);
        assert_eq!(idx_size(dir.path()), 24); // 修复回 3 × 8B
    }

    #[test]
    fn stale_index_repaired_from_log_tail() {
        // 索引滞后于日志（比日志少一条）→ tail-scan 补齐
        let dir = TempDir::new().unwrap();
        let idx_path = dir.path().join("ledger.idx");
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for i in 1..=3 {
                ledger.append(&entry_i(i)).unwrap();
            }
        }
        truncate_file(&idx_path, 16); // 索引只认识前 2 条

        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 3);
        assert_eq!(ledger.read_range(3, 4).unwrap()[0].user_id, 10_003);
        assert_eq!(idx_size(dir.path()), 24);
    }

    #[test]
    fn missing_index_rebuilt_from_log() {
        let dir = TempDir::new().unwrap();
        let idx_path = dir.path().join("ledger.idx");
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for i in 1..=3 {
                ledger.append(&entry_i(i)).unwrap();
            }
        }
        fs::remove_file(&idx_path).unwrap();

        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 3);
        assert_eq!(ledger.read_range(1, 4).unwrap().len(), 3);
        assert_eq!(idx_size(dir.path()), 24);
    }

    #[test]
    fn corrupt_index_content_rebuilt_from_log() {
        // 索引内容非法（首偏移非 0）→ 丢弃重建
        let dir = TempDir::new().unwrap();
        let idx_path = dir.path().join("ledger.idx");
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for i in 1..=3 {
                ledger.append(&entry_i(i)).unwrap();
            }
        }
        let mut data = fs::read(&idx_path).unwrap();
        data[..8].copy_from_slice(&8u64.to_le_bytes()); // offsets[0] = 8 ≠ 0
        fs::write(&idx_path, data).unwrap();

        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 3);
        assert_eq!(ledger.read_range(1, 4).unwrap().len(), 3);
        assert_eq!(read_offsets(dir.path())[0], 0);
    }

    #[test]
    fn combined_damage_no_loss_no_duplication() {
        // 同时: 日志第 5 条半写 + 索引滞后到第 3 条 → 恢复到第 4 条，
        // 完整记录全保留（不丢），seq 连续（不重）
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("ledger.dat");
        let idx_path = dir.path().join("ledger.idx");
        {
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for i in 1..=5 {
                ledger.append(&entry_i(i)).unwrap();
            }
        }
        let offsets = read_offsets(dir.path());
        truncate_file(&log_path, offsets[4] + 20); // 第 5 条半写
        truncate_file(&idx_path, 24); // 索引只到第 3 条

        let ledger = FileLedger::open(dir.path()).unwrap();
        assert_eq!(ledger.last_seq().unwrap(), 4);
        let all = ledger.read_range(1, 5).unwrap();
        assert_eq!(all.len(), 4);
        for (i, e) in all.iter().enumerate() {
            assert_eq!(e.user_id, 10_001 + i as u64); // user_id 单调 → 无重复
        }
        assert_eq!(fs::metadata(&log_path).unwrap().len(), offsets[4]);
        assert_eq!(idx_size(dir.path()), 32);
    }
}
