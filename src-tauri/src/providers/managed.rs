//! 仅读取安装目录；扫描清单不调用可能更新缓存或触发安装钩子的管理器命令。

use std::fs;
use std::path::Path;

use crate::domain::{
    native_brew_node_formula, ObservationEvidence, Ownership, PackageKind, ProviderId,
    ProviderPackage, ProviderStatus,
};

use super::{Candidate, ScanRun};

pub(super) fn collect_mise(run: &mut ScanRun<'_>) {
    let data_dir = run.options.mise_data_dir.clone();
    if !directory_available(run, &data_dir) {
        return;
    }
    run.mark_available();

    // nodejs 是明确支持的旧插件别名。mise 可以链接外部安装，目录名本身不能证明归属。
    for package_id in ["node", "nodejs"] {
        if !run.check() {
            return;
        }
        let installs = data_dir.join("installs").join(package_id);
        let Some(versions) = run.read_dir(&installs) else {
            continue;
        };
        for locator in versions {
            if !run.check() {
                return;
            }
            if !installation_entry(run, &locator) {
                continue;
            }
            let declared_version = entry_name(&locator);
            run.observe(Candidate {
                provider_package: ProviderPackage {
                    provider_id: ProviderId::Mise,
                    package_id: package_id.into(),
                    kind: PackageKind::Runtime,
                    platform: run.options.platform,
                },
                executable: locator.join("bin").join("node"),
                evidence: vec![evidence(
                    "mise-install-directory",
                    &locator,
                    Some("已观察安装目录布局，尚无管理器归属证据"),
                )],
                locator,
                declared_version,
                ownership: Ownership::Observed,
                trusted: true,
            });
        }
    }
}

pub(super) fn collect_brew(run: &mut ScanRun<'_>) {
    // 混合安装机器上的 Intel 与 Apple Silicon Cellar 分别保留。
    // 即使 PATH 中没有 brew，也能观察到已有安装目录。
    for cellar in run.options.brew_cellars.clone() {
        if !run.check() {
            return;
        }
        let Some(formulas) = run.read_dir(&cellar) else {
            continue;
        };
        run.mark_available();
        for formula_dir in formulas {
            if !run.check() {
                return;
            }
            let Some(package_id) = entry_name(&formula_dir) else {
                continue;
            };
            // 仅处理已登记的原生公式名，不从第三方公式名推断 Node 身份。
            if native_brew_node_formula(&package_id).is_none() {
                continue;
            }
            let Some(versions) = run.read_dir(&formula_dir) else {
                continue;
            };
            for locator in versions {
                if !run.check() {
                    return;
                }
                if !installation_entry(run, &locator) {
                    continue;
                }
                let receipt_path = locator.join("INSTALL_RECEIPT.json");
                let (ownership, trusted, receipt_evidence) = match run.read_metadata(&receipt_path)
                {
                    Some(bytes) => match receipt_origin(&bytes, &package_id) {
                        Ok(ReceiptOrigin::Core) => (
                            Ownership::Owned,
                            true,
                            evidence(
                                "brew-core-receipt",
                                &receipt_path,
                                Some("source.tap=homebrew/core；包身份来自原生 Cellar 公式目录"),
                            ),
                        ),
                        Ok(ReceiptOrigin::OtherTap(tap)) => {
                            let detail = format!("尚未验证的公式来源：{tap}");
                            (
                                Ownership::Unknown,
                                false,
                                evidence("brew-third-party-receipt", &receipt_path, Some(&detail)),
                            )
                        }
                        Err(message) => {
                            run.problem(
                                ProviderStatus::Error,
                                "brew_receipt_invalid",
                                &format!("{}: {message}", receipt_path.display()),
                            );
                            (
                                Ownership::Unknown,
                                false,
                                evidence("brew-receipt-invalid", &receipt_path, Some(message)),
                            )
                        }
                    },
                    None => {
                        let message = "安装收据缺失或无法读取";
                        run.problem(
                            ProviderStatus::Error,
                            "brew_receipt_unavailable",
                            &format!("{}: {message}", receipt_path.display()),
                        );
                        (
                            Ownership::Unknown,
                            false,
                            evidence("brew-receipt-unavailable", &receipt_path, Some(message)),
                        )
                    }
                };
                let declared_version = entry_name(&locator);
                run.observe(Candidate {
                    provider_package: ProviderPackage {
                        provider_id: ProviderId::Brew,
                        package_id: package_id.clone(),
                        kind: PackageKind::Formula,
                        platform: run.options.platform,
                    },
                    executable: locator.join("bin").join("node"),
                    evidence: vec![
                        evidence("brew-cellar-directory", &locator, Some(&package_id)),
                        receipt_evidence,
                    ],
                    locator,
                    declared_version,
                    ownership,
                    trusted,
                });
            }
        }
    }
}

