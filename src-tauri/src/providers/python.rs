//! macOS CPython 3 只读来源；不调用管理器、不加载 shell、site 或项目配置。
use super::*;
use crate::domain::{native_brew_python_formula, PackageKind};
#[derive(Clone, Debug)]
pub struct PythonScanOptions {
    pub common: NodeScanOptions,
    pub pyenv_root: PathBuf,
    pub uv_python_dir: PathBuf,
}
impl PythonScanOptions {
    pub fn from_environment() -> io::Result<Self> {
        let common = NodeScanOptions::from_environment()?;
        let pyenv_root = std::env::var_os("PYENV_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| common.home.join(".pyenv"));
        let uv_python_dir = std::env::var_os("UV_PYTHON_INSTALL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| common.home.join(".local/share"))
                    .join("uv/python")
            });
        Ok(Self {
            common,
            pyenv_root,
            uv_python_dir,
        })
    }
}
pub const PROVIDERS: [ProviderId; 5] = [
    ProviderId::Mise,
    ProviderId::Brew,
    ProviderId::Pyenv,
    ProviderId::Uv,
    ProviderId::Path,
];
pub(crate) fn stable(raw: &str) -> bool {
    let parts: Vec<_> = raw.split('.').collect();
    parts.len() == 3
        && parts[0] == "3"
        && parts[1].parse::<u32>().is_ok_and(|n| (8..100).contains(&n))
        && parts.iter().all(|n| {
            !n.is_empty()
                && (n.len() == 1 || !n.starts_with('0'))
                && n.len() <= 4
                && n.bytes().all(|b| b.is_ascii_digit())
        })
}
fn evidence(kind: &str, path: &Path, detail: &str) -> ObservationEvidence {
    ObservationEvidence {
        kind: kind.into(),
        path: Some(path.to_string_lossy().into()),
        detail: Some(detail.into()),
    }
}
fn root(run: &mut ScanRun<'_>, path: &Path) -> bool {
    if !run.check() {
        return false;
    }
    if !path.is_absolute() {
        run.problem(
            ProviderStatus::Error,
            "relativeProviderRoot",
            "Python 来源根目录必须是绝对路径",
        );
        return false;
    }
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => {
            run.mark_available();
            true
        }
        Ok(_) => {
            run.problem(
                ProviderStatus::Error,
                "invalidProviderRoot",
                "Python 来源根路径不是目录",
            );
            false
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => {
            run.io_problem("pythonRootMetadata", &e);
            false
        }
    }
}
fn installation(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() || m.file_type().is_symlink())
}
fn venv(run: &mut ScanRun<'_>, path: &Path) -> bool {
    match fs::symlink_metadata(path.join("pyvenv.cfg")) {
        Ok(_) => true,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            false
        }
        Err(e) => {
            run.io_problem("virtualEnvironmentMetadata", &e);
            true
        }
    }
}
fn executable(run: &mut ScanRun<'_>, path: &Path) -> PathBuf {
    let bin = path.join("bin");
    for name in ["python3", "python"] {
        let p = bin.join(name);
        if fs::symlink_metadata(&p).is_ok() {
            return p;
        }
    }
    let mut versions = run
        .read_dir(&bin)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            let minor = name.strip_prefix("python3.")?;
            if minor.is_empty() || !minor.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((minor.parse::<u32>().ok()?, p))
        })
        .collect::<Vec<_>>();
    versions.sort_by_key(|a| std::cmp::Reverse(a.0));
    versions
        .into_iter()
        .next()
        .map(|(_, p)| p)
        .unwrap_or_else(|| bin.join("python3"))
}
/// mise 自动维护的标准别名只做文件观测，不执行，也不改变精确安装的归属。
fn known_mise_alias(base: &Path, locator: &Path, name: &str) -> bool {
    let Ok(link) = fs::read_link(locator) else {
        return false;
    };
    let mut parts = link
        .components()
        .filter(|p| *p != std::path::Component::CurDir);
    let Some(std::path::Component::Normal(version)) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    let Some(target) = version.to_str().filter(|v| stable(v)) else {
        return false;
    };
    let minor = target.rsplit_once('.').map(|(v, _)| v).unwrap_or("");
    if name != "latest" && name != "3" && name != minor {
        return false;
    }
    let exact = base.join(target);
    fs::symlink_metadata(&exact).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        && locator
            .canonicalize()
            .ok()
            .zip(exact.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}
/// uv 只支持 macOS 的 CPython 命名布局；写渠道使用同一套目录名，避免扫描与执行漂移。
pub(crate) fn uv_arch() -> Option<&'static str> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    match std::env::consts::ARCH {
        "aarch64" => Some("aarch64"),
        "x86_64" => Some("x86_64"),
        _ => None,
    }
}
pub(crate) fn uv_dir_name(version: &str) -> Option<String> {
    uv_arch().map(|arch| format!("cpython-{version}-macos-{arch}-none"))
}
/// uv 维护的次版本别名（cpython-3.12-macos-aarch64-none）只做文件观测；
/// 目标必须是根目录内的精确补丁版目录，且次版本一致，才不把来源判为未知。
fn known_uv_alias(base: &Path, locator: &Path, name: &str) -> bool {
    let Some(rest) = name.strip_prefix("cpython-") else {
        return false;
    };
    let fields: Vec<_> = rest.split('-').collect();
    if fields.len() != 4 {
        return false;
    }
    let (minor, os, arch, libc) = (fields[0], fields[1], fields[2], fields[3]);
    if os != "macos" || Some(arch) != uv_arch() || libc != "none" {
        return false;
    }
    let mut parts = minor.split('.');
    if parts.next() != Some("3") || parts.next().is_none() || parts.next().is_some() {
        return false;
    }
    let Ok(target) = locator.canonicalize() else {
        return false;
    };
    let Some(target_name) = target.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let Some(target_rest) = target_name.strip_prefix("cpython-") else {
        return false;
    };
    let target_fields: Vec<_> = target_rest.split('-').collect();
    if target_fields.len() != 4 {
        return false;
    }
    let (version, target_os, target_arch, target_libc) = (
        target_fields[0],
        target_fields[1],
        target_fields[2],
        target_fields[3],
    );
    if target_os != os || target_arch != arch || target_libc != libc {
        return false;
    }
    if !stable(version) || !version.starts_with(&format!("{minor}.")) {
        return false;
    }
    let Ok(canonical_base) = base.canonicalize() else {
        return false;
    };
    target.parent() == Some(canonical_base.as_path())
        && fs::symlink_metadata(&target).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
fn collect_directories(run: &mut ScanRun<'_>, base: &Path, uv: bool) {
    if !root(run, base) {
        return;
    }
    let Some(dirs) = run.read_dir(base) else {
        return;
    };
    for locator in dirs {
        if !run.check() {
            return;
        }
        if !installation(&locator) || venv(run, &locator) {
            continue;
        }
        let name = locator.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') {
            continue;
        }
        if uv && !name.starts_with("cpython-") {
            run.problem(
                ProviderStatus::Unsupported,
                "pythonVariantUnsupported",
                "部分 uv Python 实现或布局尚未支持",
            );
            continue;
        }
        let version = if uv {
            name.strip_prefix("cpython-")
                .and_then(|n| n.split('-').next())
        } else {
            Some(name)
        };
        let trusted =
            version.is_some_and(stable) && (!uv || name.split('-').nth(2) == Some("macos"));
        let known_alias = if uv {
            known_uv_alias(base, &locator, name)
        } else {
            run.provider == ProviderId::Mise && known_mise_alias(base, &locator, name)
        };
        if !trusted && !known_alias {
            run.problem(
                ProviderStatus::Unsupported,
                "pythonVariantUnsupported",
                "部分 Python 版本别名或构建变体保持未验证",
            );
        }
        let executable = executable(run, &locator);
        run.observe(Candidate {
            provider_package: ProviderPackage {
                provider_id: run.provider,
                package_id: "python".into(),
                kind: PackageKind::Runtime,
                platform: run.options.platform,
            },
            executable,
            declared_version: version.map(str::to_owned),
            ownership: Ownership::Observed,
            evidence: vec![evidence(
                "pythonInstallDirectory",
                &locator,
                "仅观察来源目录，不授予删除或接管权限",
            )],
            locator,
            trusted,
        });
    }
}
fn collect_brew(run: &mut ScanRun<'_>) {
    for cellar in run.options.brew_cellars.clone() {
        if !root(run, &cellar) {
            continue;
        }
        let Some(formulas) = run.read_dir(&cellar) else {
            continue;
        };
        for formula in formulas {
            let name = formula.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if native_brew_python_formula(name).is_none() {
                continue;
            }
            let Some(kegs) = run.read_dir(&formula) else {
                continue;
            };
            for locator in kegs {
                if !run.check() {
                    return;
                }
                if !installation(&locator) || venv(run, &locator) {
                    continue;
                }
                let receipt = locator.join("INSTALL_RECEIPT.json");
                let owned = run.read_metadata(&receipt).is_some_and(|bytes| {
                    matches!(
                        super::managed::receipt_origin(&bytes, name),
                        Ok(super::managed::ReceiptOrigin::Core)
                    )
                });
                if !owned {
                    run.problem(
                        ProviderStatus::Error,
                        "pythonBrewReceipt",
                        "Python core 安装收据缺失、无效或属于其他 tap，未执行来源程序",
                    );
                }
                let raw = locator.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let version = raw.split('_').next().unwrap_or(raw);
                let trusted = owned && stable(version);
                if owned && !trusted {
                    run.problem(
                        ProviderStatus::Unsupported,
                        "pythonVariantUnsupported",
                        "Homebrew Python 构建版本尚未支持",
                    );
                }
                let executable = executable(run, &locator);
                run.observe(Candidate {
                    provider_package: ProviderPackage {
                        provider_id: ProviderId::Brew,
                        package_id: name.into(),
                        kind: PackageKind::Formula,
                        platform: run.options.platform,
                    },
                    executable,
                    declared_version: Some(version.into()),
                    ownership: if owned {
                        Ownership::Owned
                    } else {
                        Ownership::Unknown
                    },
                    evidence: vec![evidence(
                        "pythonBrewReceipt",
                        &receipt,
                        if owned {
                            "homebrew/core 收据；操作权限仍须单独核对"
                        } else {
                            "归属未验证，不自动执行"
                        },
                    )],
                    locator,
                    trusted,
                });
            }
        }
    }
}
fn collect_path(run: &mut ScanRun<'_>) {
    run.mark_available();
    for directory in run.options.path_entries.clone() {
        if !run.visit_entry() {
            return;
        }
        if !directory.is_absolute() {
            run.problem(
                ProviderStatus::Error,
                "relativePathEntry",
                "PATH 含相对或空目录，已跳过",
            );
            continue;
        }
        // 不执行 shims、系统 Python 或任意 PATH 文件；版本名只用于识别文件候选。
        let mut names = vec![directory.join("python"), directory.join("python3")];
        if let Some(entries) = run.read_dir(&directory) {
            names.extend(entries.into_iter().filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix("python3."))
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            }));
        }
        names.sort();
        names.dedup();
        for path in names {
            if !run.check() {
                return;
            }
            match fs::symlink_metadata(&path) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    run.io_problem("pythonPathMetadata", &e);
                    continue;
                }
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            run.observe(Candidate {
                provider_package: ProviderPackage {
                    provider_id: ProviderId::Path,
                    package_id: name,
                    kind: PackageKind::Executable,
                    platform: run.options.platform,
                },
                locator: path.clone(),
                executable: path.clone(),
                declared_version: None,
                ownership: Ownership::Observed,
                evidence: vec![evidence(
                    "pythonPathFile",
                    &path,
                    "PATH 文件观测，未执行；不代表用户终端已选用",
                )],
                trusted: false,
            });
        }
    }
}
pub fn scan_python_provider(
    provider: ProviderId,
    options: &PythonScanOptions,
    token: &CancellationToken,
) -> ProviderScan {
    let mut run = ScanRun::new(provider, &options.common, token);
    run.tool = ToolId::from("python");
    if !run.check() {
        return run.finish();
    }
    if options.common.platform != Platform::Macos || !cfg!(unix) {
        run.problem(
            ProviderStatus::Unsupported,
            "platformUnsupported",
            "当前仅提供 macOS CPython 3 来源扫描",
        );
    } else if !options.common.home.is_absolute() {
        run.problem(
            ProviderStatus::Error,
            "relativeHome",
            "探针主目录必须为绝对路径",
        );
    } else {
        match provider {
            ProviderId::Mise => collect_directories(
                &mut run,
                &options.common.mise_data_dir.join("installs/python"),
                false,
            ),
            ProviderId::Pyenv => {
                if root(&mut run, &options.pyenv_root) {
                    collect_directories(&mut run, &options.pyenv_root.join("versions"), false);
                }
            }
            ProviderId::Uv => collect_directories(&mut run, &options.uv_python_dir, true),
            ProviderId::Brew => collect_brew(&mut run),
            ProviderId::Path => collect_path(&mut run),
            _ => run.problem(
                ProviderStatus::Unsupported,
                "providerUnsupported",
                "此来源不提供 Python 扫描",
            ),
        }
    }
    run.finish()
}
pub fn scan_python_sources(
    options: &PythonScanOptions,
    token: &CancellationToken,
) -> Vec<ProviderScan> {
    std::thread::scope(|scope| {
        PROVIDERS
            .into_iter()
            .map(|provider| {
                (
                    provider,
                    scope.spawn(move || scan_python_provider(provider, options, token)),
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(provider, worker)| {
                worker.join().unwrap_or_else(|_| {
                    let mut run = ScanRun::new(provider, &options.common, token);
                    run.tool = ToolId::from("python");
                    run.problem(
                        ProviderStatus::Error,
                        "sourcePanicked",
                        "Python 来源扫描异常中断",
                    );
                    run.finish()
                })
            })
            .collect()
    })
}
#[derive(Eq, PartialEq)]
struct Stamp {
    length: u64,
    modified: Option<SystemTime>,
    canonical: PathBuf,
    #[cfg(unix)]
    identity: (u64, u64, u32),
}
fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    if !m.is_file() {
        return None;
    }
    Some(Stamp {
        length: m.len(),
        modified: m.modified().ok(),
        canonical: path.canonicalize().ok()?,
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (m.dev(), m.ino(), m.mode())
        },
    })
}
pub(super) fn probe(run: &mut ScanRun<'_>, observation: &mut InstallationObservation) {
    let executable = observation.canonical_executable_path.as_ref().unwrap();
    let path = Path::new(executable);
    let before = stamp(path);
    let expression="import sys,json,platform,sysconfig;print(json.dumps({'version':'.'.join(map(str,sys.version_info[:3])),'implementation':sys.implementation.name,'releaseLevel':sys.version_info.releaselevel,'freeThreaded':bool(sysconfig.get_config_var('Py_GIL_DISABLED')),'debug':bool(sysconfig.get_config_var('Py_DEBUG')),'architecture':platform.machine(),'execPath':sys.executable}))";
    let mut command = Command::new(path);
    command
        .args(["-I", "-S", "-B", "-c", expression])
        .env_clear()
        .env("HOME", &run.options.home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .current_dir(&run.options.home);
    let limits = ProbeLimits {
        timeout: run.options.limits.probe_timeout.min(
            run.options
                .limits
                .provider_timeout
                .saturating_sub(run.started.elapsed()),
        ),
        max_output_bytes: run.options.limits.max_output_bytes,
    };
    let result = run_probe(&mut command, &limits, run.cancellation);
    match result {
        Ok(output) if output.status.success() => {
            let data = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
            let valid = data.as_ref().and_then(|v| {
                if v["implementation"].as_str() != Some("cpython")
                    || v["releaseLevel"].as_str() != Some("final")
                    || v["freeThreaded"].as_bool() != Some(false)
                    || v["debug"].as_bool() != Some(false)
                {
                    return None;
                }
                let version = v["version"].as_str().filter(|s| stable(s))?;
                let arch = match v["architecture"].as_str()? {
                    "aarch64" | "arm64" => "arm64",
                    "x86_64" | "AMD64" => "x86_64",
                    _ => return None,
                };
                let actual = Path::new(v["execPath"].as_str()?);
                if !actual.is_absolute()
                    || actual.canonicalize().ok()
                        != observation
                            .canonical_executable_path
                            .as_ref()
                            .map(PathBuf::from)
                {
                    return None;
                }
                Some((version.to_owned(), arch.to_owned()))
            });
            if let Some((version, architecture)) = valid.filter(|_| {
                before.is_some()
                    && stamp(path) == before
                    && observation
                        .executable_path
                        .as_deref()
                        .and_then(|p| Path::new(p).canonicalize().ok())
                        == Some(path.to_path_buf())
            }) {
                let matches = observation
                    .evidence
                    .iter()
                    .find(|e| e.kind == "declaredVersion")
                    .and_then(|e| e.detail.as_deref())
                    .is_none_or(|v| v == version);
                observation.version = Some(version);
                observation.architecture = Some(architecture);
                observation.health = if matches {
                    Health::Healthy
                } else {
                    Health::Broken
                };
                if !matches {
                    run.problem(
                        ProviderStatus::Error,
                        "pythonVersionMismatch",
                        "实际 Python 版本与来源目录声明不一致",
                    );
                }
                run.add_evidence(
                    observation,
                    "pythonIsolatedProbe",
                    "CPython -I -S -B 探针已核对版本、架构与真实文件；不是终端或项目环境验证",
                );
            } else {
                observation.health = Health::Unverified;
                run.problem(
                    ProviderStatus::Error,
                    "invalidPythonProbe",
                    "Python 探针实现、版本、架构、文件身份或输出无法核对",
                );
                run.add_evidence(
                    observation,
                    "invalidPythonProbe",
                    "未能验证为稳定 CPython 3，保持未知",
                );
            }
        }
        result => {
            let (status, code) = match result {
                Err(ProbeError::Cancelled) => (ProviderStatus::Cancelled, "probeCancelled"),
                Err(ProbeError::Timeout) => (ProviderStatus::Timeout, "probeTimeout"),
                Err(ProbeError::OutputLimit) => {
                    run.truncated = true;
                    (ProviderStatus::Error, "probeOutputLimit")
                }
                _ => (ProviderStatus::Error, "pythonProbeFailed"),
            };
            run.problem(status, code, "Python 探针失败、超时、取消或超出输出预算");
            run.add_evidence(
                observation,
                code,
                "无法确认 Python 版本；保留来源记录，不写入原始输出",
            );
        }
    }
}
#[cfg(all(test, unix))]
mod tests;
