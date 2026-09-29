// 托管接入层：把系统包管理器（brew 等）安装的运行时接入 mise，并长期保持健康。
//
// 三种策略：
//  A. 直连接管：nvm/pyenv/asdf 等用户级稳定目录，直接 mise link；
//  B. 托管接管：brew 等“目录生命周期归包管理器”的来源，软链指向应用自有的农场目录
//     （~/.zenv/managed/<tool>/<version> → 真实目录），mise 只认识农场路径；
//     目标目录被 brew 升级/删除时，reconcile 对账自动重连到新版本或显式移除；
//  C. PATH 绑定：不建立任何链接，向项目 mise.toml 写入 [env] _.path（stable_bin_path
//     返回 brew opt / scoop current 这类包管理器自维护的稳定路径），由前端完成写入。
//
// 所有链接动作完成后都通过 `mise ls` 校验注册结果，失败自动回滚。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    /// relinked = 已自动重连；removed = 目标消失且无替代，已移除接入；failed = 重连失败
    pub action: String,
    pub message: String,
}

fn manifest_path() -> Result<PathBuf, AppError> {
    Ok(managed_root()
        .ok_or_else(|| AppError::Other("无法定位用户主目录".into()))?
        .join("manifest.json"))
}

fn load_manifest_at(path: &Path) -> Vec<ManagedEntry> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_manifest_at(path: &Path, entries: &[ManagedEntry]) {
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, json);
    }
}

fn load_manifest() -> Vec<ManagedEntry> {
    match manifest_path() {
        Ok(p) => load_manifest_at(&p),
        Err(_) => Vec::new(),
    }
}

fn save_manifest(entries: &[ManagedEntry]) -> Result<(), AppError> {
    let p = manifest_path()?;
    save_manifest_at(&p, entries);
    Ok(())
}

/// 创建目录软链（Windows 需开发者模式；brew 仅存在于 unix，此路径不会在 Windows 触发）。
fn create_symlink(target: &Path, link: &Path) -> Result<(), AppError> {
    let _ = std::fs::remove_file(link);
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
    let vs: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap_or_default();
    let ok = vs
        .iter()
        .any(|v| v.get("version").and_then(|x| x.as_str()) == Some(version));
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
        let _ = run_mise(&["uninstall", &spec]);
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
        let _ = run_mise(&["uninstall", &spec]);
        return Err(e);
    }
    Ok(())
}

/// 接入 mise：按来源自动选择策略，成功后更新托管清单。
pub fn adopt(tool: &str, version: &str, manager: &str, path: &str) -> Result<String, AppError> {
    if manager == "system" {
        return Err(AppError::Unsupported(
            "系统组件没有独立的版本目录，无法接管；可在「项目配置」中使用整机环境绑定（PATH 方式）"
                .into(),
        ));
    }
    let real = PathBuf::from(path);
    if !real.exists() {
        return Err(AppError::Other(format!("源目录不存在：{}", path)));
    }
    let spec = format!("{}@{}", tool, version);

    if manager == "brew" {
        // 策略 B：托管接管（brew 仅存在于 unix，农场软链可用）
        let real = resolve_brew_layout(&real);
        adopt_farm(tool, version, &real)?;
        let mut entries = load_manifest();
        entries.retain(|e| !(e.tool == tool && e.version == version));
        entries.push(ManagedEntry {
            tool: tool.to_string(),
            version: version.to_string(),
            manager: manager.to_string(),
            target: real.display().to_string(),
        });
        save_manifest(&entries)?;
        return Ok(format!(
            "已接管 {spec}（托管模式：brew 升级后将在下次扫描时自动重连到新版本）"
        ));
    }

    // 策略 A：直连
    adopt_direct(tool, version, &real)?;
    Ok(format!("已接管 {spec}"))
}

/// 解除接入：移除 mise 链接与农场记录（对旧的直连接管同样有效）。
pub fn unadopt(tool: &str, version: &str) -> Result<String, AppError> {
    let spec = format!("{}@{}", tool, version);
    run_mise(&["uninstall", &spec])?;
    if let Ok(farm) = farm_path(tool, version) {
        let _ = std::fs::remove_file(farm);
    }
    let mut entries = load_manifest();
    let before = entries.len();
    entries.retain(|e| !(e.tool == tool && e.version == version));
    if entries.len() != before {
        save_manifest(&entries)?;
    }
    Ok(format!("已解除接管 {spec}"))
}