fn directory_available(run: &mut ScanRun<'_>, path: &Path) -> bool {
    if !run.check() {
        return false;
    }
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => true,
        Ok(_) => {
            run.problem(
                ProviderStatus::Error,
                "invalid_data_directory",
                &format!("{} 不是目录", path.display()),
            );
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            metadata_problem(run, path, &error);
            false
        }
    }
}

fn installation_entry(run: &mut ScanRun<'_>, path: &Path) -> bool {
    // 此处若跟随软链接，会漏掉已经断开的安装目录。
    match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_dir() || metadata.file_type().is_symlink(),
        Err(error) => {
            metadata_problem(run, path, &error);
            // 目录枚举后即使条目被删除或权限变化，仍保留已观察到的安装位置。
            true
        }
    }
}

fn metadata_problem(run: &mut ScanRun<'_>, path: &Path, error: &std::io::Error) {
    let status = if error.kind() == std::io::ErrorKind::PermissionDenied {
        ProviderStatus::PermissionDenied
    } else {
        ProviderStatus::Error
    };
    run.problem(
        status,
        "directory_metadata_failed",
        &format!("{}: {error}", path.display()),
    );
}

fn entry_name(path: &Path) -> Option<String> {
    path.file_name()?.to_str().map(str::to_owned)
}

fn evidence(kind: &str, path: &Path, detail: Option<&str>) -> ObservationEvidence {
    ObservationEvidence {
        kind: kind.into(),
        path: Some(path.to_string_lossy().into_owned()),
        detail: detail.map(str::to_owned),
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ReceiptOrigin {
    Core,
    OtherTap(String),
}

pub(super) fn receipt_origin(bytes: &[u8], formula: &str) -> Result<ReceiptOrigin, &'static str> {
    let receipt: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "安装收据不是有效 JSON")?;
    let source = receipt
        .get("source")
        .and_then(serde_json::Value::as_object)
        .ok_or("安装收据缺少 source 对象")?;
    let tap = source
        .get("tap")
        .and_then(serde_json::Value::as_str)
        .filter(|tap| !tap.is_empty())
        .ok_or("安装收据缺少 source.tap 身份")?;
    if tap != "homebrew/core" {
        return Ok(ReceiptOrigin::OtherTap(tap.into()));
    }
    // 旧收据指向公式 Ruby 文件；现代 API 收据指向共享的 packages JWS，
    // 由原生 Cellar 目录补充包身份。若 Ruby 文件名与目录冲突则拒绝授予归属。
    if let Some(path) = source.get("path") {
        let path = Path::new(path.as_str().ok_or("安装收据 source.path 不是字符串")?);
        if path.extension().is_some_and(|extension| extension == "rb")
            && path.file_stem().and_then(|name| name.to_str()) != Some(formula)
        {
            return Err("安装收据公式路径与 Cellar 包身份冲突");
        }
    }
    Ok(ReceiptOrigin::Core)
}

#[cfg(test)]
mod tests {
    use super::{receipt_origin, ReceiptOrigin};

    #[test]
    fn recognizes_current_api_and_legacy_core_receipts() {
        // 结构来自本机 Homebrew 2026 API 收据；复现判断无需执行命令或读取本机路径。
        let current = br#"{"source":{"spec":"stable","versions":{"stable":"22.20.0","head":null,"version_scheme":0},"path":"/fixture/Library/Caches/Homebrew/api/internal/packages.arm64.jws.json","tap_git_head":null,"tap":"homebrew/core"},"arch":"arm64"}"#;
        let legacy = br#"{"source":{"path":"/fixture/Library/Taps/homebrew/homebrew-core/Formula/n/node@22.rb","tap":"homebrew/core"}}"#;
        assert_eq!(receipt_origin(current, "node@22"), Ok(ReceiptOrigin::Core));
        assert_eq!(receipt_origin(legacy, "node@22"), Ok(ReceiptOrigin::Core));
    }

