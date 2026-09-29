// 版本列表缓存：内存（进程内）+ 磁盘（跨启动）双层，TTL 一致。
// 背景：未认证 GitHub API 每小时限 60 次，重复展开版本列表不应反复请求。
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 缓存有效期：版本列表变化频率低，30 分钟足够新，又能显著减少对 GitHub 的请求。
pub const VERSION_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

/// 内存缓存条目：记录采集时间，超过 TTL 后重新拉取。
struct MemEntry {
    fetched: Instant,
    versions: Vec<String>,
}

fn mem_cache() -> &'static Mutex<HashMap<String, MemEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, MemEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 读缓存：内存 → 磁盘（命中磁盘时回填内存）。
pub fn get_versions(key: &str) -> Option<Vec<String>> {
    if let Some(versions) = mem_get(key) {
        return Some(versions);
    }
    let dir = cache_dir()?;
    let versions = disk_read_at(&dir, key)?;
    mem_set(key, &versions);
    Some(versions)
}

/// 写缓存：内存 + 磁盘（磁盘失败静默：缓存不可用只影响性能，不影响正确性）。
pub fn set_versions(key: &str, versions: &[String]) {
    mem_set(key, versions);
    if let Some(dir) = cache_dir() {
        disk_write_at(&dir, key, versions);
    }
}

fn mem_get(key: &str) -> Option<Vec<String>> {
    let cache = mem_cache().lock().ok()?;
    let entry = cache.get(key)?;
    (entry.fetched.elapsed() < VERSION_CACHE_TTL).then(|| entry.versions.clone())
}

fn mem_set(key: &str, versions: &[String]) {
    if let Ok(mut cache) = mem_cache().lock() {
        cache.insert(
            key.to_string(),
            MemEntry {
                fetched: Instant::now(),
                versions: versions.to_vec(),
            },
        );
    }
}

/// 磁盘缓存根目录：unix 为 ~/.cache/zenv，Windows 为 %USERPROFILE%\AppData\Local\zenv。
fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let home = PathBuf::from(home);
    #[cfg(windows)]
    let base = home.join("AppData/Local/zenv/cache");
    #[cfg(not(windows))]
    let base = home.join(".cache/zenv");
    Some(base)
}

/// 磁盘缓存条目（JSON 文件）。
#[derive(Serialize, Deserialize)]
struct DiskEntry {
    /// 采集时刻（Unix 秒），用于 TTL 判断
    fetched_secs: u64,
    versions: Vec<String>,
}

/// key → 缓存文件名：仅保留字母数字，其余替换为下划线。
fn disk_path(base: &Path, key: &str) -> PathBuf {
    let safe: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    base.join(format!("{}.json", safe))
}

/// 从指定目录读取磁盘缓存（TTL 内有效）。目录参数化便于单元测试。
fn disk_read_at(base: &Path, key: &str) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(disk_path(base, key)).ok()?;
    let entry: DiskEntry = serde_json::from_str(&text).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    (now.saturating_sub(entry.fetched_secs) < VERSION_CACHE_TTL.as_secs()).then_some(entry.versions)
}

/// 写磁盘缓存（目录参数化便于单元测试）。
fn disk_write_at(base: &Path, key: &str, versions: &[String]) {
    if std::fs::create_dir_all(base).is_err() {
        return;
    }
    let fetched_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entry = DiskEntry {
        fetched_secs,
        versions: versions.to_vec(),
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        let _ = std::fs::write(disk_path(base, key), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_cache_roundtrip() {
        set_versions("test:roundtrip", &["1.0.0".to_string()]);
        assert_eq!(
            get_versions("test:roundtrip"),
            Some(vec!["1.0.0".to_string()])
        );
        assert_eq!(get_versions("test:missing"), None);
    }

    #[test]
    fn disk_cache_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        disk_write_at(base, "test:disk", &["2.0.0".to_string()]);
        assert_eq!(
            disk_read_at(base, "test:disk"),
            Some(vec!["2.0.0".to_string()])
        );
        // 未写入的键返回 None
        assert_eq!(disk_read_at(base, "test:absent"), None);
        // 损坏的 JSON 视为未命中
        std::fs::write(disk_path(base, "test:broken"), "not json").unwrap();
        assert_eq!(disk_read_at(base, "test:broken"), None);
        // 文件名清洗：非法字符替换为下划线
        assert_eq!(
            disk_path(base, "gh-tags:rust/rust")
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            "gh_tags_rust_rust.json"
        );
    }
}
