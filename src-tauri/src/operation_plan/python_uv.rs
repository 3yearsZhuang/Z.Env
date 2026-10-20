//! uv 托管 CPython：固定精确版本、受控环境与 --no-bin；只处理可核验的实例。
//!
//! uv 的 `python uninstall` 在目标不存在时仍退出 0，因此结果只以重验、重扫和文件
//! 证据为准；退出码不能作为删除成功的证明。
use super::*;
use crate::process::probe::{run_probe, ProbeError, ProbeLimits};
use crate::providers::python::{stable, uv_arch, uv_dir_name, PythonScanOptions};
use std::{
    fs,
    path::{Component, Path},
};

const FLAGS: [&str; 2] = ["--no-config", "--no-progress"];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Row {
    version: String,
    locator: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Facts {
    root: PathBuf,
    rows: Vec<Row>,
    version: String,
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

fn package() -> ProviderPackage {
    ProviderPackage {
        provider_id: ProviderId::Uv,
        package_id: "python".into(),
        kind: PackageKind::Runtime,
        platform: Platform::Macos,
    }
}

fn split_uv_name(name: &str) -> Option<(&str, &str, &str, &str)> {
    let rest = name.strip_prefix("cpython-")?;
    let fields: Vec<_> = rest.split('-').collect();
    if fields.len() != 4 {
        return None;
    }
    Some((fields[0], fields[1], fields[2], fields[3]))
}

fn supported_build(fields: (&str, &str, &str, &str)) -> bool {
    let (_, os, arch, libc) = fields;
    os == "macos" && Some(arch) == uv_arch() && libc == "none"
}

/// 根目录允许尚不存在（安装会创建），但必须绝对、无 `..`，且最近存在的祖先可规范化；
/// 已存在的根目录不能是外部链接。
fn root(options: &PythonScanOptions) -> Result<PathBuf, PlanError> {
    let path = &options.uv_python_dir;
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(denied("uv Python 根目录是外部链接，保持只读"))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(PlanError::EvidenceUnavailable),
    }
    resolve_nearest(path)
}

fn resolve_nearest(path: &Path) -> Result<PathBuf, PlanError> {
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or(PlanError::EvidenceUnavailable)?;
            }
            Err(_) => return Err(PlanError::EvidenceUnavailable),
        }
    }
    let canonical = fs::canonicalize(ancestor).map_err(|_| PlanError::EvidenceUnavailable)?;
    let suffix = path
        .strip_prefix(ancestor)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    Ok(if suffix.as_os_str().is_empty() {
        canonical
    } else {
        canonical.join(suffix)
    })
}

pub(super) fn argv(action: Action, version: &str, root: &Path) -> Result<Vec<String>, PlanError> {
    let mut value: Vec<String> = FLAGS.iter().map(|flag| (*flag).to_owned()).collect();
    value.push("python".into());
    match action {
        Action::Install => {
            value.push("install".into());
            value.push("--no-bin".into());
        }
        Action::Uninstall => value.push("uninstall".into()),
        _ => return Err(PlanError::Unsupported),
    }
    value.extend([
        "--install-dir".into(),
        root.to_string_lossy().into_owned(),
        format!("cpython@{version}"),
    ]);
    Ok(value)
}

fn output(
    program: &Path,
    options: &PythonScanOptions,
    root: &Path,
    args: &[&str],
) -> Result<String, PlanError> {
    let mut cmd = writes::uv_command(program, &options.common, root);
    cmd.args(args);
    let result = run_probe(
        &mut cmd,
        &ProbeLimits {
            timeout: Duration::from_secs(10),
            max_output_bytes: 256 * 1024,
        },
        &CancellationToken::default(),
    )
    .map_err(|_| denied("uv 证据读取失败或超出预算"))?;
    if !result.status.success() {
        return Err(denied("uv 未提供所需能力证据"));
    }
    String::from_utf8(result.stdout).map_err(|_| PlanError::EvidenceUnavailable)
}