    #[test]
    fn receipt_cannot_assert_core_ownership_from_a_name_alone() {
        let third_party = br#"{"source":{"path":"/fixture/node.rb","tap":"someone/runtime"}}"#;
        assert_eq!(
            receipt_origin(third_party, "node"),
            Ok(ReceiptOrigin::OtherTap("someone/runtime".into()))
        );
        for invalid in [
            b"not-json".as_slice(),
            br#"{}"#,
            br#"{"source":{"tap":null}}"#,
            br#"{"source":{"tap":""}}"#,
            br#"{"source":{"tap":"homebrew/core","path":null}}"#,
            br#"{"source":{"tap":"homebrew/core","path":"/fixture/not-node.rb"}}"#,
        ] {
            assert!(receipt_origin(invalid, "node").is_err());
        }
    }
}

#[cfg(all(test, unix))]
mod scan_tests {
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};

    use crate::domain::{Health, Ownership, Platform, ProviderId, ProviderStatus};
    use crate::providers::{scan_node_provider, CancellationToken, NodeScanOptions, ScanLimits};

    struct Fixture {
        _root: tempfile::TempDir,
        options: NodeScanOptions,
        manager_marker: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("home");
            let fake_bin = root.path().join("fake-bin");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&fake_bin).unwrap();
            let manager_marker = root.path().join("manager-called");
            for name in ["mise", "brew"] {
                executable(
                    &fake_bin.join(name),
                    &format!("#!/bin/sh\n: > {}\nexit 99\n", shell_quote(&manager_marker)),
                );
            }
            let options = NodeScanOptions {
                platform: Platform::Macos,
                mise_data_dir: home.join(".local/share/mise"),
                nvm_dir: home.join(".nvm"),
                brew_cellars: vec![
                    root.path().join("opt/homebrew/Cellar"),
                    root.path().join("usr/local/Cellar"),
                ],
                path_entries: vec![fake_bin],
                home,
                limits: ScanLimits::default(),
            };
            Self {
                _root: root,
                options,
                manager_marker,
            }
        }

        fn brew_install(
            &self,
            cellar: usize,
            formula: &str,
            version: &str,
            receipt: Option<&str>,
        ) -> (PathBuf, PathBuf) {
            let locator = self.options.brew_cellars[cellar]
                .join(formula)
                .join(version);
            let marker = locator.join("probe-called");
            fs::create_dir_all(locator.join("bin")).unwrap();
            fake_node(&locator.join("bin/node"), &marker);
            if let Some(receipt) = receipt {
                fs::write(locator.join("INSTALL_RECEIPT.json"), receipt).unwrap();
            }
            (locator, marker)
        }

        fn scan(&self, provider_id: ProviderId) -> crate::domain::ProviderScan {
            let scan =
                scan_node_provider(provider_id, &self.options, &CancellationToken::default());
            assert!(!self.manager_marker.exists());
            scan
        }
    }

    fn shell_quote(path: &Path) -> String {
        format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
    }

    fn executable(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn fake_node(path: &Path, marker: &Path) {
        executable(
            path,
            &format!(
                "#!/bin/sh\n: > {}\nprintf '%s' '{{\"version\":\"22.20.0\",\"architecture\":\"arm64\"}}'\n",
                shell_quote(marker)
            ),
        );
    }

    #[test]
    fn only_verified_core_receipt_grants_ownership_and_executes_node() {
        let fixture = Fixture::new();
        let (official, official_marker) = fixture.brew_install(
            0,
            "node@22",
            "22.20.0_1",
            Some(r#"{"source":{"tap":"homebrew/core","path":"/fixture/packages.arm64.jws.json"}}"#),
        );
        let (third_party, third_party_marker) = fixture.brew_install(
            0,
            "node",
            "22.20.0",
            Some(r#"{"source":{"tap":"someone/runtime","path":"/fixture/node.rb"}}"#),
        );
        let scan = fixture.scan(ProviderId::Brew);
        assert_eq!(scan.report.status, ProviderStatus::Ok);
        assert!(scan.report.coverage.complete);
        assert_eq!(scan.observations.len(), 2);
        let owned = scan
            .observations
            .iter()
            .find(|observation| observation.locator == official.to_string_lossy())
            .unwrap();
        assert_eq!(owned.ownership, Ownership::Owned);
        assert_eq!(owned.health, Health::Healthy);
        assert_eq!(owned.version.as_deref(), Some("22.20.0"));
        assert_eq!(owned.architecture.as_deref(), Some("arm64"));
        assert_eq!(owned.provider_package.package_id, "node@22");
        assert!(official_marker.exists());
        let unknown = scan
            .observations
            .iter()
            .find(|observation| observation.locator == third_party.to_string_lossy())
            .unwrap();
        assert_eq!(unknown.ownership, Ownership::Unknown);
        assert_eq!(unknown.health, Health::Unverified);
        assert_eq!(unknown.version, None);
        assert!(!third_party_marker.exists());
    }

    #[test]
    fn missing_and_corrupt_receipts_preserve_untrusted_installations() {
        let fixture = Fixture::new();
        let (_, missing_marker) = fixture.brew_install(0, "node@20", "20.19.5", None);
        let (_, corrupt_marker) = fixture.brew_install(0, "node@22", "22.20.0", Some("{broken"));
        let scan = fixture.scan(ProviderId::Brew);
        assert_eq!(scan.report.status, ProviderStatus::Error);
        assert!(!scan.report.coverage.complete);
        assert!(scan.report.error.is_some());
        assert_eq!(scan.observations.len(), 2);
        for observation in &scan.observations {
            assert_eq!(observation.ownership, Ownership::Unknown);
            assert_eq!(observation.health, Health::Unverified);
            assert_eq!(observation.version, None);
        }
        assert!(!missing_marker.exists());
        assert!(!corrupt_marker.exists());
    }

    #[test]
    fn same_formula_and_version_in_both_cellars_remain_distinct() {
        let fixture = Fixture::new();
        let receipt = r#"{"source":{"tap":"homebrew/core","path":"/fixture/node@22.rb"}}"#;
        let (arm_locator, _) = fixture.brew_install(0, "node@22", "22.20.0", Some(receipt));
        let (intel_locator, _) = fixture.brew_install(1, "node@22", "22.20.0", Some(receipt));
        let scan = fixture.scan(ProviderId::Brew);
        assert_eq!(scan.report.status, ProviderStatus::Ok);
        assert_eq!(scan.observations.len(), 2);
        assert!(scan
            .observations
            .iter()
            .any(|observation| observation.locator == arm_locator.to_string_lossy()));
        assert!(scan
            .observations
            .iter()
            .any(|observation| observation.locator == intel_locator.to_string_lossy()));
        assert!(scan
            .observations
            .iter()
            .all(|observation| observation.version.as_deref() == Some("22.20.0")));
    }

    #[test]
    fn mise_preserves_broken_symlink_and_legacy_alias_without_claiming_ownership() {
        let fixture = Fixture::new();
        let current = fixture.options.mise_data_dir.join("installs/node/22.20.0");
        let legacy = fixture
            .options
            .mise_data_dir
            .join("installs/nodejs/20.19.5");
        fs::create_dir_all(current.join("bin")).unwrap();
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let marker = current.join("probe-called");
        fake_node(&current.join("bin/node"), &marker);
        symlink(fixture.options.home.join("missing-installation"), &legacy).unwrap();
        let scan = fixture.scan(ProviderId::Mise);
        assert_eq!(scan.observations.len(), 2);
        assert!(scan
            .observations
            .iter()
            .all(|observation| observation.ownership == Ownership::Observed));
        let broken = scan
            .observations
            .iter()
            .find(|observation| observation.locator == legacy.to_string_lossy())
            .unwrap();
        assert_eq!(broken.provider_package.package_id, "nodejs");
        assert_eq!(broken.health, Health::Broken);
        assert_eq!(broken.version, None);
        assert!(marker.exists());
    }
}
