// 托管接入层：把系统包管理器（brew 等）安装的运行时接入 mise，并长期保持健康。
//
// 三种策略：
//  A. 直连接管：nvm/pyenv/asdf 等用户级稳定目录，直接 mise link；
//  B. 托管接管：brew 等“目录生命周期归包管理器”的来源，软链指向应用自有的农场目录
//     （~/.zenv/managed/<tool>/<version> → 真实目录），mise 只认识农场路径；
//     目标目录被 brew 升级/删除时，只修复同软件同版本，不静默升级或删除绑定；
//  C. PATH 绑定：不建立任何链接，向项目 mise.toml 写入 [env] _.path（stable_bin_path
//     返回 brew opt / scoop current 这类包管理器自维护的稳定路径），由前端完成写入。
//
// 所有链接动作完成后都通过 `mise ls` 校验注册结果，失败自动回滚。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config;
use crate::error::AppError;
use crate::mise::{detect_tool_sources, run_mise};
use crate::sources::normalize_version;

/// 农场与清单的存放位置：~/.zenv/managed/
fn managed_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".zenv/managed"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn farm_path(tool: &str, version: &str) -> Result<PathBuf, AppError> {
    if [tool, version].iter().any(|s| {
        s.is_empty()
            || *s == "."
            || *s == ".."
            || !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    }) {
        return Err(AppError::Unsupported("托管目录标识无效".into()));
    }
    let root = managed_root().ok_or_else(|| AppError::Other("无法定位用户主目录".into()))?;
    Ok(root.join(tool).join(version))
}

/// 托管清单条目：农场里的一个接入记录。
#[derive(Serialize, Deserialize, Clone)]
pub struct ManagedEntry {
    pub tool: String,
    pub version: String,
    pub manager: String,
    /// 软链指向的真实目录（包管理器侧）
    pub target: String,
}

/// 对账事件：reconcile 的一次自愈/清理动作，供前端展示。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileEvent {
    pub tool: String,
    pub from: String,
    pub to: Option<String>,
    /// relinked = 同版本重连；needs-review = 精确版本不可用；failed = 修复失败
    pub action: String,
    pub message: String,
}

fn manifest_path() -> Result<PathBuf, AppError> {
    Ok(managed_root()
        .ok_or_else(|| AppError::Other("无法定位用户主目录".into()))?
        .join("manifest.json"))
}

fn load_manifest_at(path: &Path) -> Result<Vec<ManagedEntry>, AppError> {
    let Some(content) = config::read_optional(path)? else {
        return Ok(Vec::new());
    };
    serde_json::from_str(&content)
        .map_err(|e| AppError::Parse(format!("托管清单损坏，已停止修改，请检查备份：{e}")))
}

fn save_manifest_at(path: &Path, entries: &[ManagedEntry]) -> Result<(), AppError> {
    let json = serde_json::to_vec_pretty(entries).map_err(|e| AppError::Parse(e.to_string()))?;
    config::atomic_write(path, &json)
}

fn load_manifest() -> Result<Vec<ManagedEntry>, AppError> {
    load_manifest_at(&manifest_path()?)
}
fn save_manifest(entries: &[ManagedEntry]) -> Result<(), AppError> {
    save_manifest_at(&manifest_path()?, entries)
}

/// 创建目录软链（Windows 需开发者模式；brew 仅存在于 unix，此路径不会在 Windows 触发）。
pub(crate) fn create_symlink(target: &Path, link: &Path) -> Result<(), AppError> {
    if let Ok(old) = std::fs::read_link(link) {
        if old == target {
            return Ok(());
        }
        return Err(AppError::Other("托管路径已有其他链接，已停止覆盖".into()));
    }
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, link);
    result.map_err(|e| {
        AppError::Io(format!(
            "创建软链 {} -> {} 失败: {}",
            link.display(),
            target.display(),
            e
        ))
    })
}

