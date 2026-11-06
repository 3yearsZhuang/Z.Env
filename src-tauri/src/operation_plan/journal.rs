//! 白名单 journal；同目录原子写入，失败必须阻止下一步。
use super::{model::*, PlanError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
pub(super) const MAX_BYTES: u64 = 256 * 1024;
const MAX_RECORDS: usize = 64;

pub(super) fn validate_v1_record(record: &ExecutionRecord) -> Result<(), PlanError> {
    if record.schema_version != 1
        || !super::valid_id(&record.id)
        || record.steps.len() > 8
        || record.created_at > 9_007_199_254_740_991
        || record.updated_at > 9_007_199_254_740_991
    {
        return Err(PlanError::Journal);
    }
    Ok(())
}

fn validate_records(records: &[ExecutionRecord]) -> Result<(), PlanError> {
    if records.len() > MAX_RECORDS {
        return Err(PlanError::Journal);
    }
    for record in records {
        validate_v1_record(record)?;
    }
    let ids: std::collections::BTreeSet<_> = records.iter().map(|record| &record.id).collect();
    if ids.len() != records.len() {
        return Err(PlanError::Journal);
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u32,
    records: Vec<ExecutionRecord>,
}
pub(super) struct Journal {
    pub path: PathBuf,
    pub records: Vec<ExecutionRecord>,
    lock: Option<File>,
    expected: Option<Vec<u8>>,
}
impl Journal {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            records: Vec::new(),
            lock: None,
            expected: None,
        }
    }
    #[cfg(not(unix))]
    fn acquire(&mut self) -> Result<(), PlanError> {
        Err(PlanError::Unsupported)
    }
    #[cfg(unix)]
    fn acquire(&mut self) -> Result<(), PlanError> {
        if self.lock.is_some() {
            return Ok(());
        }
        let parent = self
            .path
            .parent()
            .filter(|p| p.is_absolute())
            .ok_or(PlanError::Journal)?;
        std::fs::create_dir_all(parent).map_err(|_| PlanError::Journal)?;
        if std::fs::symlink_metadata(parent)
            .map_err(|_| PlanError::Journal)?
            .file_type()
            .is_symlink()
        {
            return Err(PlanError::Journal);
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let lock = options
            .open(parent.join("operations-v1.lock"))
            .map_err(|_| PlanError::Journal)?;
        if !lock.metadata().map_err(|_| PlanError::Journal)?.is_file() {
            return Err(PlanError::Journal);
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(PlanError::JournalBusy);
            }
        }
        #[cfg(not(unix))]
        {
            return Err(PlanError::Unsupported);
        }
        #[cfg(unix)]
        {
            self.lock = Some(lock);
            Ok(())
        }
    }
    fn read(&mut self) -> Result<(), PlanError> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = match options.open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.records.clear();
                self.expected = None;
                return Ok(());
            }
            Err(_) => return Err(PlanError::Journal),
        };
        if !file.metadata().map_err(|_| PlanError::Journal)?.is_file() {
            return Err(PlanError::Journal);
        }
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|_| PlanError::Journal)?;
        if data.len() as u64 > MAX_BYTES {
            return Err(PlanError::Journal);
        }
        let doc: Document = serde_json::from_slice(&data).map_err(|_| PlanError::Journal)?;
        if doc.schema_version != 1 {
            return Err(PlanError::Journal);
        }
        validate_records(&doc.records)?;
        self.records = doc.records;
        self.expected = Some(data);
        Ok(())
    }
    pub fn recover_existing(&mut self) -> Result<(), PlanError> {
        if !self.path.try_exists().map_err(|_| PlanError::Journal)? {
            if std::fs::symlink_metadata(&self.path).is_ok() {
                return Err(PlanError::Journal);
            }
            return Ok(());
        }
        self.acquire()?;
        self.read()?;
        self.recover()
    }
    fn recover(&mut self) -> Result<(), PlanError> {
        validate_records(&self.records)?;
        let mut changed = false;
        for r in &mut self.records {
            if r.status == ExecutionStatus::Running {
                r.status = ExecutionStatus::Interrupted;
                r.outcome = Some(Outcome::Interrupted);
                r.updated_at = super::now().max(r.updated_at);
                for s in &mut r.steps {
                    if s.status == StepStatus::Running {
                        s.status = StepStatus::Failed;
                    } else if s.status == StepStatus::Planned {
                        s.status = StepStatus::Skipped;
                    }
                }
                changed = true;
            }
        }
        if changed {
            self.persist()?;
        }
        Ok(())
    }
    pub fn prepare(&mut self) -> Result<(), PlanError> {
        if self.lock.is_none() {
            self.acquire()?;
            self.read()?;
            self.recover()?;
        }
        Ok(())
    }
    fn unchanged(&self) -> Result<(), PlanError> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let current = match options.open(&self.path) {
            Ok(file) => {
                if !file.metadata().map_err(|_| PlanError::Journal)?.is_file() {
                    return Err(PlanError::Journal);
                }
                let mut bytes = Vec::new();
                file.take(MAX_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| PlanError::Journal)?;
                if bytes.len() as u64 > MAX_BYTES {
                    return Err(PlanError::Journal);
                }
                Some(bytes)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(PlanError::Journal),
        };
        if current != self.expected {
            return Err(PlanError::Journal);
        }
        Ok(())
    }
    pub fn persist(&mut self) -> Result<(), PlanError> {
        validate_records(&self.records)?;
        if self.lock.is_none() {
            return Err(PlanError::Journal);
        }
        let parent = self.path.parent().ok_or(PlanError::Journal)?;
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(PlanError::Journal);
        }
        let data = serde_json::to_vec(&Document {
            schema_version: 1,
            records: self.records.clone(),
        })
        .map_err(|_| PlanError::Journal)?;
        if data.len() as u64 > MAX_BYTES {
            return Err(PlanError::Journal);
        }
        self.unchanged()?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| PlanError::Journal)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|_| PlanError::Journal)?;
        }
        temp.write_all(&data)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|_| PlanError::Journal)?;
        // 非协作的外部编辑也不能被本进程的旧内存记录覆盖。
        self.unchanged()?;
        temp.persist(&self.path).map_err(|_| PlanError::Journal)?;
        self.expected = Some(data);
        #[cfg(unix)]
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| PlanError::Journal)?;
        Ok(())
    }
    pub fn push(&mut self, record: ExecutionRecord) -> Result<(), PlanError> {
        validate_v1_record(&record)?;
        self.prepare()?;
        validate_records(&self.records)?;
        if self.records.iter().any(|existing| existing.id == record.id) {
            return Err(PlanError::Journal);
        }
        self.records.insert(0, record);
        self.records.truncate(MAX_RECORDS);
        self.persist()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::inventory::query::InventoryTool;
    use serde_json::json;

    fn record(id: &str) -> ExecutionRecord {
        ExecutionRecord {
            schema_version: 1,
            tool_id: InventoryTool::Node,
            id: id.into(),
            action: Action::Verify,
            status: ExecutionStatus::Running,
            created_at: 1,
            updated_at: 1,
            steps: vec![ExecutionStep {
                kind: StepKind::Preflight,
                status: StepStatus::Planned,
            }],
            outcome: None,
            compensation: None,
        }
    }

    #[test]
    fn invalid_v1_push_is_rejected_before_io_or_memory_mutation() {
        for mode in 0..5 {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("must-not-create/operations-v1.json");
            let mut journal = Journal::new(path.clone());
            let mut invalid = record("record");
            match mode {
                0 => invalid.schema_version = 2,
                1 => invalid.id = "invalid/path".into(),
                2 => invalid.steps.resize(9, invalid.steps[0].clone()),
                3 => invalid.created_at = 9_007_199_254_740_992,
                _ => invalid.updated_at = 9_007_199_254_740_992,
            }
            assert!(matches!(journal.push(invalid), Err(PlanError::Journal)));
            assert!(journal.records.is_empty());
            assert!(!path.parent().unwrap().exists());
        }
    }

    #[test]
    fn persist_recover_and_duplicate_push_preserve_existing_file_on_invalid_memory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("operations-v1.json");
        let mut journal = Journal::new(path.clone());
        journal.push(record("record")).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(matches!(
            journal.push(record("record")),
            Err(PlanError::Journal)
        ));
        assert_eq!(journal.records.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        journal.records[0].steps.resize(
            9,
            ExecutionStep {
                kind: StepKind::Verify,
                status: StepStatus::Planned,
            },
        );
        assert!(matches!(journal.persist(), Err(PlanError::Journal)));
        assert!(matches!(journal.recover(), Err(PlanError::Journal)));
        assert_eq!(journal.records[0].status, ExecutionStatus::Running);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        journal.records[0].steps.truncate(1);
        journal.records = (0..65)
            .map(|index| record(&format!("record-{index}")))
            .collect();
        assert!(matches!(journal.persist(), Err(PlanError::Journal)));
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn live_journal_keeps_v1_budget_and_rejects_recipe_or_future_formats_without_overwrite() {
        let recipe: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/operations/recipe-record-v2.json"
        ))
        .unwrap();
        let mut invalids = vec![
            json!({"schemaVersion":2,"records":[]}),
            json!({"schemaVersion":1,"records":[recipe["records"]["partial"]]}),
            json!({"schemaVersion":1,"records":[record("same"),record("same")]}),
            json!({"schemaVersion":1,"records":(0..65).map(|index| record(&format!("record-{index}"))).collect::<Vec<_>>() }),
        ];
        let mut too_many_steps = record("record");
        too_many_steps
            .steps
            .resize(9, too_many_steps.steps[0].clone());
        invalids.push(json!({"schemaVersion":1,"records":[too_many_steps]}));
        let mut future_record = record("record");
        future_record.schema_version = 2;
        invalids.push(json!({"schemaVersion":1,"records":[future_record]}));
        for value in invalids {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("operations-v1.json");
            let data = serde_json::to_vec(&value).unwrap();
            std::fs::write(&path, &data).unwrap();
            let mut journal = Journal::new(path.clone());
            assert!(matches!(
                journal.recover_existing(),
                Err(PlanError::Journal)
            ));
            assert_eq!(std::fs::read(path).unwrap(), data);
        }
    }

    #[test]
    fn record_retention_stays_at_64_and_oversize_document_is_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("operations-v1.json");
        let mut journal = Journal::new(path.clone());
        for index in 0..65 {
            journal.push(record(&format!("record-{index}"))).unwrap();
        }
        assert_eq!(journal.records.len(), MAX_RECORDS);
        assert!(!journal.records.iter().any(|record| record.id == "record-0"));
        drop(journal);
        let oversized = vec![b' '; MAX_BYTES as usize + 1];
        std::fs::write(&path, &oversized).unwrap();
        let mut journal = Journal::new(path.clone());
        assert!(matches!(
            journal.recover_existing(),
            Err(PlanError::Journal)
        ));
        assert_eq!(std::fs::read(path).unwrap(), oversized);
    }

    #[test]
    fn recovery_keeps_valid_timestamps_monotonic_when_the_clock_is_earlier() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("operations-v1.json");
        let mut journal = Journal::new(path);
        let mut future = record("future");
        future.created_at = 9_007_199_254_740_991;
        future.updated_at = future.created_at;
        journal.push(future).unwrap();
        journal.recover().unwrap();
        assert_eq!(journal.records[0].status, ExecutionStatus::Interrupted);
        assert_eq!(journal.records[0].updated_at, 9_007_199_254_740_991);
        validate_v1_record(&journal.records[0]).unwrap();
    }

    #[test]
    fn completed_legacy_record_with_clock_rollback_recovers_without_rewriting_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("operations-v1.json");
        let data = br#"{"schemaVersion":1,"records":[{"schemaVersion":1,"id":"legacy-clock-rollback","action":"verify","status":"completed","createdAt":200,"updatedAt":100,"steps":[],"outcome":"verified"}]}"#;
        std::fs::write(&path, data).unwrap();
        let mut journal = Journal::new(path.clone());
        journal.recover_existing().unwrap();
        assert_eq!(journal.records.len(), 1);
        assert_eq!(journal.records[0].tool_id, InventoryTool::Node);
        assert_eq!(journal.records[0].status, ExecutionStatus::Completed);
        assert_eq!(std::fs::read(path).unwrap(), data);
    }
}
