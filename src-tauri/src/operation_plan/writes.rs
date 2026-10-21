//! Node 写渠道：固定来源、原生包、受控环境与管理器证据；不接受前端程序参数。
use super::{exact_version, guards, model::*, ready, PlanError};
use crate::{
    domain::*,
    process::probe::{run_probe, ProbeError, ProbeLimits},
    providers::{CancellationToken, NodeScanOptions},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
const MISE_FLAGS: [&str; 3] = ["--no-config", "--no-env", "--no-hooks"];
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct MiseRow {
    version: String,
    install_path: String,
    installed: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
enum Facts {
    Mise {
        rows: Vec<MiseRow>,
        receipt: Option<String>,
        root: PathBuf,
    },
    Brew {
        prefix: PathBuf,
        cellar: PathBuf,
        version: String,
        revision: u64,
        installed: Vec<String>,
        dependents: Vec<String>,
        dependencies: Vec<String>,
        fingerprint: String,
    },
}
#[derive(Clone)]
pub(super) struct WriteSpec {
    pub action: Action,
    pub provider: ProviderId,
    pub program: PathBuf,
    pub argv: Vec<String>,
    pub package: String,
    pub version: String,
    pub locator: PathBuf,
    pub impact: Vec<String>,
    facts: Facts,
}
fn capability(message: &'static str) -> PlanError {
    PlanError::Capability(message)
}
fn base_command(program: &Path, options: &NodeScanOptions) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .current_dir(&options.home)
        .env("HOME", &options.home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    cmd
}
pub(super) fn command(program: &Path, provider: ProviderId, options: &NodeScanOptions) -> Command {
    let mut cmd = base_command(program, options);
    if provider == ProviderId::Mise {
        for (key, value) in [
            ("MISE_NO_CONFIG", "1"),
            ("MISE_NO_ENV", "1"),
            ("MISE_NO_HOOKS", "1"),
            ("MISE_YES", "1"),
            ("MISE_NODE_COMPILE", "0"),
            ("MISE_COLOR", "0"),
        ] {
            cmd.env(key, value);
        }
        for (key, path) in [
            ("MISE_DATA_DIR", options.mise_data_dir.clone()),
            ("MISE_INSTALLS_DIR", options.mise_data_dir.join("installs")),
            ("MISE_CACHE_DIR", options.mise_data_dir.join("cache")),
            ("MISE_STATE_DIR", options.mise_data_dir.join("state")),
            (
                "MISE_DOWNLOADS_DIR",
                options.mise_data_dir.join("downloads"),
            ),
            ("MISE_SHIMS_DIR", options.mise_data_dir.join("shims")),
            (
                "MISE_CONFIG_DIR",
                options.mise_data_dir.join("zenv-command-config"),
            ),
        ] {
            cmd.env(key, path);
        }
    } else {
        for key in [
            "HOMEBREW_NO_AUTO_UPDATE",
            "HOMEBREW_NO_ANALYTICS",
            "HOMEBREW_NO_INSTALL_CLEANUP",
            "HOMEBREW_NO_AUTOREMOVE",
            "HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK",
            "HOMEBREW_NO_INSTALL_UPGRADE",
            "HOMEBREW_NO_ENV_HINTS",
            "HOMEBREW_NO_ASK",
            "HOMEBREW_NO_COLOR",
        ] {
            cmd.env(key, "1");
        }
    }
    cmd
}
/// uv 只接受 Python 来源明确给出的安装根目录；缓存固定在受控 HOME 下，避免写用户缓存。
pub(super) fn uv_command(
    program: &Path,
    options: &NodeScanOptions,
    uv_python_dir: &Path,
) -> Command {
    let mut cmd = base_command(program, options);
    for (key, value) in [
        ("UV_NO_CONFIG", "1"),
        ("UV_NO_PROGRESS", "1"),
        ("UV_COLOR", "never"),
    ] {
        cmd.env(key, value);
    }
    cmd.env("UV_PYTHON_INSTALL_DIR", uv_python_dir)
        .env("UV_CACHE_DIR", options.home.join(".cache/uv"));
    cmd
}
/// 允许读取项目/全局 mise 配置的实际解析命令；不加载 shell、不运行 hooks。
/// 与 `command` 不同，这里不设置 `MISE_NO_CONFIG`，也不重定向 `MISE_CONFIG_DIR`。
pub(super) fn mise_resolution_command(program: &Path, options: &NodeScanOptions) -> Command {
    let mut cmd = base_command(program, options);
    for (key, value) in [
        ("MISE_YES", "1"),
        ("MISE_COLOR", "0"),
        ("MISE_NO_HOOKS", "1"),
    ] {
        cmd.env(key, value);
    }
    for (key, path) in [
        ("MISE_DATA_DIR", options.mise_data_dir.clone()),
        ("MISE_INSTALLS_DIR", options.mise_data_dir.join("installs")),
        ("MISE_CACHE_DIR", options.mise_data_dir.join("cache")),
        ("MISE_STATE_DIR", options.mise_data_dir.join("state")),
        (
            "MISE_DOWNLOADS_DIR",
            options.mise_data_dir.join("downloads"),
        ),
        ("MISE_SHIMS_DIR", options.mise_data_dir.join("shims")),
    ] {
        cmd.env(key, path);
    }
    cmd
}

/// pyenv 受控编译：固定根目录与工具链 PATH，不加载用户 shell 或项目配置。
pub(super) fn pyenv_command(program: &Path, options: &NodeScanOptions, root: &Path) -> Command {
    let mut cmd = base_command(program, options);
    let path = std::env::join_paths([
        root.join("bin"),
        root.join("shims"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ])
    .unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    cmd.env("PYENV_ROOT", root)
        .env("PYENV_YES", "1")
        .env("PATH", path);
    cmd
}
pub(super) fn pyenv_environment_keys() -> Vec<String> {
    vec![
        "HOME".into(),
        "PATH".into(),
        "PYENV_ROOT".into(),
        "PYENV_YES".into(),
    ]
}
pub(super) fn environment_keys(provider: ProviderId) -> Vec<String> {
    let mut keys = vec!["HOME".into(), "PATH".into()];
    let names: &[&str] = match provider {
        ProviderId::Mise => &[
            "MISE_NO_CONFIG",
            "MISE_NO_ENV",
            "MISE_NO_HOOKS",
            "MISE_YES",
            "MISE_NODE_COMPILE",
            "MISE_COLOR",
            "MISE_DATA_DIR",
            "MISE_INSTALLS_DIR",
            "MISE_CACHE_DIR",
            "MISE_STATE_DIR",
            "MISE_DOWNLOADS_DIR",
            "MISE_SHIMS_DIR",
            "MISE_CONFIG_DIR",
        ],
        ProviderId::Uv => &[
            "UV_NO_CONFIG",
            "UV_NO_PROGRESS",
            "UV_COLOR",
            "UV_PYTHON_INSTALL_DIR",
            "UV_CACHE_DIR",
        ],
        _ => &[
            "HOMEBREW_NO_AUTO_UPDATE",
            "HOMEBREW_NO_ANALYTICS",
            "HOMEBREW_NO_INSTALL_CLEANUP",
            "HOMEBREW_NO_AUTOREMOVE",
            "HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK",
            "HOMEBREW_NO_INSTALL_UPGRADE",
            "HOMEBREW_NO_ENV_HINTS",
            "HOMEBREW_NO_ASK",
            "HOMEBREW_NO_COLOR",
        ],
    };
    keys.extend(names.iter().map(|s| s.to_string()));
    keys
}
pub(super) fn output(
    program: &Path,
    provider: ProviderId,
    options: &NodeScanOptions,
    args: &[&str],
) -> Result<String, PlanError> {
    if provider == ProviderId::Brew {
        check_brew_environment(program, options)?;
    }
    let mut cmd = command(program, provider, options);
    cmd.args(args);
    let result = run_probe(
        &mut cmd,
        &ProbeLimits {
            timeout: Duration::from_secs(10),
            max_output_bytes: 256 * 1024,
        },
        &CancellationToken::default(),
    )
    .map_err(|_| capability("管理器证据读取超时或失败，暂不能生成可执行计划"))?;
    if !result.status.success() {
        return Err(capability("管理器未提供完整证据，暂不能生成可执行计划"));
    }
    String::from_utf8(result.stdout).map_err(|_| capability("管理器输出格式不受支持"))
}

fn check_brew_environment(program: &Path, options: &NodeScanOptions) -> Result<(), PlanError> {
    let prefix = program
        .parent()
        .and_then(Path::parent)
        .ok_or(PlanError::EvidenceUnavailable)?;
    for path in [
        PathBuf::from("/etc/homebrew/brew.env"),
        prefix.join("etc/homebrew/brew.env"),
        options.home.join(".homebrew/brew.env"),
    ] {
        let (fingerprint, _) = guards::configuration(&path)?;
        // Homebrew 会用这些文件覆盖进程环境；不静默忽略或改写用户策略。
        if fingerprint.exists
            && fingerprint.sha256.as_deref()
                != Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        {
            return Err(capability(
                "Homebrew 存在额外 brew.env，无法保证预览中的受控环境；当前保持手动管理",
            ));
        }
    }
    Ok(())
}
fn resolve(path: &Path) -> Result<PathBuf, PlanError> {
    if !path.is_absolute() {
        return Err(PlanError::EvidenceUnavailable);
    }
    if path.exists() {
        path.canonicalize()
            .map_err(|_| PlanError::EvidenceUnavailable)
    } else {
        let (f, _) = guards::configuration(path)?;
        f.canonical_path
            .map(PathBuf::from)
            .ok_or(PlanError::EvidenceUnavailable)
    }
}
fn receipt(options: &NodeScanOptions) -> Result<Option<String>, PlanError> {
    let path = options
        .mise_data_dir
        .join("installs/node/.mise.backend.toml");
    if !path
        .try_exists()
        .map_err(|_| PlanError::EvidenceUnavailable)?
    {
        if std::fs::symlink_metadata(path).is_ok() {
            return Err(PlanError::EvidenceUnavailable);
        }
        return Ok(None);
    }
    let mut open = OpenOptions::new();
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
    let mut data = Vec::new();
    file.take(8193)
        .read_to_end(&mut data)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    if data.len() > 8192 {
        return Err(PlanError::EvidenceUnavailable);
    }
    let value: toml::Value =
        toml::from_str(std::str::from_utf8(&data).map_err(|_| PlanError::EvidenceUnavailable)?)
            .map_err(|_| PlanError::EvidenceUnavailable)?;
    if value.get("full").and_then(toml::Value::as_str) != Some("core:node") {
        return Err(capability(
            "该 mise 目录未证明属于 core:node；不覆盖或删除其它后端的安装",
        ));
    }
    use sha2::{Digest, Sha256};
    Ok(Some(format!("{:x}", Sha256::digest(data))))
}
fn mise_facts(program: &Path, options: &NodeScanOptions) -> Result<Facts, PlanError> {
    let help = output(program, ProviderId::Mise, options, &["--help"])?;
    if MISE_FLAGS.iter().any(|flag| !help.contains(flag)) {
        return Err(capability("当前 mise 不支持禁用配置、环境与钩子的受控调用"));
    }
    let mut args = MISE_FLAGS.to_vec();
    args.extend(["ls", "--installed", "--json", "core:node"]);
    let mut rows: Vec<MiseRow> =
        serde_json::from_str(&output(program, ProviderId::Mise, options, &args)?)
            .map_err(|_| capability("mise 安装记录格式不受支持"))?;
    if rows.len() > 128
        || rows.iter().any(|r| {
            !r.installed
                || r.version.len() > 128
                || r.install_path.len() > 4096
                || !Path::new(&r.install_path).is_absolute()
        })
    {
        return Err(capability("mise 安装记录不完整或超出范围"));
    }
    rows.sort_by(|a, b| (&a.version, &a.install_path).cmp(&(&b.version, &b.install_path)));
    Ok(Facts::Mise {
        rows,
        receipt: receipt(options)?,
        root: resolve(&options.mise_data_dir)?,
    })
}
pub(super) fn registered_nodes(
    program: &Path,
    options: &NodeScanOptions,
) -> Result<Vec<(String, String)>, PlanError> {
    let Facts::Mise { rows, .. } = mise_facts(program, options)? else {
        unreachable!()
    };
    Ok(rows
        .into_iter()
        .map(|r| (r.version, r.install_path))
        .collect())
}
fn list(text: String) -> Result<Vec<String>, PlanError> {
    let mut items: Vec<String> = text
        .lines()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if items.len() > 256
        || items.iter().any(|s| {
            s.len() > 128
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_@+.-".contains(&b))
        })
    {
        return Err(capability("Homebrew 依赖输出不完整或不受支持"));
    }
    items.sort();
    items.dedup();
    Ok(items)
}
fn brew_facts(
    program: &Path,
    options: &NodeScanOptions,
    package: &str,
    action: Action,
) -> Result<Facts, PlanError> {
    let prefix = PathBuf::from(output(program, ProviderId::Brew, options, &["--prefix"])?.trim());
    let cellar = PathBuf::from(output(program, ProviderId::Brew, options, &["--cellar"])?.trim());
    if !prefix.is_absolute()
        || !cellar.is_absolute()
        || resolve(&prefix.join("Cellar"))? != resolve(&cellar)?
        || !options
            .brew_cellars
            .iter()
            .any(|p| resolve(p).ok() == resolve(&cellar).ok())
    {
        return Err(capability("Homebrew 的真实 prefix/Cellar 与清单来源不一致"));
    }
    let value: Value = serde_json::from_str(&output(
        program,
        ProviderId::Brew,
        options,
        &["info", "--json=v2", "--formula", package],
    )?)
    .map_err(|_| capability("Homebrew 公式信息格式不受支持"))?;
    let formulas = value
        .get("formulae")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 1)
        .ok_or(capability("未获得唯一 Homebrew core 公式"))?;
    let f = &formulas[0];
    if f["name"].as_str() != Some(package)
        || f["tap"].as_str() != Some("homebrew/core")
        || f["disabled"].as_bool() != Some(false)
        || !matches!(f["full_name"].as_str(),Some(name) if native_brew_node_formula(name).as_deref()==Some(package))
    {
        return Err(capability("只支持已核对的 Homebrew core Node 公式"));
    }
    let version = f["versions"]["stable"]
        .as_str()
        .filter(|v| exact_version(v))
        .ok_or(capability("无法确认 Homebrew 当前稳定版本"))?
        .to_owned();
    let revision = f["revision"].as_u64().ok_or(capability("缺少公式修订号"))?;
    let rows = f["installed"]
        .as_array()
        .ok_or(capability("缺少 Homebrew 安装记录"))?;
    let mut installed = Vec::new();
    for row in rows {
        installed.push(
            row["version"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or(capability("Homebrew 安装版本未知"))?
                .to_owned(),
        );
    }
    installed.sort();
    let dependencies = list(output(
        program,
        ProviderId::Brew,
        options,
        &[
            "deps",
            "--formula",
            "--full-name",
            "--include-build",
            package,
        ],
    )?)?;
    let dependents = if action == Action::Uninstall {
        list(output(
            program,
            ProviderId::Brew,
            options,
            &[
                "uses",
                "--installed",
                "--recursive",
                "--include-build",
                "--include-test",
                "--include-optional",
                package,
            ],
        )?)?
    } else {
        vec![]
    };
    use sha2::{Digest, Sha256};
    let fingerprint=format!("{:x}",Sha256::digest(serde_json::to_vec(&serde_json::json!({"name":f["name"],"full_name":f["full_name"],"tap":f["tap"],"versions":f["versions"],"revision":f["revision"],"installed":f["installed"],"dependencies":f["dependencies"],"linked_keg":f["linked_keg"],"keg_only":f["keg_only"]})).map_err(|_|PlanError::EvidenceUnavailable)?));
    Ok(Facts::Brew {
        prefix,
        cellar,
        version,
        revision,
        installed,
        dependents,
        dependencies,
        fingerprint,
    })
}
pub(super) fn prepare(
    action: Action,
    package: &ProviderPackage,
    target: Option<&Installation>,
    version: Option<&str>,
    snapshot: &InventorySnapshot,
    options: &NodeScanOptions,
) -> Result<WriteSpec, PlanError> {
    if options.platform != Platform::Macos || !matches!(action, Action::Install | Action::Uninstall)
    {
        return Err(PlanError::Unsupported);
    }
    let provider = package.provider_id;
    if !matches!(provider, ProviderId::Mise | ProviderId::Brew) {
        return Err(capability(
            "此来源没有受支持的删除或安装能力；nvm 与 PATH 继续只读",
        ));
    }
    if !ready(snapshot, provider) {
        return Err(capability("来源情况未知或过期，请完整刷新后再预览写操作"));
    }
    if action == Action::Uninstall {
        let target = target.ok_or(PlanError::Unsupported)?;
        if !snapshot.installations.iter().any(|current| {
            current.installation_id == target.installation_id
                && !current.stale
                && current.health == Health::Healthy
                && current.ownership == target.ownership
        }) {
            return Err(capability(
                "所选实例的健康或归属证据已变化，请刷新后重新预览",
            ));
        }
    }
    let mut program = guards::executable(
        options,
        if provider == ProviderId::Mise {
            "mise"
        } else {
            "brew"
        },
    )
    .ok_or(capability("未找到可定位的管理器程序"))?;
    let mut impact = Vec::new();
    let (facts, version, locator, argv) = if provider == ProviderId::Mise {
        if package.package_id != "node" || package.kind != PackageKind::Runtime {
            return Err(capability(
                "仅开放原生 core:node，旧 nodejs 插件目录继续只读",
            ));
        }
        let version = version
            .or_else(|| target.and_then(|t| t.version.as_deref()))
            .filter(|v| exact_version(v))
            .ok_or(PlanError::Unsupported)?
            .to_owned();
        let locator = options.mise_data_dir.join("installs/node").join(&version);
        let facts = mise_facts(&program, options)?;
        let Facts::Mise { rows, receipt, .. } = &facts else {
            unreachable!()
        };
        let matches: Vec<_> = rows.iter().filter(|r| r.version == version).collect();
        if action == Action::Install {
            if !matches.is_empty() || std::fs::symlink_metadata(&locator).is_ok() {
                return Err(capability(
                    "该精确版本已有目录或管理器记录；请重新验证，不覆盖现有安装",
                ));
            }
        } else {
            let target = target.ok_or(PlanError::Unsupported)?;
            if Path::new(&target.locator) != locator {
                return Err(capability(
                    "所选为版本别名或外部目录，请选择精确版本目录的实例再预览卸载",
                ));
            }
            if receipt.is_none()
                || matches.len() != 1
                || std::fs::symlink_metadata(&locator)
                    .map_err(|_| PlanError::EvidenceUnavailable)?
                    .file_type()
                    .is_symlink()
                || resolve(Path::new(&matches[0].install_path))? != resolve(&locator)?
                || !resolve(Path::new(
                    target
                        .executable_path
                        .as_ref()
                        .ok_or(PlanError::EvidenceUnavailable)?,
                ))?
                .starts_with(resolve(&locator)?)
            {
                return Err(capability(
                    "未确认原生 mise 目录与管理器记录一致，不能卸载外部链接或未知归属实例",
                ));
            }
        }
        impact.push(format!(
            "仅作用于 core:node@{version}：{}",
            locator.display()
        ));
        impact.push("不加载配置或安装钩子，不运行 mise use；可能更新本来源的下载、缓存、状态与 shims；已有项目和终端不会自动切换。".into());
        let mut argv: Vec<String> = MISE_FLAGS.iter().map(|s| s.to_string()).collect();
        argv.extend([
            if action == Action::Install {
                "install"
            } else {
                "uninstall"
            }
            .into(),
            format!("core:node@{version}"),
        ]);
        (facts, version, locator, argv)
    } else {
        let native = native_brew_node_formula(&package.package_id).ok_or(PlanError::Unsupported)?;
        if let Some(target) = target {
            let dir = Path::new(&target.locator);
            let prefix = dir
                .ancestors()
                .nth(3)
                .ok_or(PlanError::EvidenceUnavailable)?;
            let candidate = prefix.join("bin/brew");
            if guards::file_stamp(&candidate).is_ok() {
                program = candidate;
            }
        }
        let facts = brew_facts(&program, options, &native, action)?;
        let Facts::Brew {
            prefix,
            cellar,
            version,
            revision,
            installed,
            dependents,
            dependencies,
            ..
        } = &facts
        else {
            unreachable!()
        };
        let keg_version = if *revision == 0 {
            version.clone()
        } else {
            format!("{version}_{revision}")
        };
        let locator = if action == Action::Install {
            if !installed.is_empty()
                || snapshot.installations.iter().any(|i| {
                    i.provider_package.provider_id == provider
                        && i.provider_package.package_id == native
                        && Path::new(&i.locator).starts_with(cellar)
                })
            {
                return Err(capability(
                    "该 formula 已有安装，不通过安装动作隐式升级或覆盖",
                ));
            }
            cellar.join(&native).join(&keg_version)
        } else {
            let target = target.ok_or(PlanError::Unsupported)?;
            if target.ownership != Ownership::Owned
                || target.health != Health::Healthy
                || target.stale
                || installed.len() != 1
                || !dependents.is_empty()
            {
                return Err(capability(
                    "卸载需要唯一已确认归属实例且没有已安装依赖者；当前影响未满足条件",
                ));
            }
            let expected = cellar.join(&native).join(&installed[0]);
            if Path::new(&target.locator) != expected
                || std::fs::symlink_metadata(&expected)
                    .map_err(|_| PlanError::EvidenceUnavailable)?
                    .file_type()
                    .is_symlink()
            {
                return Err(capability(
                    "Homebrew 卸载目标不匹配，不能移除其它目录或多版本安装",
                ));
            }
            expected
        };
        impact.push(format!(
            "固定 Homebrew prefix：{}；Cellar：{}",
            prefix.display(),
            cellar.display()
        ));
        if action == Action::Install {
            impact.push(format!("该公式与构建依赖范围：{}。依赖可能安装或升级并创建链接，目标 Node 不 link、不执行 post-install。",dependencies.join("、")));
        } else {
            impact.push(format!("已确认没有已安装的依赖者；只移除当前唯一 keg。{} 与 {} 的相关链接可能失效，引用此来源的项目不会自动改写。",prefix.join("opt").join(&native).display(),prefix.join("bin/node").display()));
        }
        impact.push(
            "禁用自动更新、清理、自动移除与已安装依赖者检查；不使用 force，不自动删除共享依赖。"
                .into(),
        );
        let argv = if action == Action::Install {
            vec![
                "install".into(),
                "--formula".into(),
                "--skip-link".into(),
                "--skip-post-install".into(),
                native,
            ]
        } else {
            vec!["uninstall".into(), "--formula".into(), native]
        };
        let promised_version = if action == Action::Uninstall {
            target
                .and_then(|t| t.version.clone())
                .ok_or(PlanError::EvidenceUnavailable)?
        } else {
            version.clone()
        };
        (facts.clone(), promised_version, locator, argv)
    };
    Ok(WriteSpec {
        action,
        provider,
        program,
        argv,
        package: package.package_id.clone(),
        version,
        locator,
        impact,
        facts,
    })
}
impl WriteSpec {
    pub fn revalidate(
        &self,
        snapshot: &InventorySnapshot,
        target: Option<&Installation>,
        options: &NodeScanOptions,
    ) -> Result<(), PlanError> {
        let package = ProviderPackage {
            provider_id: self.provider,
            package_id: self.package.clone(),
            kind: if self.provider == ProviderId::Brew {
                PackageKind::Formula
            } else {
                PackageKind::Runtime
            },
            platform: options.platform,
        };
        let next = prepare(
            self.action,
            &package,
            target,
            (self.provider == ProviderId::Mise).then_some(self.version.as_str()),
            snapshot,
            options,
        )?;
        if next.facts != self.facts
            || next.program != self.program
            || next.argv != self.argv
            || next.locator != self.locator
        {
            return Err(PlanError::Preconditions);
        }
        Ok(())
    }
    pub fn run(
        &self,
        options: &NodeScanOptions,
        cancellation: &CancellationToken,
    ) -> Result<(), PlanError> {
        self.run_limited(
            options,
            cancellation,
            &ProbeLimits {
                timeout: Duration::from_secs(900),
                max_output_bytes: 512 * 1024,
            },
        )
    }
    pub(super) fn run_limited(
        &self,
        options: &NodeScanOptions,
        cancellation: &CancellationToken,
        limits: &ProbeLimits,
    ) -> Result<(), PlanError> {
        if self.provider == ProviderId::Brew {
            check_brew_environment(&self.program, options)?;
        }
        let mut cmd = command(&self.program, self.provider, options);
        cmd.args(&self.argv);
        let result = run_probe(&mut cmd, limits, cancellation).map_err(|e| {
            if matches!(e, ProbeError::Cancelled) {
                PlanError::Cancelled
            } else {
                PlanError::Command
            }
        })?;
        if result.status.success() {
            Ok(())
        } else {
            Err(PlanError::Command)
        }
    }
    pub fn verify(
        &self,
        snapshot: &InventorySnapshot,
        options: &NodeScanOptions,
    ) -> Result<(), PlanError> {
        if !ready(snapshot, self.provider) {
            return Err(PlanError::ScanIncomplete);
        }
        if self.action == Action::Install {
            if snapshot.installations.iter().any(|i| {
                i.provider_package.provider_id == self.provider
                    && i.provider_package.package_id == self.package
                    && !i.stale
                    && i.health == Health::Healthy
                    && i.version.as_deref() == Some(self.version.as_str())
                    && Path::new(&i.locator) == self.locator
            }) {
                Ok(())
            } else {
                Err(PlanError::Verification)
            }
        } else {
            if std::fs::symlink_metadata(&self.locator).is_ok()
                || snapshot.installations.iter().any(|i| {
                    i.provider_package.provider_id == self.provider
                        && Path::new(&i.locator) == self.locator
                })
            {
                return Err(PlanError::Verification);
            }
            let facts = if self.provider == ProviderId::Mise {
                mise_facts(&self.program, options)?
            } else {
                brew_facts(&self.program, options, &self.package, self.action)?
            };
            let absent = match facts {
                Facts::Mise { rows, .. } => !rows.iter().any(|r| r.version == self.version),
                Facts::Brew { installed, .. } => installed.is_empty(),
            };
            if absent {
                Ok(())
            } else {
                Err(PlanError::Verification)
            }
        }
    }
}
