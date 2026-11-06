//! 冻结已有 Tool 项目绑定的目标与声明证据；重验不重新选择配置或执行管理器。
use super::{
    guards::{self, FileStamp},
    model::ConfigFingerprint,
    PlanError,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

// 与 project_environment 的声明读取预算一致；配置内容仍由 guards 的普通文件读取保护。
const PER_FILE_BYTES: u64 = 128 * 1024;
const TOTAL_BYTES: u64 = 1024 * 1024;
const MAX_FILES: usize = 96;
const MAX_DIRECTORY_ENTRIES: usize = 256;
const MAX_ANCESTORS: usize = 16;
const DECLARATIONS: [&str; 7] = [
    "mise.toml",
    ".mise.toml",
    ".tool-versions",
    ".nvmrc",
    ".node-version",
    "package.json",
    ".python-version",
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryStamp {
    readonly: bool,
    #[cfg(unix)]
    identity: (u64, u64, u32),
    #[cfg(windows)]
    identity: (u64, [u8; 16], u32),
}

fn directory_stamp(path: &Path) -> Result<DirectoryStamp, PlanError> {
    let metadata = std::fs::metadata(path).map_err(|_| PlanError::EvidenceUnavailable)?;
    if !metadata.is_dir() {
        return Err(PlanError::EvidenceUnavailable);
    }
    Ok(DirectoryStamp {
        readonly: metadata.permissions().readonly(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino(), metadata.mode())
        },
        #[cfg(windows)]
        identity: windows_directory_identity(path)?,
    })
}

#[cfg(windows)]
fn windows_directory_identity(path: &Path) -> Result<(u64, [u8; 16], u32), PlanError> {
    use std::{
        fs::OpenOptions,
        mem::size_of,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_ID_INFO, FILE_TYPE_DISK,
    };
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    let handle = file.as_raw_handle();
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let mut identity = FILE_ID_INFO::default();
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK
        || unsafe { GetFileInformationByHandle(handle, &mut info) } == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || info.dwFileAttributes & (FILE_ATTRIBUTE_DEVICE | FILE_ATTRIBUTE_REPARSE_POINT) != 0
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    canonical: PathBuf,
    stamp: DirectoryStamp,
}

impl DirectoryIdentity {
    fn read(path: &Path) -> Result<Self, PlanError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| PlanError::EvidenceUnavailable)?;
        let stamp = directory_stamp(&canonical)?;
        if path.canonicalize().ok().as_ref() != Some(&canonical)
            || directory_stamp(&canonical)? != stamp
        {
            return Err(PlanError::EvidenceUnavailable);
        }
        Ok(Self { canonical, stamp })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FrozenFile {
    path: PathBuf,
    destination: PathBuf,
    fingerprint: ConfigFingerprint,
    stamp: Option<FileStamp>,
}

impl FrozenFile {
    fn read(path: &Path, bytes: &mut u64) -> Result<Self, PlanError> {
        // destination 同时保留最终链接名称并使用 guards 的平台路径限制。
        let destination = guards::destination(path)?;
        let length = match std::fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Ok(_) => return Err(PlanError::EvidenceUnavailable),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(_) => return Err(PlanError::EvidenceUnavailable),
        };
        if length > PER_FILE_BYTES || bytes.saturating_add(length) > TOTAL_BYTES {
            return Err(PlanError::EvidenceUnavailable);
        }
        let (fingerprint, stamp) = guards::configuration(path)?;
        let after_length = if fingerprint.exists {
            std::fs::metadata(path)
                .map_err(|_| PlanError::EvidenceUnavailable)?
                .len()
        } else {
            0
        };
        if length != after_length || guards::destination(path)? != destination {
            return Err(PlanError::EvidenceUnavailable);
        }
        *bytes += length;
        Ok(Self {
            path: path.into(),
            destination,
            fingerprint,
            stamp,
        })
    }
}

fn configuration_variant(name: &str) -> bool {
    !matches!(name, "mise.toml" | ".mise.toml")
        && (((name.starts_with("mise.") || name.starts_with(".mise.")) && name.ends_with(".toml"))
            || matches!(name, "mise.lock" | ".mise.lock"))
}

