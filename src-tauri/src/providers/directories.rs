//! nvm 与 PATH 的只读目录发现，不加载 shell 配置或运行未知来源程序。

use std::fs;
use std::io;
use std::path::Path;

use crate::domain::{
    ObservationEvidence, Ownership, PackageKind, ProviderId, ProviderPackage, ProviderStatus,
};

use super::{Candidate, ScanRun};

pub(super) fn collect_nvm(run: &mut ScanRun<'_>) {
    let root = run.options.nvm_dir.clone();
    if !run.check() {
        return;
    }
    if !root.is_absolute() {
        run.problem(
            ProviderStatus::Error,
            "invalidProviderRoot",
            "nvm 根目录必须是绝对路径",
        );
        return;
    }

    // 根目录存在但未安装任何版本时仍是完整的空扫描。
    match fs::metadata(&root) {
        Ok(metadata) if metadata.is_dir() => run.mark_available(),
        Ok(_) => {
            run.problem(
                ProviderStatus::Error,
                "invalidProviderRoot",
                "nvm 根路径不是目录",
            );
            return;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            metadata_problem(run, "nvmRootMetadata", &error);
            return;
        }
    }

    let Some(versions) = run.read_dir(&root.join("versions/node")) else {
        return;
    };
    for version_directory in versions {
        if !run.check() {
            break;
        }
        match fs::symlink_metadata(&version_directory) {
            Ok(metadata) if metadata.is_dir() || metadata.file_type().is_symlink() => {}
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                metadata_problem(run, "nvmVersionMetadata", &error);
                continue;
            }
        }

        let declared_version = version_directory
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.strip_prefix('v').unwrap_or(name))
            .filter(|version| !version.is_empty())
            .map(str::to_owned);
        let executable = version_directory.join("bin/node");
        run.observe(Candidate {
            provider_package: ProviderPackage {
                provider_id: ProviderId::Nvm,
                package_id: "node".to_owned(),
                kind: PackageKind::Runtime,
                platform: run.options.platform,
            },
            locator: version_directory.clone(),
            executable,
            declared_version,
            ownership: Ownership::Observed,
            evidence: vec![ObservationEvidence {
                kind: "nvmVersionDirectory".to_owned(),
                path: path_string(&version_directory),
                detail: None,
            }],
            trusted: true,
        });
    }
}

pub(super) fn collect_path(run: &mut ScanRun<'_>) {
    // 遍历原始顺序中的所有目录，不能只观察第一个命中的程序。
    let entries = run.options.path_entries.clone();
    run.mark_available();
    for (index, directory) in entries.into_iter().enumerate() {
        if !run.visit_entry() {
            break;
        }
        if !directory.is_absolute() {
            run.problem(
                ProviderStatus::Error,
                "relativePathEntry",
                "PATH 包含空目录或相对目录，已跳过以避免使用工作目录",
            );
            continue;
        }

        for name in ["node", "nodejs"] {
            if !run.check() {
                return;
            }
            let executable = directory.join(name);
            match fs::symlink_metadata(&executable) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    metadata_problem(run, "pathExecutableMetadata", &error);
                    continue;
                }
            }

            run.observe(Candidate {
                provider_package: ProviderPackage {
                    provider_id: ProviderId::Path,
                    package_id: name.to_owned(),
                    kind: PackageKind::Executable,
                    platform: run.options.platform,
                },
                locator: executable.clone(),
                executable,
                declared_version: None,
                ownership: Ownership::Observed,
                evidence: vec![ObservationEvidence {
                    kind: "pathEntryIndex".to_owned(),
                    path: path_string(&directory),
                    detail: Some(index.to_string()),
                }],
                trusted: false,
            });
        }
    }
}

fn metadata_problem(run: &mut ScanRun<'_>, code: &str, error: &io::Error) {
    let status = if error.kind() == io::ErrorKind::PermissionDenied {
        ProviderStatus::PermissionDenied
    } else {
        ProviderStatus::Error
    };
    run.problem(status, code, &format!("读取来源路径元数据失败：{error}"));
}

