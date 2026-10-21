//! pyenv 受控源码编译渠道：精确版本、构建依赖/插件门禁与结果核验。
//! pyenv 通过 python-build 编译 CPython；执行时间长、依赖外部工具链，失败会留下构建产物。
use super::*;
use crate::process::probe::{run_probe, ProbeError, ProbeLimits};
use crate::providers::python::PythonScanOptions;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const BUILD_TOOLS: [&str; 6] = ["cc", "make", "patch", "git", "curl", "tar"];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Facts {
    root: PathBuf,
    versions: Vec<String>,
    plugin: bool,
    tools: Vec<String>,
}

#[derive(Clone)]
pub(super) struct WriteSpec {
    pub action: Action,
    pub program: PathBuf,
    pub argv: Vec<String>,
    pub version: String,
    pub locator: PathBuf,
    pub impact: Vec<String>,
    facts: Facts,
}

fn denied(message: &'static str) -> PlanError {
    PlanError::Capability(message)
}

fn stable(value: &str) -> bool {
    crate::providers::python::stable(value)
}

fn root(options: &PythonScanOptions) -> Result<PathBuf, PlanError> {
    let path = &options.pyenv_root;
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    let meta = fs::symlink_metadata(path).map_err(|_| denied("pyenv 根目录不存在或不可读取"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(denied("pyenv 根目录是链接或不是目录，保持只读"));
    }
    path.canonicalize()
        .map_err(|_| PlanError::EvidenceUnavailable)
}

fn command(program: &Path, options: &PythonScanOptions, root: &Path) -> Command {
    writes::pyenv_command(program, &options.common, root)
}

fn output(
    program: &Path,
    options: &PythonScanOptions,
    root: &Path,
    args: &[&str],
) -> Result<String, PlanError> {
    let mut cmd = command(program, options, root);
    cmd.args(args);
    let result = run_probe(
        &mut cmd,
        &ProbeLimits {
            timeout: Duration::from_secs(15),
            max_output_bytes: 256 * 1024,
        },
        &CancellationToken::default(),
    )
    .map_err(|_| denied("pyenv 证据读取失败或超出预算"))?;
    if !result.status.success() {
        return Err(denied("pyenv 未提供所需源码编译能力证据"));
    }
    String::from_utf8(result.stdout).map_err(|_| PlanError::EvidenceUnavailable)
}

fn facts(program: &Path, options: &PythonScanOptions, root: &Path) -> Result<Facts, PlanError> {
    output(program, options, root, &["--version"])?;
    let reported = output(program, options, root, &["root"])?;
    let reported = PathBuf::from(reported.trim());
    if reported.canonicalize().ok() != Some(root.to_path_buf()) {
        return Err(denied("pyenv root 与扫描根目录不一致"));
    }
    let commands = output(program, options, root, &["commands"])?;
    let plugin = commands.lines().any(|line| line.trim() == "install");
    let tools: Vec<String> = BUILD_TOOLS
        .iter()
        .filter(|name| guards::executable(&options.common, name).is_some())
        .map(|name| (*name).to_owned())
        .collect();
    let mut versions = vec![];
    if let Ok(entries) = fs::read_dir(root.join("versions")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str().filter(|name| stable(name)) else {
                continue;
            };
            if fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.is_dir()) {
                versions.push(name.to_owned());
            }
        }
    }
    versions.sort();
    versions.dedup();
    if versions.len() > 64 || tools.len() > BUILD_TOOLS.len() {
        return Err(denied("pyenv 版本或工具链证据超出预算"));
    }
    Ok(Facts {
        root: root.to_path_buf(),
        versions,
        plugin,
        tools,
    })
}

pub(super) fn environment_keys() -> Vec<String> {
    writes::pyenv_environment_keys()
}

pub(super) fn argv(action: Action, version: &str) -> Result<Vec<String>, PlanError> {
    match action {
        Action::Install => Ok(vec!["install".into(), version.into()]),
        Action::Uninstall => Ok(vec!["uninstall".into(), "-f".into(), version.into()]),
        _ => Err(PlanError::Unsupported),
    }
}

