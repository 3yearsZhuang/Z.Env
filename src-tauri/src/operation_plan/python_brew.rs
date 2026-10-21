//! Homebrew core Python formula 写渠道：白名单、依赖/归属门禁与结果核验。
//! Homebrew 不承诺精确补丁版；执行时版本由 formula 决定，结果以 keg 与重扫为准。
use super::*;
use crate::process::probe::{run_probe, ProbeError, ProbeLimits};
use crate::providers::python::PythonScanOptions;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Facts {
    prefix: PathBuf,
    cellar: PathBuf,
    formula: String,
    formula_name: String,
    kegs: Vec<String>,
    dependents: Vec<String>,
}

#[derive(Clone)]
pub(super) struct WriteSpec {
    pub action: Action,
    pub program: PathBuf,
    pub argv: Vec<String>,
    pub formula: String,
    pub locator: Option<PathBuf>,
    pub impact: Vec<String>,
    facts: Facts,
}

fn denied(message: &'static str) -> PlanError {
    PlanError::Capability(message)
}

pub(super) fn formula_whitelist(package: &str) -> Option<String> {
    if package == "python" {
        return Some("python".into());
    }
    let minor = package.strip_prefix("python@3.")?;
    if minor.is_empty() || minor.len() > 2 || !minor.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let minor: u32 = minor.parse().ok()?;
    (8..=99).contains(&minor).then(|| package.to_owned())
}

fn command(program: &Path, options: &PythonScanOptions) -> Command {
    writes::command(program, ProviderId::Brew, &options.common)
}

fn output(program: &Path, options: &PythonScanOptions, args: &[&str]) -> Result<String, PlanError> {
    let mut cmd = command(program, options);
    cmd.args(args);
    let result = run_probe(
        &mut cmd,
        &ProbeLimits {
            timeout: Duration::from_secs(15),
            max_output_bytes: 512 * 1024,
        },
        &CancellationToken::default(),
    )
    .map_err(|_| denied("Homebrew 证据读取失败或超出预算"))?;
    if !result.status.success() {
        return Err(denied("Homebrew 未提供所需 Python 能力证据"));
    }
    String::from_utf8(result.stdout).map_err(|_| PlanError::EvidenceUnavailable)
}

fn facts(program: &Path, options: &PythonScanOptions, formula: &str) -> Result<Facts, PlanError> {
    let prefix = PathBuf::from(output(program, options, &["--prefix"])?.trim());
    let cellar = PathBuf::from(output(program, options, &["--cellar"])?.trim());
    if !prefix.is_absolute() || !cellar.is_absolute() {
        return Err(PlanError::EvidenceUnavailable);
    }
    let info: serde_json::Value = serde_json::from_str(&output(
        program,
        options,
        &["info", "--json=v2", "--formula", formula],
    )?)
    .map_err(|_| denied("Homebrew formula 信息格式不支持"))?;
    let name = info
        .get("formulae")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("name"))
        .and_then(|value| value.as_str())
        .filter(|name| name.len() <= 128)
        .ok_or(denied("Homebrew 未返回所需 core formula"))?;
    let list = output(program, options, &["list", "--versions"])?;
    let mut kegs = vec![];
    for line in list.lines() {
        let mut words = line.split_whitespace();
        if words.next() == Some(formula) {
            kegs.extend(words.map(|word| word.chars().take(64).collect::<String>()));
        }
    }
    kegs.sort();
    kegs.dedup();
    if kegs.len() > 32 {
        return Err(denied("Homebrew keg 数量超出预算"));
    }
    let uses = output(program, options, &["uses", "--installed", formula])?;
    let dependents: Vec<String> = uses
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(64)
        .map(|line| line.chars().take(128).collect())
        .collect();
    Ok(Facts {
        prefix,
        cellar,
        formula: formula.to_owned(),
        formula_name: name.to_owned(),
        kegs,
        dependents,
    })
}