/// brew formula 的真实安装根布局修正：openjdk 系列的可执行文件在 Cellar 目录更深处，
/// 直接链接 Cellar 版本目录不满足 mise java 插件的布局要求。
fn resolve_brew_layout(dir: &Path) -> PathBuf {
    let is_openjdk = dir
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with("openjdk"));
    if cfg!(target_os = "macos") && is_openjdk {
        let inner = dir.join("libexec/openjdk.jdk/Contents/Home");
        if inner.exists() {
            return inner;
        }
    }
    dir.to_path_buf()
}

/// 校验 mise 已注册该版本（`mise ls <tool> --json` 中出现），失败时由调用方回滚。
fn verify_registered(tool: &str, version: &str) -> Result<(), AppError> {
    let out = run_mise(&["ls", tool, "--json"])?;
    let vs: Vec<serde_json::Value> =
        serde_json::from_str(&out).map_err(|e| AppError::Parse(e.to_string()))?;
    let ok = vs.iter().any(|v| {
        v.get("version").and_then(|x| x.as_str()) == Some(version)
            && v["installed"].as_bool() == Some(true)
    });
    if ok {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "mise 未注册 {tool}@{version}：目标目录布局可能不满足该工具插件的要求"
        )))
    }
}

/// 策略 B：建立农场软链并让 mise 指向农场路径；任一步失败即回滚。
fn adopt_farm(tool: &str, version: &str, real: &Path) -> Result<(), AppError> {
    let farm = farm_path(tool, version)?;
    if let Some(dir) = farm.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| AppError::Io(format!("创建农场目录失败: {}", e)))?;
    }
    create_symlink(real, &farm)?;
    let spec = format!("{}@{}", tool, version);
    if let Err(e) = run_mise(&["link", &spec, &farm.to_string_lossy()]) {
        let _ = std::fs::remove_file(&farm);
        return Err(e);
    }
    if let Err(e) = verify_registered(tool, version) {
        let _ = crate::operations::without_cancellation(|| run_mise(&["uninstall", &spec]));
        let _ = std::fs::remove_file(&farm);
        return Err(e);
    }
    Ok(())
}

/// 策略 A：直连接管（用户级稳定目录），同样带注册校验。
fn adopt_direct(tool: &str, version: &str, real: &Path) -> Result<(), AppError> {
    let spec = format!("{}@{}", tool, version);
    run_mise(&["link", &spec, &real.to_string_lossy()])?;
    if let Err(e) = verify_registered(tool, version) {
        let _ = crate::operations::without_cancellation(|| run_mise(&["uninstall", &spec]));
        return Err(e);
    }
    Ok(())
}

/// 只从目标目录执行版本探针，绝不借用 PATH 上另一个运行时冒充验证成功。
fn verify_runtime(tool: &str, version: &str, real: &Path) -> Result<(), AppError> {
    let binary = match tool {
        "node" => "node",
        "python" => "python3",
        "java" => "java",
        "go" => "go",
        "ruby" => "ruby",
        "rust" => "rustc",
        "php" => "php",
        "bun" => "bun",
        "deno" => "deno",
        "dotnet" => "dotnet",
        _ => {
            return Err(AppError::Unsupported(format!(
                "{tool} 暂无运行验证规则，请使用项目 PATH 绑定"
            )))
        }
    };
    let mut candidates = vec![
        real.join("bin").join(binary),
        real.join(binary),
        real.join(format!("{binary}.exe")),
        real.join("bin").join(format!("{binary}.exe")),
    ];
    if tool == "python" {
        candidates.extend([real.join("bin/python"), real.join("python.exe")]);
    }
    let bin = candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| AppError::Other(format!("目标目录缺少 {binary} 可执行文件")))?;
    let arg = if tool == "java" {
        "-version"
    } else if tool == "go" {
        "version"
    } else {
        "--version"
    };
    let out = crate::process::output(
        std::process::Command::new(bin).arg(arg),
        std::time::Duration::from_secs(10),
    )?;
    if !out.status.success() {
        return Err(AppError::Other("目标运行时无法执行版本探针".into()));
    }
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: Vec<_> = normalize_version(version.split('_').next().unwrap_or(version))
        .into_iter()
        .take(3)
        .collect();
    if !expected.is_empty()
        && !text.split_whitespace().any(|s| {
            normalize_version(s)
                .into_iter()
                .take(expected.len())
                .collect::<Vec<_>>()
                == expected
        })
    {
        return Err(AppError::Other(format!(
            "目标运行时的实际版本与 {version} 不符"
        )));
    }
    Ok(())
}

