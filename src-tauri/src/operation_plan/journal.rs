//! 白名单 journal；同目录原子写入，失败必须阻止下一步。
use super::{model::*, PlanError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
const MAX_BYTES: u64 = 256 * 1024;
const MAX_RECORDS: usize = 64;
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
        if doc.schema_version != 1
            || doc.records.len() > MAX_RECORDS
            || doc
                .records
                .iter()
                .any(|r| r.schema_version != 1 || !super::valid_id(&r.id) || r.steps.len() > 8)
        {
            return Err(PlanError::Journal);
        }
        let ids: std::collections::BTreeSet<_> = doc.records.iter().map(|r| &r.id).collect();
        if ids.len() != doc.records.len() {
            return Err(PlanError::Journal);
        }
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
        let mut changed = false;
        for r in &mut self.records {
            if r.status == ExecutionStatus::Running {
                r.status = ExecutionStatus::Interrupted;
                r.outcome = Some(Outcome::Interrupted);
                r.updated_at = super::now();
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
        self.prepare()?;
        self.records.insert(0, record);
        self.records.truncate(MAX_RECORDS);
        self.persist()
    }
}