pub(super) fn environment_keys() -> Vec<String> {
    writes::environment_keys(ProviderId::Brew)
}

fn ready(snapshot: &InventorySnapshot) -> bool {
    snapshot.providers.iter().any(|provider| {
        provider.provider_id == ProviderId::Brew
            && has_complete_coverage(provider, &ToolId::from("python"))
    })
}

fn locator(facts: &Facts, version: &str) -> PathBuf {
    facts.cellar.join(&facts.formula).join(version)
}

pub(super) fn prepare(
    action: Action,
    package: &ProviderPackage,
    target: Option<&Installation>,
    version: Option<&str>,
    snapshot: &InventorySnapshot,
    options: &PythonScanOptions,
) -> Result<WriteSpec, PlanError> {
    if options.common.platform != Platform::Macos
        || !cfg!(unix)
        || package.provider_id != ProviderId::Brew
        || package.kind != PackageKind::Formula
        || package.platform != Platform::Macos
        || !matches!(action, Action::Install | Action::Uninstall)
    {
        return Err(PlanError::Unsupported);
    }
    if version.is_some() {
        return Err(denied("Homebrew 不能承诺精确补丁版，请改用 formula"));
    }
    let formula = formula_whitelist(&package.package_id)
        .ok_or(denied("仅支持 Homebrew core python 或 python@3.N formula"))?;
    if !ready(snapshot) {
        return Err(denied("Python Homebrew 来源未完整读取，不能执行写操作"));
    }
    if action == Action::Uninstall {
        let target = target.ok_or(PlanError::Unsupported)?;
        if target.provider_package != *package || target.health != Health::Healthy || target.stale {
            return Err(denied("卸载需要当前健康的 Homebrew Python 实例"));
        }
    }
    let program =
        guards::executable(&options.common, "brew").ok_or(denied("没有可定位的 brew 程序"))?;
    let facts = facts(&program, options, &formula)?;
    if facts.formula_name != formula && facts.formula_name != package.package_id {
        return Err(denied("Homebrew formula 身份与请求不一致"));
    }
    let (argv, locator) = if action == Action::Install {
        if !facts.kegs.is_empty() {
            return Err(denied("Homebrew 已安装该 formula，不覆盖或静默升级"));
        }
        (
            vec![
                "install".into(),
                "--formula".into(),
                format!("homebrew/core/{formula}"),
            ],
            None,
        )
    } else {
        let target = target.ok_or(PlanError::Unsupported)?;
        let installed = facts
            .kegs
            .first()
            .ok_or(denied("Homebrew 没有该 formula 的 keg 记录"))?;
        if facts.kegs.len() != 1 {
            return Err(denied("Homebrew 存在多个 keg，未推断卸载范围"));
        }
        if !facts.dependents.is_empty() {
            return Err(denied("Homebrew formula 仍被其他 formula 依赖，保持只读"));
        }
        if target.version.as_deref() != Some(installed.as_str())
            || fs::canonicalize(&target.locator).ok()
                != fs::canonicalize(locator(&facts, installed)).ok()
        {
            return Err(denied("所选实例与 Homebrew keg 记录不一致"));
        }
        (
            vec!["uninstall".into(), "--formula".into(), formula.clone()],
            Some(locator(&facts, installed)),
        )
    };
    let impact = vec![
        format!(
            "Homebrew core {formula}：cellar {}，前缀 {}",
            facts.cellar.display(),
            facts.prefix.display()
        ),
        "Homebrew 可能安装或更新依赖、改写 python3 链接、site-packages 与缓存；版本由执行时 formula 决定".into(),
        "不支持精确补丁版承诺；已有 venv、项目和终端可能使用旧解释器，不自动改写".into(),
        "卸载只移除该 formula 的唯一 keg；仍被依赖时拒绝；失败或取消保留已产生的 keg 与依赖".into(),
    ];
    Ok(WriteSpec {
        action,
        program,
        argv,
        formula,
        locator,
        impact,
        facts,
    })
}