fn variants(directory: &Path) -> Result<Vec<OsString>, PlanError> {
    let mut names = Vec::new();
    for (index, entry) in std::fs::read_dir(directory)
        .map_err(|_| PlanError::EvidenceUnavailable)?
        .enumerate()
    {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err(PlanError::EvidenceUnavailable);
        }
        let name = entry
            .map_err(|_| PlanError::EvidenceUnavailable)?
            .file_name();
        if configuration_variant(&name.to_string_lossy()) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// 不公开可替换的字段；调用方必须保留实际准备出的配置目标，不能从新快照选另一个目标。
#[derive(Clone, Debug)]
pub(super) struct FrozenProjectTarget {
    raw: PathBuf,
    directory_input: bool,
    directory: DirectoryIdentity,
    selected: PathBuf,
    selected_destination: PathBuf,
    variants: Vec<OsString>,
    files: Vec<FrozenFile>,
}

impl FrozenProjectTarget {
    pub(super) fn capture(raw: &str, selected: &Path) -> Result<Self, PlanError> {
        let path = Path::new(raw);
        if !path.is_absolute() || raw.len() > 4096 || !selected.is_absolute() {
            return Err(PlanError::Unsupported);
        }
        // 在任何 canonicalize 前验证平台路径；这里只构造路径，不创建探测文件。
        guards::destination(&path.join("__zenv_project_guard__"))?;
        let prepared_target =
            crate::project::target(raw).map_err(|_| PlanError::EvidenceUnavailable)?;
        if prepared_target != selected {
            return Err(PlanError::Preconditions);
        }
        let directory_input = path.is_dir();
        let logical_directory = if directory_input {
            path
        } else {
            path.parent().ok_or(PlanError::EvidenceUnavailable)?
        };
        let directory = DirectoryIdentity::read(logical_directory)?;
        if selected.parent() != Some(directory.canonical.as_path()) {
            return Err(PlanError::Preconditions);
        }
        let selected_destination = guards::destination(selected)?;
        let variants = variants(&directory.canonical)?;
        let mut paths: Vec<PathBuf> = DECLARATIONS
            .iter()
            .copied()
            .chain(["pyproject.toml"])
            .map(|name| directory.canonical.join(name))
            .collect();
        paths.extend(variants.iter().map(|name| directory.canonical.join(name)));
        // 祖先配置也属于项目上下文；保留既有 16 层边界，不解析继承优先级。
        let mut ancestors = directory.canonical.ancestors().skip(1);
        for _ in 0..MAX_ANCESTORS {
            let Some(parent) = ancestors.next() else {
                break;
            };
            for name in ["mise.toml", ".mise.toml", ".tool-versions"] {
                paths.push(parent.join(name));
            }
        }
        if ancestors.next().is_some() || paths.len() > MAX_FILES {
            return Err(PlanError::EvidenceUnavailable);
        }
        let mut bytes = 0;
        let files = paths
            .iter()
            .map(|path| FrozenFile::read(path, &mut bytes))
            .collect::<Result<Vec<_>, _>>()?;
        let frozen = Self {
            raw: path.into(),
            directory_input,
            directory,
            selected: selected.into(),
            selected_destination,
            variants,
            files,
        };
        // 再比较一次，避免配置选择与声明读取跨越路径/目录成员变化。
        frozen.revalidate()?;
        Ok(frozen)
    }

    fn check_directory(&self) -> Result<(), PlanError> {
        if self.raw.is_dir() != self.directory_input {
            return Err(PlanError::Preconditions);
        }
        let logical_directory = if self.directory_input {
            self.raw.as_path()
        } else {
            self.raw.parent().ok_or(PlanError::Preconditions)?
        };
        if DirectoryIdentity::read(logical_directory).map_err(|_| PlanError::Preconditions)?
            != self.directory
            || guards::destination(&self.selected).map_err(|_| PlanError::Preconditions)?
                != self.selected_destination
            || variants(&self.directory.canonical).map_err(|_| PlanError::Preconditions)?
                != self.variants
        {
            return Err(PlanError::Preconditions);
        }
        if self.directory_input {
            // 只检查已冻结目标的选择条件，不解析或返回另一个配置目标。
            let visible = self
                .directory
                .canonical
                .join("mise.toml")
                .try_exists()
                .map_err(|_| PlanError::Preconditions)?;
            let hidden = self
                .directory
                .canonical
                .join(".mise.toml")
                .try_exists()
                .map_err(|_| PlanError::Preconditions)?;
            let valid = match self.selected.file_name().and_then(|name| name.to_str()) {
                Some(".mise.toml") => !visible && hidden,
                Some("mise.toml") => visible || !hidden,
                _ => false,
            };
            if !valid {
                return Err(PlanError::Preconditions);
            }
        }
        Ok(())
    }

    pub(super) fn revalidate(&self) -> Result<(), PlanError> {
        self.check_directory()?;
        let mut bytes = 0;
        for old in &self.files {
            let current =
                FrozenFile::read(&old.path, &mut bytes).map_err(|_| PlanError::Preconditions)?;
            if current != *old {
                return Err(PlanError::Preconditions);
            }
        }
        self.check_directory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn freeze(directory: &Path) -> FrozenProjectTarget {
        let raw = directory.to_str().unwrap();
        let selected = crate::project::target(raw).unwrap();
        FrozenProjectTarget::capture(raw, &selected).unwrap()
    }

    fn stale(frozen: &FrozenProjectTarget) {
        assert!(matches!(frozen.revalidate(), Err(PlanError::Preconditions)));
    }

    #[test]
    fn actual_prepared_target_must_match_current_selection() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(".mise.toml"), "[tools]\nnode='22'\n").unwrap();
        assert!(matches!(
            FrozenProjectTarget::capture(
                directory.path().to_str().unwrap(),
                &directory.path().join("mise.toml")
            ),
            Err(PlanError::Preconditions)
        ));
    }

    #[test]
    fn hidden_selection_cannot_switch_to_new_visible_config() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(".mise.toml"), "[tools]\nnode='22'\n").unwrap();
        let frozen = freeze(directory.path());
        std::fs::write(directory.path().join("mise.toml"), "[tools]\nnode='22'\n").unwrap();
        stale(&frozen);
        assert_eq!(frozen.selected.file_name().unwrap(), ".mise.toml");
    }

    #[test]
    fn explicit_hidden_config_keeps_its_selected_target() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("mise.toml"), "[tools]\nnode='20'\n").unwrap();
        let raw = directory.path().join(".mise.toml");
        std::fs::write(&raw, "[tools]\nnode='22'\n").unwrap();
        let raw = raw.to_str().unwrap();
        let selected = crate::project::target(raw).unwrap();
        let frozen = FrozenProjectTarget::capture(raw, &selected).unwrap();
        frozen.revalidate().unwrap();
        assert_eq!(frozen.selected.file_name().unwrap(), ".mise.toml");
    }

    #[test]
    fn legacy_input_only_freezes_evidence_and_creates_no_config() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = directory.path().join(".tool-versions");
        std::fs::write(&legacy, "node 22\npython 3.13.1\n").unwrap();
        let raw = legacy.to_str().unwrap();
        let selected = crate::project::target(raw).unwrap();
        let frozen = FrozenProjectTarget::capture(raw, &selected).unwrap();
        frozen.revalidate().unwrap();
        assert!(!selected.exists());
        assert_eq!(
            std::fs::read_to_string(legacy).unwrap(),
            "node 22\npython 3.13.1\n"
        );
    }

    #[test]
    fn missing_config_and_declaration_creation_require_new_preview() {
        let directory = tempfile::tempdir().unwrap();
        let frozen = freeze(directory.path());
        std::fs::write(directory.path().join("mise.toml"), "[tools]\nnode='22'\n").unwrap();
        stale(&frozen);
        let frozen = freeze(directory.path());
        std::fs::write(directory.path().join(".python-version"), "3.13.1\n").unwrap();
        stale(&frozen);
    }

    #[test]
    fn node_python_and_legacy_declaration_changes_require_new_preview() {
        for (name, before, after) in [
            (".nvmrc", "20\n", "22\n"),
            (".node-version", "20\n", "22\n"),
            (
                "package.json",
                "{\"engines\":{\"node\":\"20\"}}",
                "{\"engines\":{\"node\":\"22\"}}",
            ),
            (".python-version", "3.12.1\n", "3.13.1\n"),
            (
                "pyproject.toml",
                "[project]\nrequires-python='>=3.12'\n",
                "[project]\nrequires-python='>=3.13'\n",
            ),
            (".tool-versions", "node 20\n", "node 22\n"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(name);
            std::fs::write(&path, before).unwrap();
            let frozen = freeze(directory.path());
            std::fs::write(path, after).unwrap();
            stale(&frozen);
        }
    }

    #[test]
    fn configuration_variant_addition_and_content_change_are_frozen() {
        let directory = tempfile::tempdir().unwrap();
        let frozen = freeze(directory.path());
        let variant = directory.path().join("mise.local.toml");
        std::fs::write(&variant, "[tools]\nnode='22'\n").unwrap();
        stale(&frozen);
        let frozen = freeze(directory.path());
        std::fs::write(variant, "[tools]\nnode='24'\n").unwrap();
        stale(&frozen);
    }

    #[test]
    fn unrelated_file_creation_edit_and_removal_do_not_invalidate() {
        let directory = tempfile::tempdir().unwrap();
        let frozen = freeze(directory.path());
        let readme = directory.path().join("README.md");
        std::fs::write(&readme, "first\n").unwrap();
        frozen.revalidate().unwrap();
        std::fs::write(&readme, "second\n").unwrap();
        frozen.revalidate().unwrap();
        std::fs::remove_file(readme).unwrap();
        frozen.revalidate().unwrap();
    }

    #[test]
    fn declaration_read_budget_is_bounded() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("package.json"),
            vec![b' '; PER_FILE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(matches!(
            FrozenProjectTarget::capture(
                directory.path().to_str().unwrap(),
                &directory.path().canonicalize().unwrap().join("mise.toml")
            ),
            Err(PlanError::EvidenceUnavailable)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn project_alias_and_explicit_parent_alias_cannot_redirect() {
        use std::os::unix::fs::symlink;
        for explicit in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let first = root.path().join("first");
            let second = root.path().join("second");
            std::fs::create_dir(&first).unwrap();
            std::fs::create_dir(&second).unwrap();
            for directory in [&first, &second] {
                std::fs::write(directory.join("mise.toml"), "[tools]\nnode='22'\n").unwrap();
            }
            let alias = root.path().join("project");
            symlink(&first, &alias).unwrap();
            let raw = if explicit {
                alias.join("mise.toml")
            } else {
                alias.clone()
            };
            let raw = raw.to_str().unwrap();
            let selected = crate::project::target(raw).unwrap();
            let frozen = FrozenProjectTarget::capture(raw, &selected).unwrap();
            std::fs::remove_file(&alias).unwrap();
            symlink(&second, &alias).unwrap();
            stale(&frozen);
        }
    }

    #[cfg(unix)]
    #[test]
    fn configuration_link_cannot_redirect_to_identical_content() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.toml");
        let second = directory.path().join("second.toml");
        for path in [&first, &second] {
            std::fs::write(path, "[tools]\nnode='22'\n").unwrap();
        }
        let target = directory.path().join("mise.toml");
        symlink(&first, &target).unwrap();
        let frozen = freeze(directory.path());
        std::fs::remove_file(&target).unwrap();
        symlink(&second, &target).unwrap();
        stale(&frozen);
    }

    #[cfg(unix)]
    #[test]
    fn same_named_replacement_directory_has_a_new_identity() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let frozen = freeze(&project);
        std::fs::rename(&project, root.path().join("old-project")).unwrap();
        std::fs::create_dir(&project).unwrap();
        stale(&frozen);
    }
}