/// 当前列表 + 健康状态（真实目录是否仍存在）。
pub fn list() -> Vec<ManagedEntryHealthy> {
    load_manifest()
        .into_iter()
        .map(|e| {
            let healthy = Path::new(&e.target).exists();
            ManagedEntryHealthy {
                tool: e.tool,
                version: e.version,
                manager: e.manager,
                target: e.target,
                healthy,
            }
        })
        .collect()
}

/// 对账：目标目录消失（brew 升级/卸载）时自动重连到同来源的新版本，或显式移除接入。
pub fn reconcile() -> Vec<ReconcileEvent> {
    let mut events: Vec<ReconcileEvent> = Vec::new();
    let entries = load_manifest();
    if entries.is_empty() {
        return events;
    }
    let sources = detect_tool_sources();
    let mut kept: Vec<ManagedEntry> = Vec::new();

    for e in entries {
        if Path::new(&e.target).exists() {
            kept.push(e);
            continue;
        }
        // 目标消失：在同来源中寻找仍然存在的新版本目录
        let candidate = sources
            .iter()
            .filter(|s| s.tool == e.tool && s.manager == e.manager)
            .filter(|s| Path::new(&s.path).exists() && s.path != e.target)
            .max_by_key(|s| normalize_version(&s.version))
            .cloned();

        let spec = format!("{}@{}", e.tool, e.version);
        let _ = run_mise(&["uninstall", &spec]); // 旧链接（已悬空）先移除
        let _ = std::fs::remove_file(farm_path(&e.tool, &e.version).unwrap_or_default());

        match candidate {
            Some(next) if next.version != e.version => {
                let new_real = resolve_brew_layout(Path::new(&next.path));
                match adopt_farm(&e.tool, &next.version, &new_real) {
                    Ok(()) => {
                        let mut next_entry = e.clone();
                        next_entry.version = next.version.clone();
                        next_entry.target = new_real.display().to_string();
                        kept.push(next_entry);
                        events.push(ReconcileEvent {
                            tool: e.tool.clone(),
                            from: e.version.clone(),
                            to: Some(next.version.clone()),
                            action: "relinked".into(),
                            message: format!(
                                "{} {} 已被 {} 升级为 {}，已自动重连接入",
                                e.tool, e.version, e.manager, next.version
                            ),
                        });
                    }
                    Err(err) => {
                        events.push(ReconcileEvent {
                            tool: e.tool.clone(),
                            from: e.version.clone(),
                            to: Some(next.version.clone()),
                            action: "failed".into(),
                            message: format!(
                                "{} 升级到 {} 后重连失败：{}；可在运行时页重新接管",
                                e.tool, next.version, err
                            ),
                        });
                    }
                }
            }
            _ => {
                events.push(ReconcileEvent {
                    tool: e.tool.clone(),
                    from: e.version.clone(),
                    to: None,
                    action: "removed".into(),
                    message: format!(
                        "{} {} 的 {} 目录已不存在（可能被卸载），已移除接入",
                        e.tool, e.version, e.manager
                    ),
                });
            }
        }
    }
    if save_manifest(&kept).is_ok() {
        return events;
    }
    events
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

    #[test]
    fn manifest_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("manifest.json");
        assert!(load_manifest_at(&path).is_empty());
        save_manifest_at(
            &path,
            &[ManagedEntry {
                tool: "node".into(),
                version: "22.11.0".into(),
                manager: "brew".into(),
                target: "/opt/homebrew/Cellar/node/22.11.0".into(),
            }],
        );
        let loaded = load_manifest_at(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].tool, "node");
        assert_eq!(loaded[0].version, "22.11.0");
        // 损坏的 JSON 视为空清单
        std::fs::write(&path, "not json").unwrap();
        assert!(load_manifest_at(&path).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn brew_openjdk_layout_resolves_to_inner_home() {
        let tmp = tempfile::tempdir().unwrap();
        let cellar = tmp.path().join("Cellar/openjdk/21");
        let home = cellar.join("libexec/openjdk.jdk/Contents/Home");
        std::fs::create_dir_all(&home).unwrap();
        let resolved = resolve_brew_layout(&cellar);
        assert_eq!(resolved, home);
        // 非 openjdk 目录原样返回
        let plain = tmp.path().join("Cellar/node/22.11.0");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(resolve_brew_layout(&plain), plain);
    }

    #[test]
    fn adopt_rejects_system_and_missing_dir() {
        let err = adopt("python", "3.9.6", "system", "/usr/bin").unwrap_err();
        assert!(matches!(err, AppError::Unsupported(_)));
        let err = adopt("node", "20.0.0", "nvm", "/nonexistent/path").unwrap_err();
        assert!(err.to_string().contains("源目录不存在"));
    }
}