impl WriteSpec {
    pub fn revalidate(
        &self,
        target: Option<&Installation>,
        snapshot: &InventorySnapshot,
        options: &PythonScanOptions,
    ) -> Result<(), PlanError> {
        let package = ProviderPackage {
            provider_id: ProviderId::Brew,
            package_id: self.formula.clone(),
            kind: PackageKind::Formula,
            platform: Platform::Macos,
        };
        let next = prepare(self.action, &package, target, None, snapshot, options)?;
        if self.facts != next.facts
            || self.program != next.program
            || self.argv != next.argv
            || self.locator != next.locator
        {
            return Err(PlanError::Preconditions);
        }
        Ok(())
    }
    pub fn run(
        &self,
        options: &PythonScanOptions,
        token: &CancellationToken,
    ) -> Result<(), PlanError> {
        let mut cmd = command(&self.program, options);
        cmd.args(&self.argv);
        let output = run_probe(
            &mut cmd,
            &ProbeLimits {
                timeout: Duration::from_secs(900),
                max_output_bytes: 512 * 1024,
            },
            token,
        )
        .map_err(|error| {
            if matches!(error, ProbeError::Cancelled) {
                PlanError::Cancelled
            } else {
                PlanError::Command
            }
        })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(PlanError::Command)
        }
    }
    pub fn verify(
        &self,
        snapshot: &InventorySnapshot,
        options: &PythonScanOptions,
    ) -> Result<(), PlanError> {
        let native = facts(&self.program, options, &self.formula)?;
        if self.action == Action::Install {
            if !ready(snapshot) || native.kegs.len() != 1 {
                return Err(PlanError::Verification);
            }
            let expected = native
                .kegs
                .first()
                .map(|version| locator(&native, version))
                .ok_or(PlanError::Verification)?;
            let expected_package = ProviderPackage {
                provider_id: ProviderId::Brew,
                package_id: self.formula.clone(),
                kind: PackageKind::Formula,
                platform: Platform::Macos,
            };
            snapshot
                .installations
                .iter()
                .find(|item| {
                    item.tool_id == ToolId::from("python")
                        && item.provider_package == expected_package
                        && !item.stale
                        && item.health == Health::Healthy
                        && item.version.is_some()
                        && fs::canonicalize(&item.locator).ok() == fs::canonicalize(&expected).ok()
                })
                .map(|_| ())
                .ok_or(PlanError::Verification)
        } else {
            if !native.kegs.is_empty() {
                return Err(PlanError::Verification);
            }
            if snapshot.installations.iter().any(|item| {
                item.provider_package.provider_id == ProviderId::Brew
                    && item.provider_package.package_id == self.formula
                    && !item.stale
            }) {
                return Err(PlanError::Verification);
            }
            if !ready(snapshot) {
                return Err(PlanError::ScanIncomplete);
            }
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::{Health, InstallationId, Ownership, ProviderReport, ScanCoverage};
    use std::os::unix::fs::PermissionsExt;

    fn options() -> (tempfile::TempDir, PythonScanOptions) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let options = PythonScanOptions {
            common: NodeScanOptions {
                platform: Platform::Macos,
                home: root.join("home"),
                mise_data_dir: root.join("mise"),
                nvm_dir: root.join("nvm"),
                brew_cellars: vec![root.join("prefix/Cellar")],
                path_entries: vec![root.join("bin")],
                limits: Default::default(),
            },
            pyenv_root: root.join("pyenv"),
            uv_python_dir: root.join("uv"),
        };
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        let script = r#"#!/bin/sh
ROOT='@ROOT@'
case "$1" in
--prefix) printf '%s\n' "$ROOT/prefix"; exit 0;;
--cellar) printf '%s\n' "$ROOT/prefix/Cellar"; exit 0;;
info) printf '%s' '{"formulae":[{"name":"python@3.13"}]}'; exit 0;;
list) [ -f "$ROOT/list" ] && /bin/cat "$ROOT/list"; exit 0;;
uses) [ -f "$ROOT/uses" ] && /bin/cat "$ROOT/uses"; exit 0;;
install)
  [ -f "$ROOT/zero" ] && exit 0
  if [ -f "$ROOT/slow" ]; then /usr/bin/touch "$ROOT/started"; /bin/sleep 30; fi
  /bin/mkdir -p "$ROOT/prefix/Cellar/python@3.13/3.13.5/bin"
  printf '%s\n' 'python@3.13 3.13.5' > "$ROOT/list"
  exit 0;;
