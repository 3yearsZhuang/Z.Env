use super::*;
use crate::inventory::merge_inventory;
use std::os::unix::fs::{symlink, PermissionsExt};
struct Fixture {
    root: tempfile::TempDir,
    options: PythonScanOptions,
}
fn script(path: &Path, version: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let value = serde_json::json!({"version":version,"implementation":"cpython","releaseLevel":"final","freeThreaded":false,"debug":false,"architecture":"arm64","execPath":path});
    fs::write(path, format!("#!/bin/sh\nprintf '%s' '{}'\n", value)).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let options = PythonScanOptions {
            common: NodeScanOptions {
                platform: Platform::Macos,
                home,
                mise_data_dir: root.path().join("mise"),
                nvm_dir: root.path().join("nvm"),
                brew_cellars: vec![root.path().join("Cellar")],
                path_entries: vec![root.path().join("bin")],
                limits: Default::default(),
            },
            pyenv_root: root.path().join("pyenv"),
            uv_python_dir: root.path().join("uv/python"),
        };
        Self { root, options }
    }
    fn scan(&self, p: ProviderId) -> ProviderScan {
        scan_python_provider(p, &self.options, &Default::default())
    }
}
#[test]
fn five_sources_keep_distinct_installations_and_path_is_never_executed() {
    let f = Fixture::new();
    script(
        &f.options
            .common
            .mise_data_dir
            .join("installs/python/3.13.8/bin/python3"),
        "3.13.8",
    );
    script(
        &f.options.pyenv_root.join("versions/3.13.8/bin/python3"),
        "3.13.8",
    );
    script(
        &f.options
            .uv_python_dir
            .join("cpython-3.13.8-macos-aarch64-none/bin/python3"),
        "3.13.8",
    );
    let keg = f.options.common.brew_cellars[0].join("python@3.13/3.13.8_1");
    script(&keg.join("bin/python3.13"), "3.13.8");
    fs::write(
        keg.join("INSTALL_RECEIPT.json"),
        r#"{"source":{"tap":"homebrew/core","path":"Formula/python@3.13.rb"}}"#,
    )
    .unwrap();
    let bin = &f.options.common.path_entries[0];
    fs::create_dir_all(bin).unwrap();
    symlink(keg.join("bin/python3.13"), bin.join("python3")).unwrap();
    let marker = f.root.path().join("PATH_MUST_NOT_RUN");
    fs::write(
        bin.join("python"),
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(bin.join("python"), fs::Permissions::from_mode(0o755)).unwrap();
    let scans = scan_python_sources(&f.options, &Default::default());
    assert_eq!(scans.len(), 5);
    let snapshot = merge_inventory("python-fixture".into(), 1, scans).unwrap();
    assert_eq!(snapshot.tools[0].display_name, "Python");
    assert_eq!(
        snapshot
            .installations
            .iter()
            .filter(|i| i.health == Health::Healthy)
            .count(),
        4
    );
    assert_eq!(
        snapshot
            .installations
            .iter()
            .filter(|i| i.provider_package.provider_id == ProviderId::Path)
            .count(),
        2
    );
    assert!(!marker.exists());
    assert_eq!(
        snapshot
            .installations
            .iter()
            .filter(|i| i.ownership == Ownership::Owned)
            .count(),
        1
    );
}
#[test]
fn unknown_receipts_variants_virtualenvs_and_broken_links_never_become_healthy() {
    let f = Fixture::new();
    let root = f.options.pyenv_root.join("versions");
    script(&root.join("3.13.8/bin/python3"), "3.13.8");
    fs::write(root.join("3.13.8/pyvenv.cfg"), "home=/fixture").unwrap();
    script(&root.join("3.13.8t/bin/python3"), "3.13.8");
    symlink("/missing/python", root.join("3.12.9")).unwrap();
    let scan = f.scan(ProviderId::Pyenv);
    assert!(scan
        .observations
        .iter()
        .all(|o| o.health != Health::Healthy));
    assert_eq!(scan.observations.len(), 2);
    assert!(!scan.report.coverage.complete);
    let keg = f.options.common.brew_cellars[0].join("python@3.13/3.13.8");
    script(&keg.join("bin/python3"), "3.13.8");
    fs::write(
        keg.join("INSTALL_RECEIPT.json"),
        r#"{"source":{"tap":"someone/runtime"}}"#,
    )
    .unwrap();
    let scan = f.scan(ProviderId::Brew);
    assert_eq!(scan.observations[0].health, Health::Unverified);
    assert_eq!(scan.observations[0].ownership, Ownership::Unknown);
}
#[test]
fn isolated_probe_uses_fixed_flags_and_clean_environment_and_rejects_wrong_versions() {
    let f = Fixture::new();
    let path = f
        .options
        .common
        .mise_data_dir
        .join("installs/python/3.13.8/bin/python3");
    script(&path, "3.12.9");
    let scan = f.scan(ProviderId::Mise);
    assert_eq!(scan.observations[0].health, Health::Broken);
    assert_eq!(scan.report.error.unwrap().code, "pythonVersionMismatch");
    let proof = f.root.path().join("proof");
    let json = serde_json::json!({"version":"3.13.8","implementation":"cpython","releaseLevel":"final","freeThreaded":false,"debug":false,"architecture":"arm64","execPath":path});
    fs::write(&path,format!("#!/bin/sh\nprintf '%s' \"$1|$2|$3|${{PYTHONPATH-unset}}|${{PYTHONSTARTUP-unset}}|${{NODE_OPTIONS-unset}}\" > '{}'\nprintf '%s' '{}'\n",proof.display(),json)).unwrap();
    let scan = f.scan(ProviderId::Mise);
    assert_eq!(scan.observations[0].health, Health::Healthy);
    assert_eq!(
        fs::read_to_string(proof).unwrap(),
        "-I|-S|-B|unset|unset|unset"
    );
}
#[test]
fn unsupported_platform_relative_roots_limits_and_cancellation_remain_explicit() {
    let mut f = Fixture::new();
    script(
        &f.options.pyenv_root.join("versions/3.13.8/bin/python3"),
        "3.13.8",
    );
    f.options.common.platform = Platform::Windows;
    assert_eq!(
        f.scan(ProviderId::Pyenv).report.status,
        ProviderStatus::Unsupported
    );
    f.options.common.platform = Platform::Macos;
    f.options.pyenv_root = PathBuf::from("relative");
    assert_eq!(
        f.scan(ProviderId::Pyenv).report.status,
        ProviderStatus::Error
    );
    f.options.pyenv_root = f.root.path().join("pyenv");
    f.options.common.limits.max_probes = 0;
    let scan = f.scan(ProviderId::Pyenv);
    assert!(scan.report.coverage.truncated);
    assert_eq!(scan.observations[0].health, Health::Unverified);
    let token = CancellationToken::default();
    token.cancel();
    let scan = scan_python_provider(ProviderId::Pyenv, &f.options, &token);
    assert_eq!(scan.report.status, ProviderStatus::Cancelled);
    assert!(scan.observations.is_empty());
}
#[test]
fn cancelling_running_python_reaps_process_and_never_reports_a_broken_install() {
    let f = Fixture::new();
    let path = f.options.pyenv_root.join("versions/3.13.8/bin/python3");
    script(&path, "3.13.8");
    let marker = f.root.path().join("started");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\n/usr/bin/touch '{}'\n/bin/sleep 30 & wait\n",
            marker.display()
        ),
    )
    .unwrap();
    let token = CancellationToken::default();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| scan_python_provider(ProviderId::Pyenv, &f.options, &token));
        let started = Instant::now();
        while !marker.exists() {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        token.cancel();
        let scan = worker.join().unwrap();
        assert_eq!(scan.report.status, ProviderStatus::Cancelled);
        assert_eq!(scan.observations[0].health, Health::Unverified);
        assert!(started.elapsed() < Duration::from_secs(10));
    });
}
#[test]
fn interpreter_implementation_and_late_executable_retargeting_stay_unverified() {
    let f = Fixture::new();
    let bin = f.options.pyenv_root.join("versions/3.13.8/bin");
    fs::create_dir_all(&bin).unwrap();
    let real = f.root.path().join("original-python");
    let other = f.root.path().join("replacement-python");
    script(&real, "3.13.8");
    script(&other, "3.13.8");
    let path = bin.join("python3");
    symlink(&real, &path).unwrap();
    let data = serde_json::json!({"version":"3.13.8","implementation":"cpython","releaseLevel":"final","freeThreaded":false,"debug":false,"architecture":"arm64","execPath":real});
    fs::write(
        &real,
        format!(
            "#!/bin/sh\n/bin/ln -sf '{}' '{}'\nprintf '%s' '{}'\n",
            other.display(),
            path.display(),
            data
        ),
    )
    .unwrap();
    assert_eq!(
        f.scan(ProviderId::Pyenv).observations[0].health,
        Health::Unverified
    );
    let data = serde_json::json!({"version":"3.13.8","implementation":"pypy","architecture":"arm64","execPath":other});
    fs::write(&other, format!("#!/bin/sh\nprintf '%s' '{}'\n", data)).unwrap();
    assert_eq!(
        f.scan(ProviderId::Pyenv).observations[0].health,
        Health::Unverified
    );
}
#[test]
fn shared_python_contract_retains_unknown_path_and_distinct_source_identities() {
    let raw: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/python-inventory/contract-v1.json"
    ))
    .unwrap();
    let snapshot: crate::domain::InventorySnapshot = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), raw);
    assert_eq!(snapshot.providers.len(), 5);
    assert!(snapshot
        .providers
        .iter()
        .all(|p| p.coverage.tool_ids == vec![ToolId::from("python")]));
    let path = snapshot
        .installations
        .iter()
        .find(|i| i.provider_package.provider_id == ProviderId::Path)
        .unwrap();
    assert_eq!(path.health, Health::Unverified);
    assert!(path.version.is_none());
}
#[test]
fn standard_mise_aliases_are_observed_without_execution_and_do_not_hide_full_coverage() {
    let f = Fixture::new();
    let source = f.options.common.mise_data_dir.join("installs/python");
    script(&source.join("3.12.12/bin/python3"), "3.12.12");
    for alias in ["3", "3.12", "latest"] {
        symlink("./3.12.12", source.join(alias)).unwrap();
    }
    let scan = f.scan(ProviderId::Mise);
    assert_eq!(scan.report.status, ProviderStatus::Ok);
    assert!(scan.report.coverage.complete);
    assert_eq!(
        scan.observations
            .iter()
            .filter(|i| i.health == Health::Healthy)
            .count(),
        1
    );
    assert_eq!(
        scan.observations
            .iter()
            .filter(|i| i.health == Health::Unverified && i.version.is_none())
            .count(),
        3
    );
    // A standard-looking name that points outside the exact managed directory does not acquire coverage or probe permission.
    fs::remove_file(source.join("latest")).unwrap();
    symlink(f.root.path(), source.join("latest")).unwrap();
    let scan = f.scan(ProviderId::Mise);
    assert!(!scan.report.coverage.complete);
    assert_eq!(scan.report.status, ProviderStatus::Unsupported);
}

