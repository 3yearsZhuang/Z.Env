// “官方生态源”：按运行时分发到其官方/权威的版本索引。
// 仅支持已知映射；不支持的返回 Err（前端静默转为“手动输入”）。
use super::finalize_versions;
use super::github::github_tag_versions;
use crate::cache;
use crate::error::AppError;
use crate::net::{get_json, http_get};

pub fn list_remote_versions(tool: &str) -> Result<Vec<String>, AppError> {
    // 官方索引允许一定频率的重复访问，统一加双层缓存减少请求
    let key = format!("official:{}", tool);
    if let Some(cached) = cache::get_versions(&key) {
        return Ok(cached);
    }
    let list = match tool {
        "node" | "nodejs" => node_dist_versions()?,
        "go" | "golang" => go_proxy_versions()?,
        "python" | "python3" => python_ftp_versions()?,
        "java" | "openjdk" | "temurin" => adoptium_versions()?,
        "ruby" => github_tag_versions("ruby/ruby")?,
        "php" => github_tag_versions("php/php-src")?,
        "dotnet" | "dotnet-sdk" => github_tag_versions("dotnet/runtime")?,
        "rust" | "cargo" => github_tag_versions("rust-lang/rust")?,
        "swift" => github_tag_versions("swiftlang/swift")?,
        "scala" => github_tag_versions("scala/scala")?,
        "kotlin" => github_tag_versions("JetBrains/kotlin")?,
        "terraform" => github_tag_versions("hashicorp/terraform")?,
        "helm" => github_tag_versions("helm/helm")?,
        _ => return Err(AppError::Unsupported(format!("{} 暂无官方生态源", tool))),
    };
    cache::set_versions(&key, &list);
    Ok(list)
}

/// Node 官方 dist 索引：https://nodejs.org/dist/index.json
fn node_dist_versions() -> Result<Vec<String>, AppError> {
    let json = get_json("https://nodejs.org/dist/index.json")?;
    let arr = json
        .as_array()
        .ok_or_else(|| AppError::Parse("Node 源格式异常".to_string()))?;
    let list: Vec<String> = arr
        .iter()
        .filter_map(|v| v.get("version").and_then(|x| x.as_str()))
        .map(|s| s.trim_start_matches('v').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let list = finalize_versions(list);
    if list.is_empty() {
        Err(AppError::Other("未获取到 Node 版本".to_string()))
    } else {
        Ok(list)
    }
}

/// Go 官方 proxy 模块列表：https://proxy.golang.org/golang/go/@v/list
fn go_proxy_versions() -> Result<Vec<String>, AppError> {
    let (status, text) = http_get("https://proxy.golang.org/golang/go/@v/list")?;
    if status >= 400 {
        return Err(AppError::Http {
            status,
            message: "Go proxy 请求失败".to_string(),
        });
    }
    let list: Vec<String> = text
        .lines()
        .map(|l| l.trim().trim_start_matches('v').to_string())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .collect();
    let list = finalize_versions(list);
    if list.is_empty() {
        Err(AppError::Other("未获取到 Go 版本".to_string()))
    } else {
        Ok(list)
    }
}

/// Python 官方下载页目录：https://www.python.org/ftp/python/ （解析 3.N.N 目录名）
fn python_ftp_versions() -> Result<Vec<String>, AppError> {
    let (status, html) = http_get("https://www.python.org/ftp/python/")?;
    if status >= 400 {
        return Err(AppError::Http {
            status,
            message: "Python 源请求失败".to_string(),
        });
    }
    let mut list: Vec<String> = Vec::new();
    let bytes: Vec<u8> = html.bytes().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        // 找 `<a href="3.N.N/">`，读取目录名
        if bytes[i..].starts_with(b"<a href=\"") {
            let start = i + b"<a href=\"".len();
            if let Some(end) = html[start..].find("/\">") {
                let name = &html[start..start + end];
                let digits: Vec<char> = name
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                let clean: String = digits.iter().collect();
                if !clean.is_empty() && clean.starts_with('3') && clean.matches('.').count() >= 2 {
                    list.push(clean);
                }
                i = start + end + 3;
                continue;
            }
        }
        i += 1;
    }
    let list = finalize_versions(list);
    if list.is_empty() {
        Err(AppError::Other("未获取到 Python 版本".to_string()))
    } else {
        Ok(list)
    }
}

/// Adoptium（Java）可用 release 主版本：https://api.adoptium.net/v3/info/available_releases
fn adoptium_versions() -> Result<Vec<String>, AppError> {
    let json = get_json("https://api.adoptium.net/v3/info/available_releases")?;
    let mut list: Vec<String> = Vec::new();
    if let Some(v) = json.get("available_releases").and_then(|x| x.as_array()) {
        for e in v {
            if let Some(n) = e.as_i64() {
                list.push(format!("temurin-{}", n));
            }
        }
        // 长期支持(LTS)优先置顶
        let mut lts: Vec<String> = Vec::new();
        if let Some(v) = json
            .get("available_lts_releases")
            .and_then(|x| x.as_array())
        {
            for e in v {
                if let Some(n) = e.as_i64() {
                    lts.push(format!("temurin-{}", n));
                }
            }
        }
        let non_lts: Vec<String> = list.iter().filter(|&x| !lts.contains(x)).cloned().collect();
        lts.extend(non_lts);
        list = lts;
    }
    if list.is_empty() {
        Err(AppError::Other("未获取到 Java 版本".to_string()))
    } else {
        Ok(list)
    }
}
