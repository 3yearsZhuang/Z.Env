//! 固定 core:python 的 mise 写适配；不允许插件、源码回退或默认 pip 包。
use super::*;
use crate::process::probe::{run_probe, ProbeError, ProbeLimits};
use crate::providers::python::PythonScanOptions;
use std::{fs, io::Read, process::Command};
const FLAGS: [&str; 3] = ["--no-config", "--no-env", "--no-hooks"];
#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq)]
struct Row {
    version: String,
    install_path: String,
    installed: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Facts {
    root: PathBuf,
    rows: Vec<Row>,
    receipt: Option<String>,
    packages: ConfigFingerprint,
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
fn packages_file(options: &PythonScanOptions) -> PathBuf {
    options
        .common
        .mise_data_dir
        .join("zenv-command-config/no-python-packages")
}
pub(super) fn command(program: &Path, options: &PythonScanOptions) -> Command {
    let mut cmd = writes::command(program, ProviderId::Mise, &options.common);
    cmd.env_remove("MISE_NODE_COMPILE")
        .env("MISE_PYTHON_COMPILE", "0")
        .env("MISE_PYTHON_DEFAULT_PACKAGES_FILE", packages_file(options));
    cmd
}
pub(super) fn environment_keys() -> Vec<String> {
    let mut keys = writes::environment_keys(ProviderId::Mise);
    keys.retain(|k| k != "MISE_NODE_COMPILE");
    keys.extend([
        "MISE_PYTHON_COMPILE".into(),
        "MISE_PYTHON_DEFAULT_PACKAGES_FILE".into(),
    ]);
    keys
}
fn output(program: &Path, options: &PythonScanOptions, args: &[&str]) -> Result<String, PlanError> {
    let mut cmd = command(program, options);
    cmd.args(args);
    let result = run_probe(
        &mut cmd,
        &ProbeLimits {
            timeout: Duration::from_secs(10),
            max_output_bytes: 256 * 1024,
        },
        &CancellationToken::default(),
    )
    .map_err(|_| denied("mise Python 证据读取失败或超出预算"))?;
    if !result.status.success() {
        return Err(denied("mise 未提供所需 Python 能力证据"));
    }
    String::from_utf8(result.stdout).map_err(|_| PlanError::EvidenceUnavailable)
}
fn resolve(path: &Path) -> Result<PathBuf, PlanError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    match fs::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let (f, _) = configuration(path)?;
            f.canonical_path
                .map(PathBuf::from)
                .ok_or(PlanError::EvidenceUnavailable)
        }
        Err(_) => Err(PlanError::EvidenceUnavailable),
    }
}
fn absent(path: &Path) -> Result<bool, PlanError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(PlanError::EvidenceUnavailable),
    }
}
fn receipt(options: &PythonScanOptions) -> Result<Option<String>, PlanError> {
    let path = options
        .common
        .mise_data_dir
        .join("installs/python/.mise.backend.toml");
    let (fingerprint, _) = configuration(&path)?;
    if !fingerprint.exists {
        return Ok(None);
    }
    if fs::symlink_metadata(&path)
        .map_err(|_| PlanError::EvidenceUnavailable)?
        .file_type()
        .is_symlink()
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    // configuration 已有有界原子指纹；再读后重新核对，避免使用另一个文件的内容。
    let mut open = fs::OpenOptions::new();
    open.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = open
        .open(&path)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    if !file
        .metadata()
        .map_err(|_| PlanError::EvidenceUnavailable)?
        .is_file()
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    let mut bytes = Vec::new();
    file.take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    if bytes.len() > 8192 {
        return Err(PlanError::EvidenceUnavailable);
    }
    let value: toml::Value =
        toml::from_str(std::str::from_utf8(&bytes).map_err(|_| PlanError::EvidenceUnavailable)?)
            .map_err(|_| PlanError::EvidenceUnavailable)?;
    if value.get("full").and_then(toml::Value::as_str) != Some("core:python") {
        return Err(denied("Python 目录没有 core:python 收据；不接管其他后端"));
    }
    if configuration(&path)?.0 != fingerprint {
        return Err(PlanError::Preconditions);
    }
    Ok(fingerprint.sha256)
}
fn facts(program: &Path, options: &PythonScanOptions) -> Result<Facts, PlanError> {
    let (packages, _) = configuration(&packages_file(options))?;
    if packages.exists {
        return Err(denied("受控默认包路径已有文件；拒绝自动运行额外 pip 安装"));
    }
    let help = output(program, options, &["--help"])?;
    if FLAGS.iter().any(|s| !help.contains(s)) {
        return Err(denied("mise 不支持禁用配置、环境和钩子的调用"));
    }
    if output(
        program,
        options,
        &[
            "--no-config",
            "--no-env",
            "--no-hooks",
            "settings",
            "get",
            "python.compile",
        ],
    )?
    .trim()
        != "false"
    {
        return Err(denied("mise 未确认禁用 Python 源码构建，不能执行"));
    }
    if output(
        program,
        options,
        &[
            "--no-config",
            "--no-env",
            "--no-hooks",
            "settings",
            "get",
            "python.default_packages_file",
        ],
    )?
    .trim()
        != packages_file(options).to_string_lossy()
    {
        return Err(denied("mise 未确认受控默认包路径，不能执行"));
    }
    let mut rows: Vec<Row> = serde_json::from_str(&output(
        program,
        options,
        &[
            "--no-config",
            "--no-env",
            "--no-hooks",
            "ls",
            "--installed",
            "--json",
            "core:python",
        ],
    )?)
    .map_err(|_| denied("mise Python 安装记录格式不支持"))?;
    if rows.len() > 128
        || rows.iter().any(|r| {
            !r.installed
                || r.version.len() > 128
                || r.install_path.len() > 4096
                || !Path::new(&r.install_path).is_absolute()
        })
    {
        return Err(denied("mise Python 安装记录不完整或超出预算"));
    }
    rows.sort_by(|a, b| (&a.version, &a.install_path).cmp(&(&b.version, &b.install_path)));
    Ok(Facts {
        root: resolve(&options.common.mise_data_dir)?,
        rows,
        receipt: receipt(options)?,
        packages,
    })
}
pub(super) fn ready(snapshot: &InventorySnapshot) -> bool {
    snapshot.providers.iter().any(|p| {
        p.provider_id == ProviderId::Mise && has_complete_coverage(p, &ToolId::from("python"))
    })
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
        || package.provider_id != ProviderId::Mise
        || package.package_id != "python"
        || package.kind != PackageKind::Runtime
        || package.platform != Platform::Macos
        || !matches!(action, Action::Install | Action::Uninstall)
    {
        return Err(PlanError::Unsupported);
    }
    let version = version
        .or_else(|| target.and_then(|t| t.version.as_deref()))
        .filter(|v| crate::providers::python::stable(v))
        .ok_or(PlanError::Unsupported)?
        .to_owned();
    let root = options.common.mise_data_dir.join("installs/python");
    let locator = root.join(&version);
    let root_missing = absent(&root)?;
    let empty_new_source = action == Action::Install
        && root_missing
        && snapshot.providers.iter().any(|p| {
            p.provider_id == ProviderId::Mise
                && p.status == ProviderStatus::Unavailable
                && !p.stale
                && !p.coverage.truncated
                && p.coverage.tool_ids == vec![ToolId::from("python")]
                && p.error
                    .as_ref()
                    .is_some_and(|e| e.code == "sourceUnavailable")
        })
        && !snapshot
            .installations
            .iter()
            .any(|i| i.provider_package.provider_id == ProviderId::Mise);
    if !ready(snapshot) && !empty_new_source {
        return Err(denied("Python mise 来源未完整读取，不能执行写操作"));
    }
    if !root_missing
        && fs::symlink_metadata(&root)
            .map_err(|_| PlanError::EvidenceUnavailable)?
            .file_type()
            .is_symlink()
    {
        return Err(denied("Python 安装根目录是外部链接，保持只读"));
    }
    if action == Action::Install && !absent(&locator)? {
        return Err(denied("该精确 Python 目录已存在，不覆盖原安装或链接"));
    }
    if action == Action::Uninstall {
        let target = target.ok_or(PlanError::Unsupported)?;
        if target.tool_id != ToolId::from("python")
            || target.provider_package != *package
            || target.stale
            || target.health != Health::Healthy
            || Path::new(&target.locator) != locator
            || receipt(options)?.is_none()
            || !snapshot.installations.iter().any(|i| i == target)
        {
            return Err(denied("卸载需要当前健康的原生 core:python 精确实例和收据"));
        }
        if fs::symlink_metadata(&locator)
            .map_err(|_| PlanError::EvidenceUnavailable)?
            .file_type()
            .is_symlink()
        {
            return Err(denied("不能卸载外部 Python 链接"));
        }
        let executable = target
            .executable_path
            .as_ref()
            .ok_or(PlanError::EvidenceUnavailable)?;
        if !resolve(Path::new(executable))?.starts_with(resolve(&locator)?) {
            return Err(denied("Python 可执行文件位于安装目录之外，保持只读"));
        }
    }
    let program =
        guards::executable(&options.common, "mise").ok_or(denied("没有可定位的 mise 程序"))?;
    let facts = facts(&program, options)?;
    if resolve(&root)? != facts.root.join("installs/python") {
        return Err(denied("Python 来源父目录发生重定向，保持只读"));
    }
    if !root_missing && facts.receipt.is_none() {
        let mut entries = fs::read_dir(&root).map_err(|_| PlanError::EvidenceUnavailable)?;
        if !facts.rows.is_empty()
            || snapshot
                .installations
                .iter()
                .any(|i| i.provider_package.provider_id == ProviderId::Mise)
            || entries
                .next()
                .transpose()
                .map_err(|_| PlanError::EvidenceUnavailable)?
                .is_some()
        {
            return Err(denied(
                "已有 Python 来源缺少原生 core 收据，不能通过安装隐式接管",
            ));
        }
    }
    let rows: Vec<_> = facts.rows.iter().filter(|r| r.version == version).collect();
    if action == Action::Install {
        if !rows.is_empty() || (root_missing && !facts.rows.is_empty()) {
            return Err(denied("管理器已有安装记录或来源状态不一致，不覆盖安装"));
        }
    } else if rows.len() != 1 || resolve(Path::new(&rows[0].install_path))? != resolve(&locator)? {
        return Err(denied("原生 Python 管理器记录与所选实例不一致"));
    }
    let mut argv = FLAGS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    argv.extend([
        if action == Action::Install {
            "install"
        } else {
            "uninstall"
        }
        .into(),
        format!("core:python@{version}"),
    ]);
    let impact = vec![
        format!("固定原生 core:python@{version}：{}", locator.display()),
        "仅预编译 CPython；不加载配置、环境与钩子，默认 pip 包文件指向已核对的不存在路径；不运行 mise use".into(),
        "可能更新下载、缓存、状态及 shims；失败或取消保留已产生的文件，重扫后报告，不自动删除或重新安装".into(),
        "卸载只移除这个精确安装；依赖它的虚拟环境、项目和终端可能失效，不自动改写这些引用".into(),
        "mise 可更新 3、次版本及 latest 别名和 shims；已有宽泛版本请求的实际解析可能变化，需另行核对".into(),
    ];
    Ok(WriteSpec {
        action,
        program,
        argv,
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
        let package = ProviderPackage {
            provider_id: ProviderId::Mise,
            package_id: "python".into(),
            kind: PackageKind::Runtime,
            platform: Platform::Macos,
        };
        let next = prepare(
            self.action,
            &package,
            target,
            Some(&self.version),
            snapshot,
            options,
        )?;
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
        // 即使外部进程在重验后创建默认包文件，也不进入安装器。
        if configuration(&packages_file(options))?.0 != self.facts.packages {
            return Err(PlanError::Preconditions);
        }
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
        .map_err(|e| {
            if matches!(e, ProbeError::Cancelled) {
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
        let native = facts(&self.program, options)?;
        let rows: Vec<_> = native
            .rows
            .iter()
            .filter(|r| r.version == self.version)
            .collect();
        if self.action == Action::Install {
            if !ready(snapshot) {
                return Err(PlanError::ScanIncomplete);
            }
            if native.receipt.is_none()
                || rows.len() != 1
                || resolve(Path::new(&rows[0].install_path))? != resolve(&self.locator)?
                || fs::symlink_metadata(&self.locator)
                    .map_err(|_| PlanError::Verification)?
                    .file_type()
                    .is_symlink()
            {
                return Err(PlanError::Verification);
            }
            let expected = resolve(&self.locator)?;
            snapshot
                .installations
                .iter()
                .find(|i| {
                    i.tool_id == ToolId::from("python")
                        && i.provider_package.provider_id == ProviderId::Mise
                        && i.provider_package.package_id == "python"
                        && !i.stale
                        && i.health == Health::Healthy
                        && i.version.as_deref() == Some(&self.version)
                        && Path::new(&i.locator) == self.locator
                        && i.canonical_executable_path
                            .as_ref()
                            .is_some_and(|p| Path::new(p).starts_with(&expected))
                })
                .map(|_| ())
                .ok_or(PlanError::Verification)
        } else {
            // 最后一个版本被移除后，mise 可移除整个来源目录；原生空记录 + 物理不存在仍可证明该目标移除。
            if !rows.is_empty()
                || !absent(&self.locator)?
                || snapshot.installations.iter().any(|i| {
                    i.provider_package.provider_id == ProviderId::Mise
                        && !i.stale
                        && Path::new(&i.locator) == self.locator
                })
            {
                return Err(PlanError::Verification);
            }
            if !ready(snapshot)
                && !(absent(&options.common.mise_data_dir.join("installs/python"))?
                    && native.rows.is_empty())
            {
                return Err(PlanError::ScanIncomplete);
            }
            Ok(())
        }
    }
}
