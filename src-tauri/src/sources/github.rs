// GitHub Tags 源：从仓库 tags 提取可安装版本。
// 带翻页（大仓库 tags 超过一页）与双层缓存（未认证 API 每小时限 60 次）。
use serde_json::Value;

use super::finalize_versions;
use crate::cache;
use crate::error::AppError;
use crate::net::get_json;

/// 若干常见运行时对应的 GitHub 仓库（用于依 tags 抓取版本，作为备份源）。
const GITHUB_REPOS: &[(&str, &str)] = &[
    ("node", "nodejs/node"),
    ("npm", "npm/cli"),
    ("bun", "oven-sh/bun"),
    ("deno", "denoland/deno"),
    ("terraform", "hashicorp/terraform"),
];

/// 单个仓库最多翻取的 tag 页数（每页 100 条）。
/// rust-lang/rust 等仓库 tag 数远超一页，只取第一页会静默丢失大量版本。
pub(crate) const GITHUB_TAG_PAGES: usize = 3;

/// 通过 GitHub Tags API 获取远程版本。仅支持 `GITHUB_REPOS` 中收录的运行时；
/// 统一走 `github_tag_versions`（带翻页、缓存与状态码级错误处理）。
pub fn list_remote_versions(tool: &str) -> Result<Vec<String>, AppError> {
    let repo = GITHUB_REPOS
        .iter()
        .find(|(t, _)| *t == tool)
        .map(|(_, r)| *r)
        .ok_or_else(|| AppError::Unsupported(format!("暂无 {} 的 GitHub 源映射", tool)))?;
    github_tag_versions(repo)
}

/// 从某 GitHub 仓库的 tags 提取“看起来像版本号”的纯版本列表。
/// 自动处理 `v3.3.0` / `v3_3_0` / `php-8.3.0` / `1.75.0` 等差异。
/// 带翻页（最多 GITHUB_TAG_PAGES 页）与双层缓存，避免大仓库被截断、重复请求被限流。
pub(crate) fn github_tag_versions(repo: &str) -> Result<Vec<String>, AppError> {
    // 命中缓存直接返回，未认证 API 每小时仅 60 次，重复展开列表不应反复请求
    let key = format!("gh-tags:{}", repo);
    if let Some(cached) = cache::get_versions(&key) {
        return Ok(cached);
    }
    let mut list: Vec<String> = Vec::new();
    for page in 1..=GITHUB_TAG_PAGES {
        let url = format!(
            "https://api.github.com/repos/{}/tags?per_page=100&page={}",
            repo, page
        );
        let json = match get_json(&url) {
            Ok(j) => j,
            // 首页即失败说明源不可用；后续页失败则退回已取到的结果
            Err(e) => {
                if list.is_empty() {
                    return Err(e);
                }
                break;
            }
        };
        let Some(arr) = json.as_array() else {
            if list.is_empty() {
                return Err(AppError::Parse("GitHub 返回格式异常".to_string()));
            }
            break;
        };
        list.extend(parse_tag_page(arr));
        if arr.len() < 100 {
            break; // 不足一页，说明已是最后一页
        }
    }
    let list = finalize_versions(list);
    if list.is_empty() {
        Err(AppError::Other(format!("{} 官方源未取到版本", repo)))
    } else {
        cache::set_versions(&key, &list);
        Ok(list)
    }
}

/// 解析一页 tags JSON，提取纯版本列表（纯函数，便于测试）。
fn parse_tag_page(arr: &[Value]) -> Vec<String> {
    arr.iter()
        .filter_map(|tag| tag.get("name").and_then(|v| v.as_str()))
        .filter_map(clean_tag_version)
        .collect()
}

/// 把形如 `v3_3_0` / `php-8.3.0` / `1.75.0` 的 tag 名清洗为纯版本串。
fn clean_tag_version(name: &str) -> Option<String> {
    // 下划线视为点分隔（ruby 旧式 tag）
    let dotted = name.replace('_', ".");
    let start = dotted.find(|c: char| c.is_ascii_digit())?;
    // 数字前前缀过长的 tag（如 rust-lang/rust 的 release-0.7）多为分支/批次命名而非版本
    if start > 6 {
        return None;
    }
    let crop = &dotted[start..];
    let v: String = crop
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    // 至少是主.次，避免只取到一级数字产生误导
    if v.split('.').count() >= 2 {
        Some(v)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_tag_version_handles_common_tag_shapes() {
        assert_eq!(clean_tag_version("v3.3.0").as_deref(), Some("3.3.0"));
        assert_eq!(clean_tag_version("v3_3_0").as_deref(), Some("3.3.0"));
        assert_eq!(clean_tag_version("php-8.3.0").as_deref(), Some("8.3.0"));
        assert_eq!(clean_tag_version("1.75.0").as_deref(), Some("1.75.0"));
        // 品牌前缀 tag（此前 bun 的 bun-v* 全被过滤导致列表为空）
        assert_eq!(clean_tag_version("bun-v1.1.0").as_deref(), Some("1.1.0"));
        // 一级数字或无数字的 tag 不应误判为版本
        assert_eq!(clean_tag_version("nightly"), None);
        assert_eq!(clean_tag_version("weekly.2024"), None);
        // 分支/批次命名（前缀过长）不应误判为版本
        assert_eq!(clean_tag_version("release-0.7"), None);
    }

    #[test]
    fn parse_tag_page_extracts_versions_only() {
        let arr: Vec<Value> = serde_json::from_str(
            r#"[{"name":"v1.10.0"},{"name":"bun-v1.1.0"},{"name":"nightly"},{"name":"v2.0.0"}]"#,
        )
        .unwrap();
        let mut got = parse_tag_page(&arr);
        got.sort();
        assert_eq!(got, vec!["1.1.0", "1.10.0", "2.0.0"]);
    }
}