fn path_string(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::Health;
    use crate::providers::{scan_node_provider, CancellationToken, NodeScanOptions, ScanLimits};
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    fn options(home: &Path) -> NodeScanOptions {
        NodeScanOptions {
            platform: crate::domain::Platform::Macos,
            home: home.to_path_buf(),
            mise_data_dir: home.join("mise"),
            nvm_dir: home.join("nvm"),
            brew_cellars: Vec::new(),
            path_entries: Vec::new(),
            limits: ScanLimits::default(),
        }
    }

    fn path_script(path: &Path, marker: &Path) {
        // 一旦错误执行 PATH 程序，该文件就会暴露只读探测的越界。
        let marker = marker.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            path,
            format!(
                "#!/bin/sh\n: > '{marker}'\nprintf '%s' '{{\"version\":\"v22.0.0\",\"arch\":\"arm64\"}}'\n"
            ),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn nvm_distinguishes_missing_manager_from_empty_installations() {
        let home = tempfile::tempdir().unwrap();
        let options = options(home.path());
        let missing = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(missing.report.status, ProviderStatus::Unavailable);
        assert!(!missing.report.coverage.complete);

        fs::create_dir_all(&options.nvm_dir).unwrap();
        let empty = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(empty.report.status, ProviderStatus::Ok);
        assert!(empty.report.coverage.complete);
        assert!(empty.observations.is_empty());
    }

    #[test]
    fn nvm_rejects_file_roots_and_relative_roots() {
        let home = tempfile::tempdir().unwrap();
        let mut options = options(home.path());
        fs::write(&options.nvm_dir, "not a directory").unwrap();
        let file_root =
            scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(file_root.report.status, ProviderStatus::Error);
        assert!(!file_root.report.coverage.complete);

        options.nvm_dir = PathBuf::from("relative/nvm");
        let relative_root =
            scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(relative_root.report.status, ProviderStatus::Error);
        assert!(relative_root.observations.is_empty());
    }

    #[test]
    fn nvm_preserves_broken_version_links_without_assuming_ownership_or_version() {
        let home = tempfile::tempdir().unwrap();
        let options = options(home.path());
        let versions = options.nvm_dir.join("versions/node");
        fs::create_dir_all(&versions).unwrap();
        symlink(home.path().join("missing-node"), versions.join("v22.0.0")).unwrap();
        let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());

        assert_eq!(scan.report.status, ProviderStatus::Ok);
        assert_eq!(scan.observations.len(), 1);
        let observation = &scan.observations[0];
        assert_eq!(observation.health, Health::Broken);
        assert_eq!(observation.ownership, Ownership::Observed);
        assert!(observation.version.is_none());
        assert!(observation.canonical_executable_path.is_none());
        assert_eq!(
            observation.executable_path.as_deref(),
            versions.join("v22.0.0/bin/node").to_str()
        );
    }

    #[test]
    fn nvm_does_not_load_nvm_shell_configuration() {
        let home = tempfile::tempdir().unwrap();
        let options = options(home.path());
        fs::create_dir_all(&options.nvm_dir).unwrap();
        let marker = home.path().join("probe-was-executed");
        path_script(&options.nvm_dir.join("nvm.sh"), &marker);
        let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());

        assert_eq!(scan.report.status, ProviderStatus::Ok);
        assert!(!marker.exists());
        assert!(scan.observations.is_empty());
    }

    #[test]
    fn path_keeps_all_directories_and_alias_names_without_running_programs() {
        let home = tempfile::tempdir().unwrap();
        let mut options = options(home.path());
        let first = home.path().join("first");
        let second = home.path().join("second");
        let marker = home.path().join("probe-was-executed");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        path_script(&first.join("node"), &marker);
        path_script(&second.join("node"), &marker);
        path_script(&second.join("nodejs"), &marker);
        options.path_entries = vec![first.clone(), second.clone()];
        let scan = scan_node_provider(ProviderId::Path, &options, &CancellationToken::default());

        assert_eq!(scan.report.status, ProviderStatus::Ok);
        assert!(scan.report.coverage.complete);
        assert_eq!(scan.observations.len(), 3);
        assert!(!marker.exists());
        for observation in &scan.observations {
            assert!(observation.version.is_none());
            assert!(observation.architecture.is_none());
            assert_eq!(observation.ownership, Ownership::Observed);
            let entry = observation
                .evidence
                .iter()
                .find(|evidence| evidence.kind == "pathEntryIndex")
                .unwrap();
            let expected = if observation.locator.starts_with(first.to_str().unwrap()) {
                (first.to_str(), "0")
            } else {
                (second.to_str(), "1")
            };
            assert_eq!(entry.path.as_deref(), expected.0);
            assert_eq!(entry.detail.as_deref(), Some(expected.1));
        }
    }

    #[test]
    fn path_invalid_entries_mark_incomplete_but_do_not_hide_later_candidates() {
        let home = tempfile::tempdir().unwrap();
        let mut options = options(home.path());
        let absolute = home.path().join("absolute");
        let marker = home.path().join("probe-was-executed");
        fs::create_dir_all(&absolute).unwrap();
        path_script(&absolute.join("node"), &marker);
        options.path_entries = vec![PathBuf::new(), PathBuf::from("relative/bin"), absolute];
        let scan = scan_node_provider(ProviderId::Path, &options, &CancellationToken::default());

        assert_eq!(scan.report.status, ProviderStatus::Error);
        assert!(!scan.report.coverage.complete);
        assert_eq!(scan.observations.len(), 1);
        assert!(scan.observations[0].evidence.iter().any(|evidence| {
            evidence.kind == "pathEntryIndex" && evidence.detail.as_deref() == Some("2")
        }));
        assert!(!marker.exists());
    }

    #[test]
    fn path_preserves_broken_links_and_ignores_missing_directories() {
        let home = tempfile::tempdir().unwrap();
        let mut options = options(home.path());
        let directory = home.path().join("bin");
        fs::create_dir_all(&directory).unwrap();
        symlink(home.path().join("missing-node"), directory.join("node")).unwrap();
        options.path_entries = vec![home.path().join("not-installed"), directory];
        let scan = scan_node_provider(ProviderId::Path, &options, &CancellationToken::default());

        assert_eq!(scan.report.status, ProviderStatus::Ok);
        assert!(scan.report.coverage.complete);
        assert_eq!(scan.observations.len(), 1);
        assert_eq!(scan.observations[0].health, Health::Broken);
        assert!(scan.observations[0].canonical_executable_path.is_none());
    }
}