fn registered_path(tool: &str, version: &str) -> Result<Option<PathBuf>, AppError> {
    let out = run_mise(&["ls", tool, "--json"])?;
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&out).map_err(|e| AppError::Parse(e.to_string()))?;
    Ok(rows
        .iter()
        .find(|v| v["version"].as_str() == Some(version))
        .and_then(|v| v["install_path"].as_str())
        .map(PathBuf::from))
}

pub fn adopt(tool: &str, version: &str, manager: &str, path: &str) -> Result<String, AppError> {
    if manager == "system" {
        return Err(AppError::Unsupported(
            "系统组件不可接管，请使用项目 PATH 绑定".into(),
        ));
    }
    let real = PathBuf::from(path);
    if !real.is_dir() {
        return Err(AppError::Other(format!("源目录不存在：{path}")));
    }
    farm_path(tool, version)?;
    let real = if manager == "brew" {
        resolve_brew_layout(&real)
    } else {
        real
    };
    verify_runtime(tool, version, &real)?;
    let mut entries = load_manifest()?;
    if let Some(existing) = registered_path(tool, version)? {
        if existing.canonicalize()? != real.canonicalize()? {
            return Err(AppError::Other("同版本已有其他安装，已停止覆盖".into()));
        }
        if !std::fs::symlink_metadata(&existing)?
            .file_type()
            .is_symlink()
        {
            return Err(AppError::Other(
                "该版本是独立安装，不应登记为外部接管".into(),
            ));
        }
        entries.retain(|e| !(e.tool == tool && e.version == version));
        entries.push(ManagedEntry {
            tool: tool.into(),
            version: version.into(),
            manager: manager.into(),
            target: real.display().to_string(),
        });
        save_manifest(&entries)?;
        return Ok(format!("{tool}@{version} 已接入且运行验证通过"));
    }
    if manager == "brew" {
        adopt_farm(tool, version, &real)?;
    } else {
        adopt_direct(tool, version, &real)?;
    }
    entries.retain(|e| !(e.tool == tool && e.version == version));
    entries.push(ManagedEntry {
        tool: tool.into(),
        version: version.into(),
        manager: manager.into(),
        target: real.display().to_string(),
    });
    if let Err(error) = save_manifest(&entries) {
        let rollback = crate::operations::without_cancellation(|| {
            run_mise(&["uninstall", &format!("{tool}@{version}")])
        });
        if manager == "brew" {
            let _ = std::fs::remove_file(farm_path(tool, version)?);
        }
        return Err(AppError::Other(format!(
            "清单保存失败：{error}；注册回滚{}",
            if rollback.is_ok() {
                "完成"
            } else {
                "失败，请检查运行时"
            }
        )));
    }
    Ok(format!("已接管 {tool}@{version}，目标运行验证通过"))
}

pub fn unadopt(tool: &str, version: &str) -> Result<String, AppError> {
    farm_path(tool, version)?;
    let old = load_manifest()?;
    let entry = old.iter().find(|e| e.tool == tool && e.version == version);
    let registered = registered_path(tool, version)?;
    let registration = registered.map(LinkState::capture).transpose()?;
    if registration.is_none() && entry.is_none() {
        return Err(AppError::Other("未发现该托管接入".into()));
    }
    let farm = LinkState::capture(farm_path(tool, version)?)?;
    let kept: Vec<_> = old
        .iter()
        .filter(|e| !(e.tool == tool && e.version == version))
        .cloned()
        .collect();
    let result = (|| {
        run_mise(&["uninstall", &format!("{tool}@{version}")])?;
        if farm.target.is_some() {
            std::fs::remove_file(&farm.path)?;
        }
        save_manifest(&kept)
    })();
    if let Err(error) = result {
        let rollback = (|| -> Result<(), AppError> {
            farm.restore()?;
            if let Some(registration) = registration {
                registration.restore()?;
            }
            Ok(())
        })();
        return Err(AppError::Other(match rollback {
            Ok(()) => format!("{error}；原接管链接已恢复"),
            Err(rollback) => format!("{error}；回滚失败：{rollback}，请检查托管清单和链接"),
        }));
    }
    Ok(format!("已解除接管 {tool}@{version}"))
}

