//! 有界普通文件读取与前后证据比较；内容只用于解析，不持久化配置或环境值。
use super::model::FileEvidence;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};
const PER_FILE: usize = 128 * 1024;
const TOTAL: usize = 1024 * 1024;
#[derive(Clone, Eq, PartialEq)]
struct Stamp {
    canonical: PathBuf,
    length: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, u32),
}
fn stamp(path: &Path, file: &File) -> Result<Stamp, &'static str> {
    let m = file.metadata().map_err(|_| "fileMetadata")?;
    if !m.is_file() {
        return Err("notRegularFile");
    }
    Ok(Stamp {
        canonical: path.into(),
        length: m.len(),
        modified: m.modified().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (m.dev(), m.ino(), m.mode())
        },
    })
}
fn predicted(path: &Path) -> Result<PathBuf, &'static str> {
    let mut prefix = path.to_path_buf();
    let mut tail = vec![];
    loop {
        match prefix.canonicalize() {
            Ok(mut p) => {
                for part in tail.into_iter().rev() {
                    p.push(part);
                }
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::symlink_metadata(&prefix).is_ok() {
                    return Err("brokenLink");
                }
                tail.push(prefix.file_name().ok_or("invalidPath")?.to_os_string());
                prefix = prefix.parent().ok_or("invalidPath")?.into();
            }
            Err(_) => return Err("fileAccess"),
        }
    }
}
#[derive(Clone)]
struct ReadFile {
    evidence: FileEvidence,
    text: Option<String>,
    stamp: Option<Stamp>,
}
fn read(path: &Path) -> Result<ReadFile, &'static str> {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let target = predicted(path)?;
            return Ok(ReadFile {
                evidence: FileEvidence {
                    path: path.to_string_lossy().into(),
                    canonical_path: target.to_string_lossy().into(),
                    exists: false,
                    sha256: None,
                },
                text: None,
                stamp: None,
            });
        }
        Err(_) => return Err("fileAccess"),
    };
    let mut open = OpenOptions::new();
    open.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = open.open(&canonical).map_err(|_| "fileAccess")?;
    let before = stamp(&canonical, &file)?;
    let mut bytes = vec![];
    (&file)
        .take(PER_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "fileRead")?;
    if bytes.len() > PER_FILE {
        return Err("fileBudget");
    }
    if before != stamp(&canonical, &file)? || path.canonicalize().ok() != Some(canonical.clone()) {
        return Err("fileChanged");
    }
    let evidence = FileEvidence {
        path: path.to_string_lossy().into(),
        canonical_path: canonical.to_string_lossy().into(),
        exists: true,
        sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
    };
    Ok(ReadFile {
        evidence,
        text: Some(String::from_utf8(bytes).map_err(|_| "fileEncoding")?),
        stamp: Some(before),
    })
}
#[derive(Default)]
pub(super) struct Reader {
    files: Vec<ReadFile>,
    bytes: usize,
}
impl Reader {
    pub fn read(&mut self, path: &Path) -> Result<Option<String>, &'static str> {
        if let Some(file) = self
            .files
            .iter()
            .find(|f| Path::new(&f.evidence.path) == path)
        {
            return Ok(file.text.clone());
        }
        if self.files.len() >= 96 {
            return Err("fileCountBudget");
        }
        let file = read(path)?;
        self.bytes += file.text.as_ref().map_or(0, String::len);
        if self.bytes > TOTAL {
            return Err("metadataBudget");
        }
        let text = file.text.clone();
        self.files.push(file);
        Ok(text)
    }
    pub fn evidence(&self) -> Vec<FileEvidence> {
        self.files.iter().map(|f| f.evidence.clone()).collect()
    }
    pub fn unchanged(&self) -> bool {
        self.files.iter().all(|old| {
            read(Path::new(&old.evidence.path)).is_ok_and(|new| {
                new.evidence.sha256 == old.evidence.sha256
                    && new.evidence.canonical_path == old.evidence.canonical_path
                    && new.evidence.exists == old.evidence.exists
                    && new.stamp == old.stamp
            })
        })
    }
}

/// 目录成员变化可能引入此前不存在的 local/平台配置，不能只比较已读取文件。
#[derive(Eq, PartialEq)]
pub(super) struct DirectoryStamp {
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, u32, i64, i64),
}
pub(super) fn directory_stamp(path: &Path) -> Result<DirectoryStamp, &'static str> {
    let m = std::fs::metadata(path).map_err(|_| "directoryMetadata")?;
    if !m.is_dir() {
        return Err("notDirectory");
    }
    Ok(DirectoryStamp {
        modified: m.modified().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (m.dev(), m.ino(), m.mode(), m.ctime(), m.ctime_nsec())
        },
    })
}