uninstall)
  /bin/rm -rf "$ROOT/prefix/Cellar/python@3.13"
  : > "$ROOT/list"
  exit 0;;
esac
exit 99
"#
        .replace("@ROOT@", &root.display().to_string());
        let path = root.join("bin/brew");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (directory, options)
    }

    fn package() -> ProviderPackage {
        ProviderPackage {
            provider_id: ProviderId::Brew,
            package_id: "python@3.13".into(),
            kind: PackageKind::Formula,
            platform: Platform::Macos,
        }
    }

    fn report(complete: bool) -> ProviderReport {
        ProviderReport {
            provider_id: ProviderId::Brew,
            status: ProviderStatus::Ok,
            observed_at: 0,
            stale: false,
            coverage: ScanCoverage {
                tool_ids: vec![ToolId::from("python")],
                complete,
                truncated: false,
            },
            error: None,
        }
    }

    fn snapshot(installations: Vec<Installation>, complete: bool) -> InventorySnapshot {
        InventorySnapshot {
            schema_version: 1,
            request_id: "snapshot".into(),
            observed_at: 0,
            tools: vec![],
            providers: vec![report(complete)],
            installations,
            aliases: vec![],
        }
    }

    fn installation(root: &Path) -> Installation {
        Installation {
            installation_id: InstallationId::from("brew-python-3.13.5"),
            provider_package: package(),
            tool_id: ToolId::from("python"),
            locator: root
                .join("prefix/Cellar/python@3.13/3.13.5")
                .to_string_lossy()
                .into(),
            executable_path: Some(
                root.join("prefix/Cellar/python@3.13/3.13.5/bin/python3")
                    .to_string_lossy()
                    .into(),
            ),
            canonical_executable_path: Some(
                root.join("prefix/Cellar/python@3.13/3.13.5/bin/python3")
                    .to_string_lossy()
                    .into(),
            ),
            version: Some("3.13.5".into()),
            architecture: Some("arm64".into()),
            health: Health::Healthy,
            ownership: Ownership::Owned,
            evidence: vec![],
            stale: false,
        }
    }

    #[test]
    fn formula_whitelist_accepts_core_and_rejects_precise_or_third_party() {
        assert_eq!(formula_whitelist("python"), Some("python".into()));
        assert_eq!(formula_whitelist("python@3.13"), Some("python@3.13".into()));
        for value in [
            "python@3",
            "python@3.7",
            "python@3.13.1",
            "python@4.0",
            "python3",
            "pyenv",
        ] {
            assert_eq!(formula_whitelist(value), None, "{value}");
        }
    }

    #[test]
    fn install_formula_writes_keg_and_verifies_snapshot() {
        let (_directory, options) = options();
        let spec = prepare(
            Action::Install,
            &package(),
            None,
            None,
            &snapshot(vec![], true),
            &options,
        )
        .unwrap();
        assert_eq!(
            spec.argv,
            ["install", "--formula", "homebrew/core/python@3.13"]
        );
        spec.run(&options, &CancellationToken::default()).unwrap();
        let root = &options.common.home.parent().unwrap().to_path_buf();
        spec.verify(&snapshot(vec![installation(root)], true), &options)
            .unwrap();
    }

    #[test]
    fn install_refuses_existing_keg_exact_version_and_missing_source() {
        let (directory, options) = options();
        std::fs::write(directory.path().join("list"), "python@3.13 3.13.5\n").unwrap();
        assert!(prepare(
            Action::Install,
            &package(),
            None,
            None,
            &snapshot(vec![], true),
            &options,
        )
        .is_err());
        assert!(prepare(
            Action::Install,
            &package(),
            None,
            Some("3.13.5"),
            &snapshot(vec![], true),
            &options,
        )
        .is_err());
        assert!(prepare(
            Action::Install,
            &package(),
            None,
            None,
            &snapshot(vec![], false),
            &options,
        )
        .is_err());
    }

    #[test]
    fn uninstall_requires_single_unused_keg_and_owned_target() {
        let (directory, options) = options();
        std::fs::write(directory.path().join("list"), "python@3.13 3.13.5\n").unwrap();
        std::fs::create_dir_all(
            directory
                .path()
                .join("prefix/Cellar/python@3.13/3.13.5/bin"),
        )
        .unwrap();
        std::fs::write(
            directory
                .path()
                .join("prefix/Cellar/python@3.13/3.13.5/bin/python3"),
            "",
        )
        .unwrap();
        let root = directory.path().to_path_buf();
        let target = installation(&root);
        let spec = prepare(
            Action::Uninstall,
            &package(),
            Some(&target),
            None,
            &snapshot(vec![target.clone()], true),
            &options,
        )
        .unwrap();
        assert_eq!(spec.argv, ["uninstall", "--formula", "python@3.13"]);
        spec.run(&options, &CancellationToken::default()).unwrap();
        spec.verify(&snapshot(vec![], true), &options).unwrap();

        std::fs::write(directory.path().join("list"), "python@3.13 3.13.5\n").unwrap();
        std::fs::create_dir_all(
            directory
                .path()
                .join("prefix/Cellar/python@3.13/3.13.5/bin"),
        )
        .unwrap();
        std::fs::write(directory.path().join("uses"), "other\n").unwrap();
        assert!(prepare(
            Action::Uninstall,
            &package(),
            Some(&target),
            None,
            &snapshot(vec![target.clone()], true),
            &options,
        )
        .is_err());
    }

    #[test]
    fn cancelled_install_reaps_manager_before_keg() {
        let (directory, options) = options();
        std::fs::write(directory.path().join("slow"), "").unwrap();
        let spec = prepare(
            Action::Install,
            &package(),
            None,
            None,
            &snapshot(vec![], true),
            &options,
        )
        .unwrap();
        let token = CancellationToken::default();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| spec.run(&options, &token));
            let started = directory.path().join("started");
            let waited = std::time::Instant::now();
            while !started.exists() {
                assert!(waited.elapsed() < std::time::Duration::from_secs(5));
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            token.cancel();
            assert!(matches!(worker.join().unwrap(), Err(PlanError::Cancelled)));
        });
        assert!(!directory.path().join("prefix/Cellar/python@3.13").exists());
    }

    #[test]
    fn zero_exit_without_keg_and_external_change_are_not_success() {
        let (directory, options) = options();
        std::fs::write(directory.path().join("zero"), "").unwrap();
        let spec = prepare(
            Action::Install,
            &package(),
            None,
            None,
            &snapshot(vec![], true),
            &options,
        )
        .unwrap();
        spec.run(&options, &CancellationToken::default()).unwrap();
        assert!(matches!(
            spec.verify(&snapshot(vec![], true), &options),
            Err(PlanError::Verification)
        ));
        std::fs::write(directory.path().join("list"), "python@3.13 3.13.4\n").unwrap();
        assert!(spec
            .revalidate(None, &snapshot(vec![], true), &options)
            .is_err());
    }
}