pub fn list() -> Result<Vec<ManagedEntryHealthy>, AppError> {
    Ok(load_manifest()?
        .into_iter()
        .map(|e| {
            let healthy = healthy(&e);
            ManagedEntryHealthy {
                tool: e.tool,
                version: e.version,
                manager: e.manager,
                target: e.target,
                healthy,
            }
        })
        .collect())
}

fn healthy(entry: &ManagedEntry) -> bool {
    let target = Path::new(&entry.target);
    target.is_dir()
        && registered_path(&entry.tool, &entry.version)
            .ok()
            .flatten()
            .is_some_and(|path| path.canonicalize().ok() == target.canonicalize().ok())
        && verify_runtime(&entry.tool, &entry.version, target).is_ok()
}

/// 捕获链接状态；普通目录绝不参与修复或回滚，避免删除独立安装。
struct LinkState {
    path: PathBuf,
    target: Option<PathBuf>,
}
impl LinkState {
    fn capture(path: PathBuf) -> Result<Self, AppError> {
        let target = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => Some(std::fs::read_link(&path)?),
            Ok(_) => {
                return Err(AppError::Other(format!(
                    "{} 不是接管链接，已停止覆盖",
                    path.display()
                )))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, target })
    }
    fn restore(&self) -> Result<(), AppError> {
        match &self.target {
            Some(target) => replace_symlink(target, &self.path),
            None => match std::fs::symlink_metadata(&self.path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    std::fs::remove_file(&self.path)?;
                    Ok(())
                }
                Ok(_) => Err(AppError::Other(
                    "回滚路径已被替换为普通文件，请人工检查".into(),
                )),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            },
        }
    }
}

fn replace_symlink(target: &Path, path: &Path) -> Result<(), AppError> {
    let old = LinkState::capture(path.to_path_buf())?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Other("链接路径无父目录".into()))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::tempdir_in(parent)?;
    let next = staging.path().join("next");
    create_symlink(target, &next)?;
    #[cfg(windows)]
    if old.target.is_some() {
        std::fs::rename(path, staging.path().join("old"))?;
    }
    if let Err(error) = std::fs::rename(&next, path) {
        #[cfg(windows)]
        if old.target.is_some() {
            std::fs::rename(staging.path().join("old"), path)?;
        }
        return Err(error.into());
    }
    let _ = old;
    Ok(())
}

