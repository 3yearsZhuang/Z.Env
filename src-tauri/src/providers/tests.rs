use super::*;

#[test]
fn probe_output_requires_actual_version_and_normalizes_runtime_architecture() {
    assert_eq!(
        parse_probe(br#"{"version":"22.23.3","architecture":"x64"}"#),
        Some(("22.23.3".into(), "x86_64".into()))
    );
    assert_eq!(
        parse_probe(br#"{"version":"v24.0.0-nightly20260101","architecture":"arm64"}"#),
        Some(("24.0.0-nightly20260101".into(), "arm64".into()))
    );
    for invalid in [
        b"not json".as_slice(),
        br#"{"version":"22","architecture":"arm64"}"#,
        br#"{"version":"22.0.0;touch file","architecture":"arm64"}"#,
        br#"{"version":"22.0.0","architecture":"unknown"}"#,
        br#"{"version":null,"architecture":"arm64"}"#,
        br#"{"version":"22.0.0"}"#,
    ] {
        assert_eq!(parse_probe(invalid), None);
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::inventory::{has_complete_coverage, merge_inventory};
    use std::collections::BTreeMap;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn options(root: &Path) -> NodeScanOptions {
        NodeScanOptions {
            platform: Platform::Macos,
            home: root.to_owned(),
            mise_data_dir: root.join("mise-data"),
            nvm_dir: root.join("nvm"),
            brew_cellars: vec![root.join("Cellar-arm"), root.join("Cellar-intel")],
            path_entries: vec![root.join("bin")],
            limits: ScanLimits::default(),
        }
    }

    fn script(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn node(path: &Path) {
        script(
            path,
            "printf '%s' '{\"version\":\"22.23.3\",\"architecture\":\"arm64\"}'",
        );
    }

    fn receipt(directory: &Path) {
        fs::write(
            directory.join("INSTALL_RECEIPT.json"),
            r#"{"source":{"tap":"homebrew/core"}}"#,
        )
        .unwrap();
    }

    #[test]
    fn no_mise_keeps_nvm_and_brew_instances_and_path_alias_without_writing_configs() {
        let temp = tempfile::tempdir().unwrap();
        let options = options(temp.path());
        let nvm = options.nvm_dir.join("versions/node/v20.0.0/bin/node");
        node(&nvm);
        let brew = options.brew_cellars[0].join("node@22/22.0.0");
        node(&brew.join("bin/node"));
        receipt(&brew);
        fs::create_dir_all(&options.path_entries[0]).unwrap();
        symlink(brew.join("bin/node"), options.path_entries[0].join("node")).unwrap();
        for name in [
            "mise.toml",
            ".zshrc",
            ".bashrc",
            ".config/mise/config.toml",
            ".zenv/managed.json",
        ] {
            let path = temp.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "do not change: secret + version binding\n").unwrap();
        }
        let before = files(temp.path());
        let scans = scan_node_sources(&options, &CancellationToken::default());
        assert_eq!(scans.len(), 4);
        assert_eq!(scans[0].report.status, ProviderStatus::Unavailable);
        let snapshot = merge_inventory("readonly".into(), now_ms(), scans).unwrap();
        assert_eq!(snapshot.installations.len(), 3);
        assert_eq!(snapshot.aliases.len(), 1);
        let path = snapshot
            .installations
            .iter()
            .find(|i| i.provider_package.provider_id == ProviderId::Path)
            .unwrap();
        assert_eq!(path.version, None);
        assert_eq!(path.health, Health::Unverified);
        let nvm = snapshot
            .installations
            .iter()
            .find(|i| i.provider_package.provider_id == ProviderId::Nvm)
            .unwrap();
        assert_eq!(nvm.version.as_deref(), Some("22.23.3"));
        assert!(nvm
            .evidence
            .iter()
            .any(|e| e.kind == "declaredVersion" && e.detail.as_deref() == Some("20.0.0")));
        assert_eq!(before, files(temp.path()));
    }

    #[test]
    fn controlled_probe_drops_injection_variables_and_uses_explicit_cwd_and_path() {
        let temp = tempfile::tempdir().unwrap();
        let options = options(temp.path());
        let path = options.nvm_dir.join("versions/node/v22/bin/node");
        let escaped_home = temp.path().to_string_lossy().replace('\'', "'\\''");
        let physical_home = temp
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .replace('\'', "'\\''");
        script(&path, &format!("[ \"$PWD\" = '{physical_home}' ] || exit 31\n[ \"$HOME\" = '{escaped_home}' ] || exit 32\n[ \"$PATH\" = '/usr/bin:/bin:/usr/sbin:/sbin' ] || exit 33\n[ -z \"${{NODE_OPTIONS+x}}${{NODE_PATH+x}}${{LD_PRELOAD+x}}${{DYLD_INSERT_LIBRARIES+x}}\" ] || exit 34\nprintf '%s' '{{\"version\":\"22.23.3\",\"architecture\":\"arm64\"}}'"));
        let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(scan.report.status, ProviderStatus::Ok);
        assert_eq!(scan.observations[0].health, Health::Healthy);
    }

    #[test]
    fn probe_failures_are_distinct_and_never_trust_the_directory_version() {
        for (body, expected, health) in [
            ("sleep 5", ProviderStatus::Timeout, Health::Unverified),
            ("exit 7", ProviderStatus::Error, Health::Broken),
            (
                "printf 'not a version'",
                ProviderStatus::Error,
                Health::Broken,
            ),
            (
                "while :; do printf '0123456789abcdef'; done",
                ProviderStatus::Error,
                Health::Broken,
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut options = options(temp.path());
            options.limits.probe_timeout = Duration::from_secs(2);
            options.limits.max_output_bytes = 256;
            script(
                &options.nvm_dir.join("versions/node/v99.99.99/bin/node"),
                body,
            );
            let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
            assert_eq!(scan.report.status, expected, "{body}");
            assert!(!scan.report.coverage.complete);
            assert_eq!(scan.observations.len(), 1);
            assert_eq!(scan.observations[0].version, None);
            assert_eq!(scan.observations[0].health, health);
        }
    }

    #[test]
    fn individual_timeout_does_not_hide_other_providers() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = options(temp.path());
        options.limits.probe_timeout = Duration::from_secs(2);
        script(
            &options.mise_data_dir.join("installs/node/22/bin/node"),
            "sleep 5",
        );
        node(&options.nvm_dir.join("versions/node/v22/bin/node"));
        let started = Instant::now();
        let scans = scan_node_sources(&options, &CancellationToken::default());
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(scans[0].report.status, ProviderStatus::Timeout);
        assert_eq!(scans[2].report.status, ProviderStatus::Ok);
        assert_eq!(scans[2].observations[0].version.as_deref(), Some("22.23.3"));
    }

    #[test]
    fn cancellation_returns_partial_evidence_without_claiming_broken_install() {
        let temp = tempfile::tempdir().unwrap();
        let options = options(temp.path());
        let marker = temp.path().join("started");
        script(
            &options.nvm_dir.join("versions/node/v22/bin/node"),
            &format!("printf start > '{}'; sleep 5", marker.display()),
        );
        let token = CancellationToken::default();
        let cancel = token.clone();
        let trigger = std::thread::spawn(move || {
            let started = Instant::now();
            while !marker.exists() && started.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(5));
            }
            cancel.cancel();
        });
        let scan = scan_node_provider(ProviderId::Nvm, &options, &token);
        trigger.join().unwrap();
        assert_eq!(scan.report.status, ProviderStatus::Cancelled);
        assert!(!scan.report.coverage.complete);
        assert_eq!(scan.observations[0].health, Health::Unverified);
        let cancelled = scan_node_sources(&options, &token);
        assert!(cancelled
            .iter()
            .all(|s| s.report.status == ProviderStatus::Cancelled && s.observations.is_empty()));
    }

    #[test]
    fn directory_candidate_probe_and_metadata_budgets_are_visible() {
        for budget in ["directory", "candidate", "probe", "metadata"] {
            let temp = tempfile::tempdir().unwrap();
            let mut options = options(temp.path());
            let directory = options.brew_cellars[0].join("node@22/22.0.0");
            node(&directory.join("bin/node"));
            receipt(&directory);
            match budget {
                "directory" => options.limits.max_directory_entries = 0,
                "candidate" => options.limits.max_candidates = 0,
                "probe" => options.limits.max_probes = 0,
                "metadata" => options.limits.max_metadata_bytes = 1,
                _ => unreachable!(),
            }
            let scan =
                scan_node_provider(ProviderId::Brew, &options, &CancellationToken::default());
            assert!(scan.report.coverage.truncated, "{budget}");
            assert!(!has_complete_coverage(&scan.report, &ToolId::from("node")));
            assert!(scan.observations.iter().all(|o| o.version.is_none()));
        }
    }

    #[test]
    fn oversized_receipt_exhausts_shared_metadata_budget_for_later_files() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = options(temp.path());
        options.limits.max_metadata_bytes = 4;
        let first = temp.path().join("first.json");
        let second = temp.path().join("second.json");
        fs::write(&first, "123456789").unwrap();
        fs::write(&second, "12").unwrap();
        let token = CancellationToken::default();
        let mut run = ScanRun::new(ProviderId::Brew, &options, &token);
        assert!(run.read_metadata(&first).is_none());
        assert_eq!(run.metadata_bytes, 4);
        assert!(run.read_metadata(&second).is_none());
        assert_eq!(run.metadata_bytes, 4);
        assert!(run.truncated);
    }

    #[test]
    fn unsupported_platform_and_zero_time_budget_do_not_execute_any_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = options(temp.path());
        node(&options.nvm_dir.join("versions/node/v22/bin/node"));
        for platform in [Platform::Linux, Platform::Windows] {
            options.platform = platform;
            let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
            assert_eq!(scan.report.status, ProviderStatus::Unsupported);
            assert!(scan.observations.is_empty());
        }
        options.platform = Platform::Macos;
        let mut relative_mise = options.clone();
        relative_mise.mise_data_dir = PathBuf::from("relative-mise");
        let scan = scan_node_provider(
            ProviderId::Mise,
            &relative_mise,
            &CancellationToken::default(),
        );
        assert_eq!(scan.report.status, ProviderStatus::Error);
        assert_eq!(scan.report.error.unwrap().code, "relativeProviderRoot");
        assert!(scan.observations.is_empty());
        options.limits.provider_timeout = Duration::ZERO;
        let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(scan.report.status, ProviderStatus::Timeout);
        assert!(scan.observations.is_empty());
    }

    #[test]
    fn path_directory_budget_marks_coverage_truncated() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = options(temp.path());
        options.path_entries = vec![temp.path().join("first"), temp.path().join("second")];
        for entry in &options.path_entries {
            node(&entry.join("node"));
        }
        options.limits.max_directory_entries = 1;
        let scan = scan_node_provider(ProviderId::Path, &options, &CancellationToken::default());
        assert_eq!(scan.observations.len(), 1);
        assert!(scan.report.coverage.truncated);
        assert!(!scan.report.coverage.complete);
    }

    #[test]
    fn non_executable_and_non_regular_candidates_remain_broken_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let options = options(temp.path());
        let path = options.nvm_dir.join("versions/node/v22/bin/node");
        node(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(scan.report.status, ProviderStatus::PermissionDenied);
        assert_eq!(scan.observations[0].health, Health::Broken);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let scan = scan_node_provider(ProviderId::Nvm, &options, &CancellationToken::default());
        assert_eq!(scan.observations[0].health, Health::Broken);
        assert_eq!(scan.observations[0].version, None);
    }

    #[test]
    fn metadata_symlink_and_fifo_are_not_opened_or_executed() {
        let temp = tempfile::tempdir().unwrap();
        let options = options(temp.path());
        let directory = options.brew_cellars[0].join("node/22");
        node(&directory.join("bin/node"));
        let receipt = directory.join("INSTALL_RECEIPT.json");
        symlink("/missing-receipt", &receipt).unwrap();
        let scan = scan_node_provider(ProviderId::Brew, &options, &CancellationToken::default());
        assert_eq!(scan.report.status, ProviderStatus::Error);
        assert_eq!(scan.observations[0].ownership, Ownership::Unknown);
        fs::remove_file(&receipt).unwrap();
        let name = std::ffi::CString::new(receipt.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let started = Instant::now();
        let scan = scan_node_provider(ProviderId::Brew, &options, &CancellationToken::default());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(scan.report.status, ProviderStatus::Error);
    }

    fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut pending = vec![root.to_owned()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    pending.push(entry.path());
                } else if file_type.is_file() {
                    files.insert(entry.path(), fs::read(entry.path()).unwrap());
                }
            }
        }
        files
    }
}