fn facts(program: &Path, options: &PythonScanOptions, root: &Path) -> Result<Facts, PlanError> {
    let version = output(program, options, root, &["--version"])?;
    let mut pieces = version.split_whitespace();
    if pieces.next() != Some("uv") {
        return Err(denied("uv 版本输出不可识别"));
    }
    let version = pieces.next().ok_or(denied("uv 版本输出不可识别"))?;
    if !version
        .split('.')
        .take(2)
        .all(|part| part.parse::<u32>().is_ok())
    {
        return Err(denied("uv 版本输出不可识别"));
    }
    let version = version.to_owned();

    let help = output(program, options, root, &["--help"])?;
    if !help.contains("--no-config") {
        return Err(denied("uv 不支持禁用配置文件，不能执行"));
    }
    let install = output(program, options, root, &["python", "install", "--help"])?;
    if !install.contains("--no-bin") || !install.contains("--install-dir") {
        return Err(denied("uv 不支持 --no-bin 或受控安装目录，不能执行"));
    }
    let uninstall = output(program, options, root, &["python", "uninstall", "--help"])?;
    if !uninstall.contains("--install-dir") {
        return Err(denied("uv 卸载不支持受控安装目录，不能执行"));
    }

    let mut rows = Vec::new();
    let mut aliases = Vec::new();
    match fs::read_dir(root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|_| PlanError::EvidenceUnavailable)?;
                let name = entry.file_name();
                let name = name.to_str().ok_or(PlanError::EvidenceUnavailable)?;
                if name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                let meta =
                    fs::symlink_metadata(&path).map_err(|_| PlanError::EvidenceUnavailable)?;
                if meta.file_type().is_symlink() {
                    let target =
                        fs::canonicalize(&path).map_err(|_| denied("uv 次版本别名无法解析"))?;
                    let target_name = target
                        .file_name()
                        .and_then(|value| value.to_str())
                        .ok_or(denied("uv 次版本别名目标不可识别"))?;
                    let target_fields = split_uv_name(target_name)
                        .ok_or(denied("uv 次版本别名目标不是精确 CPython 目录"))?;
                    if !supported_build(target_fields) || !stable(target_fields.0) {
                        return Err(denied("uv 次版本别名目标不是受支持的 CPython 构建"));
                    }
                    let alias_fields =
                        split_uv_name(name).ok_or(denied("uv 次版本别名不可识别"))?;
                    if !alias_fields.0.starts_with("3.")
                        || alias_fields.0.split('.').count() != 2
                        || !target_fields.0.starts_with(&format!("{}.", alias_fields.0))
                    {
                        return Err(denied("uv 次版本别名与目标版本不一致"));
                    }
                    if target.parent() != Some(root) {
                        return Err(denied("uv 次版本别名指向根目录之外"));
                    }
                    aliases.push(target);
                } else if meta.is_dir() {
                    let fields = split_uv_name(name)
                        .ok_or(denied("uv Python 目录包含未支持的条目，保持只读"))?;
                    if !supported_build(fields) || !stable(fields.0) {
                        return Err(denied("uv Python 目录包含未支持的构建，保持只读"));
                    }
                    rows.push(Row {
                        version: fields.0.to_owned(),
                        locator: path,
                    });
                } else {
                    return Err(denied("uv Python 目录包含未知文件，保持只读"));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(PlanError::EvidenceUnavailable),
    }
    for target in aliases {
        if !rows.iter().any(|row| row.locator == target) {
            return Err(denied("uv 次版本别名指向未列出的目录"));
        }
    }
    rows.sort_by(|a, b| (&a.version, &a.locator).cmp(&(&b.version, &b.locator)));
    rows.dedup();
    Ok(Facts {
        root: root.to_owned(),
        rows,
        version,
    })
}

fn source_ready(snapshot: &InventorySnapshot) -> bool {
    snapshot.providers.iter().any(|provider| {
        provider.provider_id == ProviderId::Uv
            && has_complete_coverage(provider, &ToolId::from("python"))
    })
}

pub(super) fn environment_keys() -> Vec<String> {
    writes::environment_keys(ProviderId::Uv)
}