fn repair(
    entry: &ManagedEntry,
    real: &Path,
    commit: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    farm_path(&entry.tool, &entry.version)?;
    verify_runtime(&entry.tool, &entry.version, real)?;
    let registered = registered_path(&entry.tool, &entry.version)?;
    let mut links = Vec::new();
    if let Some(path) = registered.as_ref() {
        let old = LinkState::capture(path.clone())?;
        if let Some(target) = &old.target {
            let absolute = if target.is_absolute() {
                target.clone()
            } else {
                path.parent().unwrap().join(target)
            };
            let farm = farm_path(&entry.tool, &entry.version)?;
            if absolute != Path::new(&entry.target) && absolute != farm {
                return Err(AppError::Other(
                    "注册链接已指向其他来源，已停止自动修复".into(),
                ));
            }
        }
        links.push(old);
    }
    let destination = if entry.manager == "brew" {
        let farm = farm_path(&entry.tool, &entry.version)?;
        let old = LinkState::capture(farm.clone())?;
        if old
            .target
            .as_deref()
            .is_some_and(|target| target != Path::new(&entry.target))
        {
            return Err(AppError::Other(
                "农场链接已指向其他来源，已停止自动修复".into(),
            ));
        }
        links.push(old);
        farm
    } else {
        real.to_path_buf()
    };
    let result = (|| {
        if entry.manager == "brew" {
            replace_symlink(real, &destination)?;
        }
        if let Some(path) = registered.as_ref() {
            replace_symlink(&destination, path)?;
        } else {
            run_mise(&[
                "link",
                &format!("{}@{}", entry.tool, entry.version),
                &destination.to_string_lossy(),
            ])?;
        }
        verify_registered(&entry.tool, &entry.version)?;
        commit()
    })();
    if let Err(error) = result {
        let rollback = crate::operations::without_cancellation(|| -> Result<(), AppError> {
            if registered.is_none() {
                if let Some(path) = registered_path(&entry.tool, &entry.version)? {
                    let current = LinkState::capture(path.clone())?;
                    if current.target.as_deref() == Some(destination.as_path()) {
                        std::fs::remove_file(path)?;
                    } else {
                        return Err(AppError::Other("新注册链接目标不符，请人工检查".into()));
                    }
                }
            }
            for link in links.iter().rev() {
                link.restore()?;
            }
            Ok(())
        });
        return Err(AppError::Other(match rollback {
            Ok(()) => format!("{error}；原链接已恢复"),
            Err(rollback) => format!("{error}；链接回滚失败：{rollback}，请检查托管清单与链接"),
        }));
    }
    Ok(())
}