#[cfg(target_os = "macos")]
#[test]
fn uv_minor_alias_stays_observable_without_hiding_full_coverage() {
    let f = Fixture::new();
    let exact = uv_dir_name("3.12.13").unwrap();
    let alias = uv_dir_name("3.12").unwrap();
    let real = f.options.uv_python_dir.join(&exact);
    script(&real.join("bin/python3"), "3.12.13");
    symlink(&real, f.options.uv_python_dir.join(&alias)).unwrap();
    let scan = f.scan(ProviderId::Uv);
    assert_eq!(scan.report.status, ProviderStatus::Ok);
    assert!(scan.report.error.is_none());
    assert!(scan.report.coverage.complete);
    assert_eq!(
        scan.observations
            .iter()
            .filter(|i| i.health == Health::Healthy)
            .count(),
        1
    );
    assert_eq!(
        scan.observations
            .iter()
            .filter(|i| i.health == Health::Unverified)
            .count(),
        1
    );
    assert!(scan
        .observations
        .iter()
        .any(|i| Path::new(&i.locator).ends_with(&exact)));
}

#[cfg(target_os = "macos")]
#[test]
fn uv_alias_pointing_outside_the_managed_directory_is_not_trusted() {
    let f = Fixture::new();
    let exact = uv_dir_name("3.12.13").unwrap();
    let alias = uv_dir_name("3.12").unwrap();
    let outside = f.root.path().join("outside").join(&exact);
    script(&outside.join("bin/python3"), "3.12.13");
    fs::create_dir_all(&f.options.uv_python_dir).unwrap();
    symlink(&outside, f.options.uv_python_dir.join(&alias)).unwrap();
    let scan = f.scan(ProviderId::Uv);
    assert!(!scan.report.coverage.complete);
    assert_eq!(scan.report.status, ProviderStatus::Unsupported);
}