pub(super) fn prepare(
    action: Action,
    target: Option<&Installation>,
    version: Option<&str>,
    snapshot: &InventorySnapshot,
    options: &PythonScanOptions,
) -> Result<WriteSpec, PlanError> {
    let expected = package();
    if options.common.platform != Platform::Macos
        || !cfg!(unix)
        || expected.platform != Platform::Macos
        || !matches!(action, Action::Install | Action::Uninstall)
    {
        return Err(PlanError::Unsupported);
    }
    let version = version
        .or_else(|| target.and_then(|item| item.version.as_deref()))
        .filter(|value| stable(value))
        .ok_or(PlanError::Unsupported)?
        .to_owned();
    let root = root(options)?;
    let name = uv_dir_name(&version).ok_or(PlanError::Unsupported)?;
    let locator = root.join(name);
    let missing_root = match fs::symlink_metadata(&root) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => return Err(PlanError::EvidenceUnavailable),
    };
    let empty_new_source = action == Action::Install
        && missing_root
        && !snapshot
            .installations
            .iter()
            .any(|item| item.provider_package.provider_id == ProviderId::Uv)
        && snapshot.providers.iter().any(|provider| {
            provider.provider_id == ProviderId::Uv
                && provider.status == ProviderStatus::Unavailable
                && !provider.stale
                && !provider.coverage.truncated
                && provider.coverage.tool_ids == vec![ToolId::from("python")]
                && provider
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == "sourceUnavailable")
        });
    if !source_ready(snapshot) && !empty_new_source {
        return Err(denied("uv Python 来源未完整读取，不能执行写操作"));
    }
    let program =
        guards::executable(&options.common, "uv").ok_or(denied("没有可定位的 uv 程序"))?;
    let facts = facts(&program, options, &root)?;
    if facts.root != root {
        return Err(denied("uv Python 根目录发生重定向，保持只读"));
    }
    let existing = facts.rows.iter().find(|row| row.version == version);
    if action == Action::Install {
        if existing.is_some() || fs::symlink_metadata(&locator).is_ok() {
            return Err(denied("uv 已有该精确版本或目录，不覆盖安装"));
        }
    } else {
        let target = target.ok_or(PlanError::Unsupported)?;
        if target.tool_id != ToolId::from("python")
            || target.provider_package != expected
            || target.stale
            || target.health != Health::Healthy
            || fs::canonicalize(&target.locator).ok() != Some(locator.clone())
            || !snapshot.installations.iter().any(|item| item == target)
        {
            return Err(denied("卸载需要当前健康的 uv 托管精确实例"));
        }
        let row = existing.ok_or(denied("uv 目录没有该精确版本记录"))?;
        if row.locator != locator {
            return Err(denied("uv 记录与所选实例目录不一致"));
        }
        let executable = target
            .executable_path
            .as_ref()
            .ok_or(PlanError::EvidenceUnavailable)?;
        let canonical = fs::canonicalize(executable).map_err(|_| PlanError::EvidenceUnavailable)?;
        if !canonical.starts_with(&locator) {
            return Err(denied("uv 可执行文件位于安装目录之外，保持只读"));
        }
        if fs::symlink_metadata(&locator)
            .map_err(|_| PlanError::EvidenceUnavailable)?
            .file_type()
            .is_symlink()
        {
            return Err(denied("不能卸载外部 uv Python 链接"));
        }
    }
    let impact = vec![
        format!("固定 uv 托管 CPython@{version}：{}", locator.display()),
        "使用 --no-bin，不向 bin 目录安装入口；不写 Windows 注册表；缓存、下载与 .lock 可能保留"
            .into(),
        "uv 可能把同一次版本别名（cpython-3.12-*）重指向本次安装；别名只作观测，不授予额外归属"
            .into(),
        "失败或取消保留已产生的目录、缓存与别名，重扫后报告，不自动删除或重新安装".into(),
        "卸载只移除这个精确补丁版；依赖它的虚拟环境、项目和终端可能失效，不自动改写这些引用".into(),
        "uv 卸载在目标不存在时也退出 0，结果只以重验、重扫和文件证据为准".into(),
    ];
    Ok(WriteSpec {
        action,
        program,
        argv: argv(action, &version, &root)?,
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
        // 预览后目录或别名被外部改动时，命令阶段仍要拒绝。
        let current = facts(&self.program, options, &self.facts.root)?;
        if current != self.facts {
            return Err(PlanError::Preconditions);
        }
        let mut cmd = writes::uv_command(&self.program, &options.common, &self.facts.root);
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
        let native = facts(&self.program, options, &self.facts.root)?;
        let row = native.rows.iter().find(|row| row.version == self.version);
        if self.action == Action::Install {
            if !source_ready(snapshot) {
                return Err(PlanError::ScanIncomplete);
            }
            let row = row.ok_or(PlanError::Verification)?;
            if row.locator != self.locator
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
                    item.tool_id == ToolId::from("python")
                        && item.provider_package.provider_id == ProviderId::Uv
                        && item.provider_package.package_id == "python"
                        && !item.stale
                        && item.health == Health::Healthy
                        && item.version.as_deref() == Some(&self.version)
                        && fs::canonicalize(&item.locator).ok().as_deref()
                            == Some(self.locator.as_path())
                        && item
                            .canonical_executable_path
                            .as_ref()
                            .is_some_and(|path| Path::new(path).starts_with(&self.locator))
                })
                .map(|_| ())
                .ok_or(PlanError::Verification)
        } else {
            if row.is_some()
                || fs::symlink_metadata(&self.locator).is_ok()
                || snapshot.installations.iter().any(|item| {
                    item.provider_package.provider_id == ProviderId::Uv
                        && !item.stale
                        && fs::canonicalize(&item.locator).ok().as_deref()
                            == Some(self.locator.as_path())
                })
            {
                return Err(PlanError::Verification);
            }
            if !source_ready(snapshot)
                && !(native.rows.is_empty() && fs::symlink_metadata(&self.facts.root).is_err())
            {
                return Err(PlanError::ScanIncomplete);
            }
            Ok(())
        }
    }
}