/// 包身份取自原始 Cellar 路径，不能把 node@20 与 node@22 当成同一软件。
fn formula(path: &str) -> Option<String> {
    let parts: Vec<_> = Path::new(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    parts
        .windows(2)
        .find(|w| w[0] == "Cellar")
        .map(|w| w[1].clone())
}

pub fn reconcile() -> Result<Vec<ReconcileEvent>, AppError> {
    let mut events = Vec::new();
    let mut entries = load_manifest()?;
    let sources = detect_tool_sources();
    for index in 0..entries.len() {
        let entry = entries[index].clone();
        if healthy(&entry) {
            continue;
        }
        let same_package = |s: &&crate::mise::ToolSource| {
            s.tool == entry.tool
                && s.manager == entry.manager
                && (entry.manager != "brew"
                    || (formula(&entry.target).is_some()
                        && formula(&s.path) == formula(&entry.target)))
        };
        let exact = sources
            .iter()
            .filter(same_package)
            .find(|s| s.version == entry.version && Path::new(&s.path).is_dir());
        // 精确版本绑定不能在扫描时静默跨版本升级；保留记录，明确让用户选择新版本。
        let real = if Path::new(&entry.target).is_dir() {
            Some(PathBuf::from(&entry.target))
        } else {
            exact.map(|source| {
                if entry.manager == "brew" {
                    resolve_brew_layout(Path::new(&source.path))
                } else {
                    PathBuf::from(&source.path)
                }
            })
        };
        if let Some(real) = real {
            let mut updated = entries.clone();
            updated[index].target = real.display().to_string();
            let result = repair(&entry, &real, || save_manifest(&updated));
            let (action, message) = match result {
                Ok(()) => {
                    entries = updated;
                    (
                        "relinked",
                        "已修复同软件、同版本的链接，原版本绑定保持有效".into(),
                    )
                }
                Err(error) => ("failed", error.to_string()),
            };
            events.push(ReconcileEvent {
                tool: entry.tool.clone(),
                from: entry.version.clone(),
                to: Some(entry.version.clone()),
                action: action.into(),
                message,
            });
        } else {
            events.push(ReconcileEvent {
                tool: entry.tool.clone(),
                from: entry.version.clone(),
                to: None,
                action: "needs-review".into(),
                message: format!(
                    "{}@{} 的来源已移除；精确版本绑定保留，请选择并验证替代版本后再解除旧接管",
                    entry.tool, entry.version
                ),
            });
        }
    }
    if events.iter().any(|e| e.action != "relinked") {
        if let Some(id) = crate::operations::current_id() {
            crate::operations::partial(&id);
        }
    }
    Ok(events)
}

/// 策略 C：返回包管理器自维护的稳定 bin 路径（brew 用 opt 链接，scoop 用 current），
/// 供前端写入项目 mise.toml 的 [env] _.path。
pub fn stable_bin_path(manager: &str, name: &str, path_hint: &str) -> Result<PathBuf, AppError> {
    match manager {
        "brew" => {
            // 从扫描路径（Cellar/<formula>/<ver>）还原 formula 名，opt 链接由 brew 维护、升级自动跟随
            let formula = Path::new(path_hint)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .windows(2)
                .find(|w| w[0] == "Cellar")
                .map(|w| w[1].clone())
                .unwrap_or_else(|| name.to_string());
            let mut bases = vec!["/opt/homebrew".to_string(), "/usr/local".to_string()];
            if let Some(h) = home_dir() {
                bases.push(h.join(".linuxbrew").display().to_string());
            }
            for base in bases {
                let bin = Path::new(&base).join("opt").join(&formula).join("bin");
                if bin.exists() {
                    return Ok(bin);
                }
            }
            Err(AppError::Other(format!(
                "未找到 brew 的 opt 链接（{name}），可能未通过 brew 安装"
            )))
        }
        "scoop" => {
            let home = home_dir().ok_or_else(|| AppError::Other("无法定位用户主目录".into()))?;
            let current = home.join("scoop/apps").join(name).join("current");
            if current.exists() {
                Ok(current)
            } else {
                Err(AppError::Other(format!("未找到 scoop 应用 {name}")))
            }
        }
        _ => Err(AppError::Unsupported(
            "该来源暂无稳定路径，建议使用软链接管".into(),
        )),
    }
}

/// 带健康状态的清单条目（对外序列化）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEntryHealthy {
    pub tool: String,
    pub version: String,
    pub manager: String,
    pub target: String,
    pub healthy: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn replacement_can_restore_broken_link_and_never_overwrites_native_directory() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("registered");
        let missing = dir.path().join("old-missing");
        let replacement = dir.path().join("new");
        std::fs::create_dir(&replacement).unwrap();
        create_symlink(&missing, &link).unwrap();
        let before = LinkState::capture(link.clone()).unwrap();
        replace_symlink(&replacement, &link).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), replacement);
        before.restore().unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), missing);
        assert!(replace_symlink(&missing, &replacement).is_err());
        assert!(replacement.is_dir());
    }
    #[test]
    fn corrupt_manifest_and_write_errors_are_not_silenced() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("manifest.json");
        assert!(load_manifest_at(&file).unwrap().is_empty());
        save_manifest_at(&file, &[]).unwrap();
        std::fs::write(&file, "broken").unwrap();
        assert!(load_manifest_at(&file).is_err());
        assert!(save_manifest_at(dir.path(), &[]).is_err());
    }
    #[test]
    fn empty_runtime_is_rejected_before_linking() {
        let dir = tempfile::tempdir().unwrap();
        assert!(verify_runtime("node", "20", dir.path()).is_err());
        assert!(adopt("python", "3", "system", "/usr/bin").is_err());
    }
    #[test]
    fn brew_package_identity_preserves_major_formula() {
        assert_ne!(
            formula("/opt/homebrew/Cellar/node@20/20.1"),
            formula("/opt/homebrew/Cellar/node@22/22.1")
        );
        assert_eq!(
            formula("/opt/homebrew/Cellar/openjdk@21/21/libexec/openjdk.jdk/Contents/Home"),
            Some("openjdk@21".into())
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn java_layout_resolves_real_home() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Cellar/openjdk@21/21");
        let inner = root.join("libexec/openjdk.jdk/Contents/Home");
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(resolve_brew_layout(&root), inner);
    }
}
