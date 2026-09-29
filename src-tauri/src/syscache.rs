// 系统包（brew/winget/apt/pacman）探测结果的进程内 TTL 缓存。
// 背景：brew list 在包多时可耗时数十秒，每次页面挂载重新探测会明显拖慢数据到达；
// TTL 内重复打开直接命中缓存，安装/卸载后由命令层主动失效，保证不出现陈旧的“已安装”状态。
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::env::SystemPkg;

/// 探测缓存有效期：包管理器状态变化频率低，10 分钟足够新鲜。
const SYSPKG_TTL: Duration = Duration::from_secs(10 * 60);

/// 已安装列表缓存：manager → (采集时刻, 包名列表)
type InstalledMap = HashMap<String, (Instant, Vec<String>)>;

/// 版本列表缓存：manager → (采集时刻, 名称+版本列表)
type VersionsMap = HashMap<String, (Instant, Vec<SystemPkg>)>;

fn installed_cache() -> &'static Mutex<InstalledMap> {
    static C: OnceLock<Mutex<InstalledMap>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn versions_cache() -> &'static Mutex<VersionsMap> {
    static C: OnceLock<Mutex<VersionsMap>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 读取已安装列表缓存：命中且未过期时返回克隆。
pub fn get_installed(manager: &str) -> Option<Vec<String>> {
    let cache = installed_cache().lock().ok()?;
    let (fetched, pkgs) = cache.get(manager)?;
    (fetched.elapsed() < SYSPKG_TTL).then(|| pkgs.clone())
}

/// 写入已安装列表缓存（失败静默：缓存不可用只影响性能，不影响正确性）。
pub fn set_installed(manager: &str, pkgs: Vec<String>) {
    if let Ok(mut cache) = installed_cache().lock() {
        cache.insert(manager.to_string(), (Instant::now(), pkgs));
    }
}

/// 读取版本列表缓存：命中且未过期时返回克隆。
pub fn get_versions(manager: &str) -> Option<Vec<SystemPkg>> {
    let cache = versions_cache().lock().ok()?;
    let (fetched, pkgs) = cache.get(manager)?;
    (fetched.elapsed() < SYSPKG_TTL).then(|| pkgs.clone())
}

/// 写入版本列表缓存（失败静默）。
pub fn set_versions(manager: &str, pkgs: Vec<SystemPkg>) {
    if let Ok(mut cache) = versions_cache().lock() {
        cache.insert(manager.to_string(), (Instant::now(), pkgs));
    }
}

/// 安装/卸载后失效该管理器的全部探测缓存（下次探测拿到真实状态）。
pub fn invalidate(manager: &str) {
    if let Ok(mut cache) = installed_cache().lock() {
        cache.remove(manager);
    }
    if let Ok(mut cache) = versions_cache().lock() {
        cache.remove(manager);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_cache_roundtrip_and_invalidate() {
        set_installed("brew", vec!["git".to_string(), "wget".to_string()]);
        assert_eq!(
            get_installed("brew"),
            Some(vec!["git".to_string(), "wget".to_string()])
        );
        // 其他管理器未写入
        assert_eq!(get_installed("apt"), None);
        // 失效后返回 None
        invalidate("brew");
        assert_eq!(get_installed("brew"), None);
    }

    #[test]
    fn versions_cache_roundtrip_and_invalidate() {
        set_versions(
            "brew",
            vec![SystemPkg {
                name: "git".to_string(),
                version: Some("2.46.0".to_string()),
            }],
        );
        let got = get_versions("brew").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "git");
        assert_eq!(got[0].version.as_deref(), Some("2.46.0"));
        invalidate("brew");
        assert!(get_versions("brew").is_none());
    }
}
