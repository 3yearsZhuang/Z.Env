//! 有界文件读取与目标证据；完整内容只参与哈希，不进入计划或 journal。
use super::{model::ConfigFingerprint, PlanError};
use crate::providers::NodeScanOptions;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FileStamp {
    canonical: PathBuf,
    length: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, u32),
    #[cfg(windows)]
    identity: (u64, [u8; 16], u32),
}
fn opened(path: &Path) -> Result<File, PlanError> {
    #[cfg(windows)]
    disk_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags_no_follow();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        // 允许上层先解析合法 alias；打开时最终分量再次变成 reparse 则拒绝读取。
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    if !file
        .metadata()
        .map_err(|_| PlanError::EvidenceUnavailable)?
        .is_file()
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    #[cfg(windows)]
    windows_identity(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn disk_path(path: &Path) -> Result<(), PlanError> {
    use std::path::{Component, Prefix};
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(_) | Prefix::VerbatimDisk(_) => {}
                Prefix::UNC(_, share) | Prefix::VerbatimUNC(_, share)
                    if !share.to_string_lossy().eq_ignore_ascii_case("pipe") => {}
                // 禁止设备、GlobalRoot、命名管道等在 canonicalize/open 阶段阻塞或执行设备 I/O。
                _ => return Err(PlanError::EvidenceUnavailable),
            },
            Component::Normal(name) => {
                let text = name.to_string_lossy();
                let stem = text
                    .trim_end_matches([' ', '.'])
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .trim_end()
                    .to_ascii_uppercase();
                let serial = stem
                    .strip_prefix("COM")
                    .or_else(|| stem.strip_prefix("LPT"))
                    .is_some_and(|suffix| {
                        matches!(
                            suffix,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    });
                if text.contains(':')
                    || serial
                    || matches!(
                        stem.as_str(),
                        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
                    )
                {
                    return Err(PlanError::EvidenceUnavailable);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(windows)]
fn windows_identity(file: &File) -> Result<(u64, [u8; 16], u32), PlanError> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ID_INFO, FILE_TYPE_DISK,
    };
    let handle = file.as_raw_handle();
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let mut identity = FILE_ID_INFO::default();
    // 检查已打开句柄，而非可在检查后被替换的路径。文件系统未提供稳定 ID 时不授予写权限。
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK
        || unsafe { GetFileInformationByHandle(handle, &mut info) } == 0
        || info.dwFileAttributes
            & (FILE_ATTRIBUTE_DEVICE | FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
            != 0
        || unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut identity as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    Ok((
        identity.VolumeSerialNumber,
        identity.FileId.Identifier,
        info.dwFileAttributes,
    ))
}

fn missing_target(path: &Path) -> Result<PathBuf, PlanError> {
    let mut prefix = path.to_path_buf();
    let mut tail = Vec::new();
    loop {
        match prefix.canonicalize() {
            Ok(mut base) => {
                for part in tail.into_iter().rev() {
                    base.push(part);
                }
                return Ok(base);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::symlink_metadata(&prefix).is_ok() {
                    return Err(PlanError::EvidenceUnavailable);
                }
                tail.push(
                    prefix
                        .file_name()
                        .ok_or(PlanError::EvidenceUnavailable)?
                        .to_os_string(),
                );
                prefix = prefix
                    .parent()
                    .ok_or(PlanError::EvidenceUnavailable)?
                    .to_path_buf();
            }
            Err(_) => return Err(PlanError::EvidenceUnavailable),
        }
    }
}
/// 只解析父目录，保留最终链接本身，供绑定资源做路径重定向检查。
pub(super) fn destination(path: &Path) -> Result<PathBuf, PlanError> {
    #[cfg(windows)]
    disk_path(path)?;
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    let parent = missing_target(path.parent().ok_or(PlanError::EvidenceUnavailable)?)?;
    Ok(parent.join(path.file_name().ok_or(PlanError::EvidenceUnavailable)?))
}
#[cfg(unix)]
trait NoFollow {
    fn custom_flags_no_follow(&mut self);
}
#[cfg(unix)]
impl NoFollow for OpenOptions {
    fn custom_flags_no_follow(&mut self) {
        use std::os::unix::fs::OpenOptionsExt;
        self.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
}
fn stamp(canonical: PathBuf, file: &File) -> Result<FileStamp, PlanError> {
    let meta = file
        .metadata()
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    Ok(FileStamp {
        canonical,
        length: meta.len(),
        modified: meta.modified().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino(), meta.mode())
        },
        #[cfg(windows)]
        identity: windows_identity(file)?,
    })
}
pub(super) fn file_stamp(path: &Path) -> Result<FileStamp, PlanError> {
    #[cfg(windows)]
    disk_path(path)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    let file = opened(&canonical)?;
    stamp(canonical, &file)
}
pub(super) fn configuration(
    path: &Path,
) -> Result<(ConfigFingerprint, Option<FileStamp>), PlanError> {
    #[cfg(windows)]
    disk_path(path)?;
    if !path.is_absolute() {
        return Err(PlanError::EvidenceUnavailable);
    }
    let canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 断链不能与原本不存在的配置混为一谈。
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(PlanError::EvidenceUnavailable);
            }
            return Ok((
                ConfigFingerprint {
                    path: path.to_string_lossy().into(),
                    canonical_path: Some(missing_target(path)?.to_string_lossy().into()),
                    exists: false,
                    sha256: None,
                },
                None,
            ));
        }
        Err(_) => return Err(PlanError::EvidenceUnavailable),
    };
    let file = opened(&canonical)?;
    let before = stamp(canonical.clone(), &file)?;
    let mut bytes = Vec::new();
    (&file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    if bytes.len() > 1024 * 1024
        || before != stamp(canonical.clone(), &file)?
        || file_stamp(path)? != before
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    Ok((
        ConfigFingerprint {
            path: path.to_string_lossy().into(),
            canonical_path: Some(canonical.to_string_lossy().into()),
            exists: true,
            sha256: Some(digest),
        },
        Some(before),
    ))
}

pub(super) fn context(options: &NodeScanOptions, config: &Path) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        options.platform,
        options.home,
        options.mise_data_dir,
        options.nvm_dir,
        options.brew_cellars,
        options.path_entries,
        config,
        options.limits
    )
}
pub(super) fn executable(options: &NodeScanOptions, name: &str) -> Option<PathBuf> {
    let mut dirs = options.path_entries.clone();
    dirs.extend([
        options.home.join(".local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    dirs.into_iter()
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|path| {
            let Ok(stamp) = file_stamp(path) else {
                return false;
            };
            #[cfg(unix)]
            {
                stamp.identity.2 & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                let _ = stamp;
                true
            }
        })
}

#[cfg(all(test, windows))]
#[path = "guards_windows_tests.rs"]
mod windows_tests;
