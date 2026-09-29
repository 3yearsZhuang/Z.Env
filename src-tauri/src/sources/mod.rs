// 远程版本源：mise 官方 ls-remote / ASDF / GitHub Tags / 官方生态源。
// 四类源统一产出“降序去重后的纯版本列表”，便于前端直接展示。
mod asdf;
mod github;
mod official;

use serde::Deserialize;

use crate::error::AppError;
use crate::mise::run_mise;

/// 远程可安装版本（来自 `mise ls-remote <tool> --json`）
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteVersion {
    pub version: String,
}

/// 查看某个工具的远程可安装版本（mise 官方源）。
pub fn list_remote_versions(tool: &str) -> Result<Vec<String>, AppError> {
    // mise ls-remote 默认远端大版本列表；限定工具名
    let output = run_mise(&["ls-remote", tool, "--json"])?;
    let versions: Vec<RemoteVersion> = serde_json::from_str(&output)
        .map_err(|e| AppError::Parse(format!("解析 {} 远程版本失败: {}", tool, e)))?;
    Ok(finalize_versions(
        versions.into_iter().map(|v| v.version).collect(),
    ))
}

/// 通过 ASDF 源获取远程版本（若安装了 asdf）。
pub fn list_remote_versions_asdf(tool: &str) -> Result<Vec<String>, AppError> {
    asdf::list_remote_versions(tool)
}

/// 通过 GitHub Tags 源获取远程版本（`GITHUB_REPOS` 中收录的运行时）。
pub fn list_remote_versions_github(tool: &str) -> Result<Vec<String>, AppError> {
    github::list_remote_versions(tool)
}

/// 通过“官方生态源”（node/go/python/java 等官方索引）获取远程版本。
pub fn list_remote_versions_official(tool: &str) -> Result<Vec<String>, AppError> {
    official::list_remote_versions(tool)
}

/// 一个简单的版本比较辅助（按点分段、按段数字比较）。
pub(crate) fn normalize_version(v: &str) -> Vec<u64> {
    let mut segs = Vec::new();
    let mut num = String::new();
    for ch in v.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else if !num.is_empty() {
            segs.push(num.parse().unwrap_or(0));
            num.clear();
        }
    }
    if !num.is_empty() {
        segs.push(num.parse().unwrap_or(0));
    }
    segs
}

/// 收尾：先按版本降序排序再去重（dedup 只移除相邻重复，必须先排序）。
pub(crate) fn finalize_versions(mut list: Vec<String>) -> Vec<String> {
    list.sort_by_key(|a| std::cmp::Reverse(normalize_version(a)));
    list.dedup();
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_versions_sorts_descending_and_dedups() {
        let got = finalize_versions(vec![
            "1.9.0".to_string(),
            "1.10.0".to_string(),
            "1.9.0".to_string(),
            "1.2.0".to_string(),
        ]);
        assert_eq!(got, vec!["1.10.0", "1.9.0", "1.2.0"]);
    }

    #[test]
    fn normalize_version_segments_numerically() {
        assert_eq!(normalize_version("1.10.0"), vec![1, 10, 0]);
        // 非数字字符被忽略
        assert_eq!(normalize_version("v2"), vec![2]);
        assert_eq!(normalize_version("nightly"), Vec::<u64>::new());
    }
}