pub(super) fn prepare(
    action: Action,
    target: Option<&Installation>,
    version: Option<&str>,
    snapshot: &InventorySnapshot,
    options: &PythonScanOptions,
) -> Result<WriteSpec, PlanError> {
    if options.common.platform != Platform::Macos
        || !cfg!(unix)
        || !matches!(action, Action::Install | Action::Uninstall)
    {
        return Err(PlanError::Unsupported);
    }
    let version = version
        .or_else(|| target.and_then(|item| item.version.as_deref()))
        .filter(|value| stable(value))
        .ok_or(denied("pyenv 需要精确稳定 CPython 版本"))?
        .to_owned();
    if !snapshot.providers.iter().any(|provider| {
        provider.provider_id == ProviderId::Pyenv
            && has_complete_coverage(provider, &ToolId::from("python"))
    }) {
        return Err(denied("Python pyenv 来源未完整读取，不能执行编译"));
    }
    let root = root(options)?;
    let locator = root.join("versions").join(&version);
    let program =
        guards::executable(&options.common, "pyenv").ok_or(denied("没有可定位的 pyenv 程序"))?;
    let facts = facts(&program, options, &root)?;
    if !facts.plugin {
        return Err(denied("pyenv 缺少 install/python-build 插件能力"));
    }
    let missing: Vec<_> = BUILD_TOOLS
        .iter()
        .filter(|name| !facts.tools.iter().any(|tool| tool == **name))
        .collect();
    if !missing.is_empty() {
        return Err(PlanError::Capability(
            "pyenv 编译缺少构建工具：cc/make/patch/git/curl/tar",
        ));
    }
    let installed = facts.versions.iter().any(|item| item == &version);
    if action == Action::Install {
        if installed || fs::symlink_metadata(&locator).is_ok() {
            return Err(denied("pyenv 已有该精确版本目录，不覆盖编译"));
        }
    } else {
        let target = target.ok_or(PlanError::Unsupported)?;
        if target.provider_package.provider_id != ProviderId::Pyenv
            || target.tool_id != ToolId::from("python")
            || target.health != Health::Healthy
            || target.stale
        {
            return Err(denied("卸载需要当前健康的 pyenv Python 实例"));
        }
        if !installed || fs::canonicalize(&target.locator).ok() != fs::canonicalize(&locator).ok() {
            return Err(denied("所选实例与 pyenv 版本目录不一致"));
        }
        if fs::symlink_metadata(&locator)
            .map_err(|_| PlanError::EvidenceUnavailable)?
            .file_type()
            .is_symlink()
        {
            return Err(denied("不能卸载外部 pyenv 链接"));
        }
        let executable = target
            .executable_path
            .as_ref()
            .ok_or(PlanError::EvidenceUnavailable)?;
        if !fs::canonicalize(executable)
            .map_err(|_| PlanError::EvidenceUnavailable)?
            .starts_with(&locator)
        {
            return Err(denied("pyenv 可执行文件位于版本目录之外，保持只读"));
        }
    }
    let impact = vec![
        format!("pyenv 源码编译 CPython {version}：{}", locator.display()),
        "通过 python-build 编译；需要 Xcode CLT 及 OpenSSL/readline 等构建依赖，可能联网下载源码与补丁".into(),
        "插件、补丁与构建脚本可能参与；可能耗时数分钟到数十分钟，失败会留下部分构建产物".into(),
        "不自动安装系统依赖、不 source shell、不改项目版本请求；rehash 与已有 venv 影响需另行核对".into(),
        "卸载只移除该精确版本目录；依赖它的虚拟环境、项目和终端可能失效".into(),
    ];
    Ok(WriteSpec {
        action,
        program,
        argv: argv(action, &version)?,
        version,
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
        let next = prepare(self.action, target, Some(&self.version), snapshot, options)?;
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
        let mut cmd = command(&self.program, options, &self.facts.root);
        cmd.args(&self.argv);
        let output = run_probe(
            &mut cmd,
            &ProbeLimits {
                timeout: Duration::from_secs(3600),
                max_output_bytes: 1024 * 1024,
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
        let native = facts(&self.program, options, &self.facts.root)?;
        let installed = native.versions.iter().any(|item| item == &self.version);
        if self.action == Action::Install {
            if !installed
                || fs::symlink_metadata(&self.locator)
                    .map_err(|_| PlanError::Verification)?
                    .file_type()
                    .is_symlink()
            {
                return Err(PlanError::Verification);
            }
            snapshot
                .installations
                .iter()
                .find(|item| {
                    item.provider_package.provider_id == ProviderId::Pyenv
                        && item.tool_id == ToolId::from("python")
                        && !item.stale
                        && item.health == Health::Healthy
                        && item.version.as_deref() == Some(&self.version)
                        && fs::canonicalize(&item.locator).ok()
                            == fs::canonicalize(&self.locator).ok()
                })
                .map(|_| ())
                .ok_or(PlanError::Verification)
        } else {
            if installed || fs::symlink_metadata(&self.locator).is_ok() {
                return Err(PlanError::Verification);
            }
            if snapshot.installations.iter().any(|item| {
                item.provider_package.provider_id == ProviderId::Pyenv
                    && !item.stale
                    && fs::canonicalize(&item.locator).ok() == fs::canonicalize(&self.locator).ok()
            }) {
                return Err(PlanError::Verification);
            }
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::{
        Health, InstallationId, Ownership, PackageKind, ProviderPackage, ProviderReport,
        ScanCoverage,
    };
    use std::os::unix::fs::PermissionsExt;

    fn script(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn options() -> (tempfile::TempDir, PythonScanOptions) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let pyenv = root.join("pyenv");
        fs::create_dir_all(pyenv.join("versions")).unwrap();
        fs::create_dir_all(pyenv.join("bin")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        let canonical = pyenv.canonicalize().unwrap();
        let text = format!(
            r#"#!/bin/sh
ROOT='{root}'
case "$1" in
--version) printf 'pyenv 2.4.0\n'; exit 0;;
root) printf '%s\n' "$ROOT"; exit 0;;
commands) printf 'install\nuninstall\nversions\n'; exit 0;;
install)
  [ -f "{root}/zero" ] && exit 0
  if [ -f "{root}/slow" ]; then /usr/bin/touch "{root}/started"; /bin/sleep 30; fi
  /bin/mkdir -p "$ROOT/versions/$2/bin"
  printf '#!/bin/sh\nexit 0\n' > "$ROOT/versions/$2/bin/python3"
  /bin/chmod 755 "$ROOT/versions/$2/bin/python3"
  exit 0;;
uninstall) /bin/rm -rf "$ROOT/versions/$4"; exit 0;;
esac
exit 99
"#,
            root = canonical.display()
        );
        script(&pyenv.join("bin/pyenv"), &text);
        for name in ["cc", "make", "patch", "git", "curl", "tar"] {
            script(&pyenv.join("bin").join(name), "#!/bin/sh\nexit 0\n");
        }
        let options = PythonScanOptions {
            common: NodeScanOptions {
                platform: Platform::Macos,
                home: root.join("home"),
                mise_data_dir: root.join("mise"),
                nvm_dir: root.join("nvm"),
                brew_cellars: vec![root.join("Cellar")],
                path_entries: vec![pyenv.join("bin")],
                limits: Default::default(),
            },
            pyenv_root: pyenv,
            uv_python_dir: root.join("uv"),
        };
        (directory, options)
    }

    fn package() -> ProviderPackage {
        ProviderPackage {
            provider_id: ProviderId::Pyenv,
            package_id: "python".into(),
            kind: PackageKind::Runtime,
            platform: Platform::Macos,
        }
    }

    fn snapshot(
        _options: &PythonScanOptions,
        installations: Vec<Installation>,
    ) -> InventorySnapshot {
        InventorySnapshot {
            schema_version: 1,
            request_id: "snapshot".into(),
            observed_at: 0,
            tools: vec![],
            providers: vec![ProviderReport {
                provider_id: ProviderId::Pyenv,
                status: ProviderStatus::Ok,
                observed_at: 0,
                stale: false,
                coverage: ScanCoverage {
                    tool_ids: vec![ToolId::from("python")],
                    complete: true,
                    truncated: false,
                },
                error: None,
            }],
            installations,
            aliases: vec![],
        }
    }

    fn installation(options: &PythonScanOptions, version: &str) -> Installation {
        let locator = options.pyenv_root.join("versions").join(version);
        Installation {
            installation_id: InstallationId::from("pyenv-version"),
            provider_package: package(),
            tool_id: ToolId::from("python"),
            locator: locator.to_string_lossy().into(),
            executable_path: Some(locator.join("bin/python3").to_string_lossy().into()),
            canonical_executable_path: Some(locator.join("bin/python3").to_string_lossy().into()),
            version: Some(version.into()),
            architecture: Some("arm64".into()),
            health: Health::Healthy,
            ownership: Ownership::Owned,
            evidence: vec![],
            stale: false,
        }
    }

    #[test]
    fn pyenv_install_compiles_exact_version_and_verifies() {
        let (_directory, options) = options();
        let spec = prepare(
            Action::Install,
            None,
            Some("3.13.15"),
            &snapshot(&options, vec![]),
            &options,
        )
        .unwrap();
        assert_eq!(spec.argv, ["install", "3.13.15"]);
        spec.run(&options, &CancellationToken::default()).unwrap();
        assert!(options
            .pyenv_root
            .join("versions/3.13.15/bin/python3")
            .is_file());
        spec.verify(
            &snapshot(&options, vec![installation(&options, "3.13.15")]),
            &options,
        )
        .unwrap();
    }

    #[test]
    fn pyenv_uninstall_removes_exact_version_and_rechecks() {
        let (_directory, options) = options();
        fs::create_dir_all(options.pyenv_root.join("versions/3.13.15/bin")).unwrap();
        fs::write(
            options.pyenv_root.join("versions/3.13.15/bin/python3"),
            "#!/bin/sh\nexit 0\n",
        )
        .unwrap();
        let target = installation(&options, "3.13.15");
        let spec = prepare(
            Action::Uninstall,
            Some(&target),
            None,
            &snapshot(&options, vec![target.clone()]),
            &options,
        )
        .unwrap();
        assert_eq!(spec.argv, ["uninstall", "-f", "3.13.15"]);
        spec.run(&options, &CancellationToken::default()).unwrap();
        assert!(!options.pyenv_root.join("versions/3.13.15").exists());
        spec.verify(&snapshot(&options, vec![]), &options).unwrap();
    }

    #[test]
    fn pyenv_requires_build_tools_plugin_and_absent_target() {
        let (_directory, options) = options();
        fs::create_dir_all(options.pyenv_root.join("versions/3.13.15/bin")).unwrap();
        assert!(prepare(
            Action::Install,
            None,
            Some("3.13.15"),
            &snapshot(&options, vec![]),
            &options,
        )
        .is_err());
        fs::remove_dir_all(options.pyenv_root.join("versions/3.13.15")).unwrap();
        fs::remove_file(options.pyenv_root.join("bin/cc")).unwrap();
        assert!(prepare(
            Action::Install,
            None,
            Some("3.13.15"),
            &snapshot(&options, vec![]),
            &options,
        )
        .is_err());
        script(&options.pyenv_root.join("bin/cc"), "#!/bin/sh\nexit 0\n");
        script(
            &options.pyenv_root.join("bin/pyenv"),
            "#!/bin/sh\nif [ \"$1\" = commands ]; then printf 'versions\\n'; exit 0; fi\nexit 0\n",
        );
        assert!(prepare(
            Action::Install,
            None,
            Some("3.13.15"),
            &snapshot(&options, vec![]),
            &options,
        )
        .is_err());
    }

    #[test]
    fn cancelled_pyenv_install_reaps_build_process() {
        let (_directory, options) = options();
        fs::write(options.pyenv_root.join("slow"), "").unwrap();
        let spec = prepare(
            Action::Install,
            None,
            Some("3.13.15"),
            &snapshot(&options, vec![]),
            &options,
        )
        .unwrap();
        let token = CancellationToken::default();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| spec.run(&options, &token));
            let started = options.pyenv_root.join("started");
            let waited = std::time::Instant::now();
            while !started.exists() {
                assert!(waited.elapsed() < std::time::Duration::from_secs(5));
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            token.cancel();
            assert!(matches!(worker.join().unwrap(), Err(PlanError::Cancelled)));
        });
        assert!(!options.pyenv_root.join("versions/3.13.15").exists());
    }

    #[test]
    fn pyenv_zero_exit_without_directory_is_not_success() {
        let (_directory, options) = options();
        fs::write(options.pyenv_root.parent().unwrap().join("zero"), "").unwrap();
        let spec = prepare(
            Action::Install,
            None,
            Some("3.13.15"),
            &snapshot(&options, vec![]),
            &options,
        )
        .unwrap();
        spec.run(&options, &CancellationToken::default()).unwrap();
        assert!(matches!(
            spec.verify(&snapshot(&options, vec![]), &options),
            Err(PlanError::Verification)
        ));
    }
}
