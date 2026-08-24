use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

/// mise 安装状态（用于首页提醒）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiseStatus {
    pub installed: bool,
    pub version: Option<String>,
}

/// 检测 mise 是否安装并返回其版本。
pub fn mise_status() -> MiseStatus {
    match run_mise(&["--version"]) {
        Ok(out) => {
            let version = out.lines().next().map(|l| l.to_string());
            MiseStatus {
                installed: true,
                version,
            }
        }
        Err(_) => MiseStatus {
            installed: false,
            version: None,
        },
    }
}

/// 由其他工具托管/安装的一个运行时版本。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSource {
    pub tool: String,
    pub version: String,
    pub manager: String,
    pub path: String,
}

/// 扫描常见目录中被其他工具托管的运行时（nvm / pyenv / asdf / sdkman / rvm 等）。
/// 这些环境不属于 mise，单独列出便于用户了解已存在的环境。
pub fn detect_tool_sources() -> Vec<ToolSource> {
    let mut out: Vec<ToolSource> = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let Some(home) = home else { return out };
    let home = std::path::PathBuf::from(home);

    // nvm: ~/.nvm/versions/node/<vX.Y.Z>
    scan_versions(
        &mut out,
        home.join(".nvm/versions/node").as_path(),
        "node",
        "nvm",
        |name| name.trim_start_matches('v').to_string(),
    );
    // pyenv: ~/.pyenv/versions/<X.Y.Z>
    scan_versions(
        &mut out,
        home.join(".pyenv/versions").as_path(),
        "python",
        "pyenv",
        |name| name.to_string(),
    );
    // rvm: ~/.rvm/rubies/<ruby-X.Y.Z>
    scan_versions(
        &mut out,
        home.join(".rvm/rubies").as_path(),
        "ruby",
        "rvm",
        |name| name.replace("ruby-", "").replace("ruby@", ""),
    );
    // SDKMAN: ~/.sdkman/candidates/<tool>/<version>
    scan_sdkman(&mut out, home.join(".sdkman/candidates").as_path());
    // asdf: ~/.asdf/installs/<tool>/<version>
    scan_asdf(&mut out, home.join(".asdf/installs").as_path());
    // nodenv ~/.nodenv/versions
    scan_versions(
        &mut out,
        home.join(".nodenv/versions").as_path(),
        "node",
        "nodenv",
        |n| n.to_string(),
    );
    // rbenv ~/.rbenv/versions
    scan_versions(
        &mut out,
        home.join(".rbenv/versions").as_path(),
        "ruby",
        "rbenv",
        |n| n.to_string(),
    );
    // goenv ~/.goenv/versions
    scan_versions(
        &mut out,
        home.join(".goenv/versions").as_path(),
        "go",
        "goenv",
        |n| n.to_string(),
    );
    // n: ~/n/versions/node/<vX.Y.Z>
    scan_versions(
        &mut out,
        home.join("n/versions/node").as_path(),
        "node",
        "n",
        |n| n.trim_start_matches('v').to_string(),
    );
    // fnm: ~/.local/share/fnm/node-versions（含 bin）
    scan_versions(
        &mut out,
        home.join(".local/share/fnm/node-versions").as_path(),
        "node",
        "fnm",
        |n| n.trim_start_matches('v').to_string(),
    );
    // volta: ~/.volta/tools/image/<tool>/<version>
    scan_tool_versions(&mut out, home.join(".volta/tools/image").as_path(), "volta");
    // Homebrew Cellar（可跨平台：Intel /usr/local，ARM /opt/homebrew，Linuxbrew）
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        scan_homebrew_cellar(&mut out, std::path::Path::new("/opt/homebrew/Cellar"));
        scan_homebrew_cellar(&mut out, std::path::Path::new("/usr/local/Cellar"));
        scan_homebrew_cellar(&mut out, &home.join(".linuxbrew/Cellar"));
    }
    // macOS 系统自带 / CommandLineTools 二进制
    #[cfg(target_os = "macos")]
    scan_system_binaries(&mut out);
    // Windows: scoop apps
    #[cfg(target_os = "windows")]
    scan_scoop(&mut out, home.join("scoop/apps").as_path());

    out.sort_by(|a, b| {
        (a.tool.as_str(), a.manager.as_str(), a.version.as_str()).cmp(&(
            b.tool.as_str(),
            b.manager.as_str(),
            b.version.as_str(),
        ))
    });
    out
}

/// 扫描 ~/dir 下的一级子目录作为版本，要求目录内含 bin/。
fn scan_versions<F>(
    out: &mut Vec<ToolSource>,
    dir: &std::path::Path,
    tool: &str,
    manager: &str,
    version_fn: F,
) where
    F: Fn(&str) -> String,
{
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let p = e.path();
        if !p.join("bin").exists() {
            continue;
        }
        let version = version_fn(&name);
        if version.is_empty() {
            continue;
        }
        out.push(ToolSource {
            tool: tool.to_string(),
            version,
            manager: manager.to_string(),
            path: p.display().to_string(),
        });
    }
}

/// SDKMAN：candidates/<tool>/<version>
fn scan_sdkman(out: &mut Vec<ToolSource>, dir: &std::path::Path) {
    let Ok(tools) = std::fs::read_dir(dir) else {
        return;
    };
    for t in tools.flatten() {
        if !t.path().is_dir() {
            continue;
        }
        let tool_name = t.file_name().to_string_lossy().to_string();
        if let Ok(versions) = std::fs::read_dir(t.path()) {
            for v in versions.flatten() {
                let vname = v.file_name().to_string_lossy().to_string();
                // 忽略 "current" 符号链接与隐藏项
                if vname == "current" || vname.starts_with('.') {
                    continue;
                }
                if v.path().join("bin").exists() {
                    out.push(ToolSource {
                        tool: tool_name.clone(),
                        version: vname,
                        manager: "sdkman".to_string(),
                        path: v.path().display().to_string(),
                    });
                }
            }
        }
    }
}

/// asdf：installs/<tool>/<version>
fn scan_asdf(out: &mut Vec<ToolSource>, dir: &std::path::Path) {
    let Ok(tools) = std::fs::read_dir(dir) else {
        return;
    };
    for t in tools.flatten() {
        if !t.path().is_dir() {
            continue;
        }
        let tool_name = t.file_name().to_string_lossy().to_string();
        if let Ok(versions) = std::fs::read_dir(t.path()) {
            for v in versions.flatten() {
                let vname = v.file_name().to_string_lossy().to_string();
                if vname == "latest" || vname.starts_with('.') {
                    continue;
                }
                if v.path().join("bin").exists() {
                    out.push(ToolSource {
                        tool: tool_name.clone(),
                        version: vname,
                        manager: "asdf".to_string(),
                        path: v.path().display().to_string(),
                    });
                }
            }
        }
    }
}

/// 通用 <tool>/<version> 结构扫描（用于 volta image 等）。
fn scan_tool_versions(out: &mut Vec<ToolSource>, dir: &std::path::Path, manager: &str) {
    let Ok(tools) = std::fs::read_dir(dir) else {
        return;
    };
    for t in tools.flatten() {
        if !t.path().is_dir() {
            continue;
        }
        let tool_name = t.file_name().to_string_lossy().to_string();
        if let Ok(versions) = std::fs::read_dir(t.path()) {
            for v in versions.flatten() {
                let vname = v.file_name().to_string_lossy().to_string();
                if vname.starts_with('.') || vname == "current" {
                    continue;
                }
                if v.path().join("bin").exists() {
                    out.push(ToolSource {
                        tool: tool_name.clone(),
                        version: vname,
                        manager: manager.to_string(),
                        path: v.path().display().to_string(),
                    });
                }
            }
        }
    }
}

/// Homebrew Cellar：<formula>/<version>，公式名归一化为运行时名。
#[allow(dead_code)]
fn scan_homebrew_cellar(out: &mut Vec<ToolSource>, dir: &std::path::Path) {
    let Ok(formulas) = std::fs::read_dir(dir) else {
        return;
    };
    for f in formulas.flatten() {
        if !f.path().is_dir() {
            continue;
        }
        let formula = f.file_name().to_string_lossy().to_string();
        let Some(tool) = homebrew_tool_name(&formula) else {
            continue;
        };
        let Ok(versions) = std::fs::read_dir(f.path()) else {
            continue;
        };
        for v in versions.flatten() {
            let vname = v.file_name().to_string_lossy().to_string();
            if vname.starts_with('.') {
                continue;
            }
            if v.path().join("bin").exists() {
                out.push(ToolSource {
                    tool: tool.to_string(),
                    version: vname,
                    manager: "brew".to_string(),
                    path: v.path().display().to_string(),
                });
            }
        }
    }
}

/// 将 Homebrew 公式名映射为运行时名：去掉 @版本，openjdk → java。
#[allow(dead_code)]
fn homebrew_tool_name(formula: &str) -> Option<&str> {
    let base = formula.split('@').next().unwrap_or(formula);
    let mapped = match base {
        "openjdk" => "java",
        other => other,
    };
    Some(mapped)
}

/// macOS 系统自带 / CommandLineTools 的固定二进制（cluster 只探测存在的）。
#[cfg(target_os = "macos")]
fn scan_system_binaries(out: &mut Vec<ToolSource>) {
    const CANDIDATES: &[(&str, &str)] = &[
        ("python3", "/usr/bin/python3"),
        ("python", "/usr/bin/python3"),
        ("ruby", "/usr/bin/ruby"),
        ("java", "/usr/bin/java"),
        ("node", "/usr/local/bin/node"),
    ];
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (tool, bin) in CANDIDATES {
        let p = std::path::Path::new(bin);
        if !p.exists() {
            continue;
        }
        // 以首个命中该工具的系统版本为准
        if seen.contains(*tool) {
            continue;
        }
        let version = probe_version(bin)
            .map(|v| format!("system ({})", v))
            .unwrap_or_else(|| "system".to_string());
        seen.insert(tool.to_string());
        out.push(ToolSource {
            tool: tool.to_string(),
            version,
            manager: "system".to_string(),
            path: bin.to_string(),
        });
    }
    // 系统自带多版本能力：查询 java_home 的 JDK 列表
    let jh = std::process::Command::new("/usr/libexec/java_home")
        .args(["-V"])
        .stderr(std::process::Stdio::piped())
        .output();
    if let Ok(o) = jh {
        for line in String::from_utf8_lossy(&o.stderr).lines() {
            let l = line.trim();
            if l.starts_with("Matching Java Virtual Machines") || l.is_empty() {
                continue;
            }
            // 形如 "  20.0.2 (x86_64)  " 取版本号
            let v = l.split_whitespace().next().unwrap_or("").to_string();
            if !v.is_empty() {
                out.push(ToolSource {
                    tool: "java".to_string(),
                    version: format!("system ({})", v),
                    manager: "system".to_string(),
                    path: "/usr/libexec/java_home".to_string(),
                });
            }
        }
    }
    let _ = out;
}
/// 试探式读取二进制 `--version` 输出中的第一个版本 token。
#[allow(dead_code)]
fn probe_version(bin: &str) -> Option<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout)
        .chars()
        .chain(String::from_utf8_lossy(&out.stderr).chars())
        .collect::<String>();
    // 取包含数字的 token（如 "3.9.6"）
    let token = text
        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == 'v')
        .find(|t| !t.is_empty() && t.chars().any(|c| c.is_ascii_digit()));
    token.map(|t| t.to_string())
}

/// Windows scoop：apps/<app>/<version>（app 名即工具名）。
#[cfg(target_os = "windows")]
fn scan_scoop(out: &mut Vec<ToolSource>, dir: &std::path::Path) {
    scan_tool_versions(out, dir, "scoop");
}

/// 安装进度事件负载，通过 `mise:install-progress` 事件推送给前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub tool: String,
    pub version: String,
    pub line: String,
}

/// 单个已安装工具版本的信息（来自 `mise ls --json`）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolVersion {
    pub version: String,
    pub requested_version: Option<String>,
    pub install_path: Option<String>,
    pub installed: Option<bool>,
    pub active: Option<bool>,
}

/// 聚合后的工具信息（一个工具名对应多个已安装版本）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub versions: Vec<ToolVersion>,
    pub active_versions: Vec<String>,
}

/// 远程可安装版本（来自 `mise ls-remote <tool> --json`）
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteVersion {
    pub version: String,
}

/// 定位 mise 可执行文件。
/// 优先使用绝对路径（GUI 应用不一定继承 shell 的 PATH）。
pub fn find_mise() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(h) = &home {
        let h = PathBuf::from(h);
        #[cfg(windows)]
        {
            candidates.push(h.join(".local/bin/mise.exe"));
            candidates.push(h.join("scoop/shims/mise.exe"));
            candidates.push(h.join(".cargo/bin/mise.exe"));
        }
        #[cfg(not(windows))]
        {
            candidates.push(h.join(".local/bin/mise"));
            candidates.push(h.join(".cargo/bin/mise"));
        }
    }

    #[cfg(unix)]
    {
        candidates.push(PathBuf::from("/usr/local/bin/mise"));
        candidates.push(PathBuf::from("/opt/homebrew/bin/mise"));
        candidates.push(PathBuf::from("/usr/bin/mise"));
        candidates.push(PathBuf::from("/bin/mise"));
    }

    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    // 兜底：依赖系统 PATH
    PathBuf::from("mise")
}

/// 构造一个用于运行 mise 的 PATH，确保 mise 能定位到它自带的工具。
pub(crate) fn build_path() -> String {
    let mut dirs: Vec<String> = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(h) = &home {
        let h = PathBuf::from(h);
        dirs.push(h.join(".local/bin").display().to_string());
        #[cfg(unix)]
        {
            dirs.push("/usr/local/bin".to_string());
            dirs.push("/opt/homebrew/bin".to_string());
        }
    }
    if let Ok(existing) = std::env::var("PATH") {
        dirs.push(existing);
    }
    // Windows 的路径分隔符是 ";"，其他平台是 ":"
    let sep = if cfg!(windows) { ";" } else { ":" };
    dirs.join(sep)
}

/// 用户主目录。GUI 应用从 Finder/Dock 启动时 cwd 可能不可写或非项目目录，
/// 统一以 home 作为 mise 命令的工作目录，保证 use/install/uninstall/link 可靠执行。
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// 执行一个 mise 命令并返回标准输出。失败时返回错误信息。
pub fn run_mise(args: &[&str]) -> Result<String, String> {
    let mise = find_mise();
    let mut command = Command::new(&mise);
    command.args(args).env("PATH", build_path());
    if let Some(h) = home_dir() {
        command.current_dir(h);
    }
    let output = command
        .output()
        .map_err(|e| format!("无法执行 mise ({}): {}", mise.display(), e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let combined = if stderr.is_empty() { stdout } else { stderr };
        if combined.is_empty() {
            Err("mise 命令失败（无输出，请检查工具名或是否已安装对应插件）".to_string())
        } else {
            Err(combined)
        }
    }
}

/// 列出所有已安装的工具及其版本。
pub fn list_tools() -> Result<Vec<ToolInfo>, String> {
    let output = run_mise(&["ls", "--json"])?;
    let map: Result<serde_json::Map<String, serde_json::Value>, _> = serde_json::from_str(&output);
    let map = map.map_err(|e| format!("解析 mise ls 输出失败: {}", e))?;

    let mut tools: Vec<ToolInfo> = Vec::new();
    for (name, value) in map {
        let versions: Vec<ToolVersion> = match serde_json::from_value(value) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let active_versions: Vec<String> = versions
            .iter()
            .filter(|v| v.active == Some(true))
            .map(|v| v.version.clone())
            .collect();
        tools.push(ToolInfo {
            name,
            versions,
            active_versions,
        });
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tools)
}

/// 查看某个工具的远程可安装版本。
pub fn list_remote_versions(tool: &str) -> Result<Vec<String>, String> {
    // mise ls-remote 默认远端大版本列表；限定工具名
    let output = run_mise(&["ls-remote", tool, "--json"])?;
    let versions: Vec<RemoteVersion> =
        serde_json::from_str(&output).map_err(|e| format!("解析 {} 远程版本失败: {}", tool, e))?;
    let mut list: Vec<String> = versions.into_iter().map(|v| v.version).collect();
    // 去重并保留顺序；前端期望较新的在前面
    list.dedup();
    list.sort_by(|a, b| normalize_version(b).cmp(&normalize_version(a)));
    Ok(list)
}

/// 通过 ASDF 源获取远程版本（若安装了 asdf）。
pub fn list_remote_versions_asdf(tool: &str) -> Result<Vec<String>, String> {
    let out = Command::new("asdf")
        .args(["list-all", tool])
        .output()
        .map_err(|e| format!("无法执行 asdf list-all: {}", e))?;
    if !out.status.success() {
        return Err("asdf 不可用或工具无对应插件".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut list: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    list.dedup();
    list.sort_by(|a, b| normalize_version(b).cmp(&normalize_version(a)));
    Ok(list)
}

/// 若干常见运行时对应的 GitHub 仓库（用于依 tags 抓取版本，作为备份源）。
const GITHUB_REPOS: &[(&str, &str)] = &[
    ("node", "nodejs/node"),
    ("npm", "npm/cli"),
    ("bun", "oven-sh/bun"),
    ("deno", "denoland/deno"),
    ("terraform", "hashicorp/terraform"),
];

/// 通过 GitHub Tags API 获取远程版本。仅支持 `GITHUB_REPOS` 中收录的运行时；
/// 标签名规范化：去掉前缀 "v"、剔除含预发布分隔符的条目。
pub fn list_remote_versions_github(tool: &str) -> Result<Vec<String>, String> {
    let repo = GITHUB_REPOS
        .iter()
        .find(|(t, _)| *t == tool)
        .map(|(_, r)| *r)
        .ok_or_else(|| format!("暂无 {} 的 GitHub 源映射", tool))?;
    let url = format!(
        "https://api.github.com/repos/{}/tags?per_page=100&page=1",
        repo
    );
    let out = Command::new("curl")
        .args(["-s", "-L", "-H", "User-Agent: mise-gui", &url])
        .output()
        .map_err(|e| format!("无法请求 GitHub: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("\"message\"") {
        return Err("GitHub API 返回异常（可能已达未授权限流）".to_string());
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "GitHub 返回格式异常".to_string())?;
    let arr = json
        .as_array()
        .ok_or_else(|| "GitHub 返回格式异常".to_string())?;
    let mut list: Vec<String> = Vec::new();
    for tag in arr {
        let name = tag.get("name").and_then(|v| v.as_str()).unwrap_or("");
        // 只保留形如 "X.Y.Z"、可被 mise 直接安装的纯版本标签
        let v = name.trim_start_matches('v');
        if v.is_empty() || !v.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        list.push(v.to_string());
    }
    list.dedup();
    list.sort_by(|a, b| normalize_version(b).cmp(&normalize_version(a)));
    Ok(list)
}

/// 通用 curl 抓取并解析 JSON。
fn curl_json(url: &str) -> Result<serde_json::Value, String> {
    let out = Command::new("curl")
        .args(["-s", "-L", "-H", "User-Agent: mise-gui", url])
        .output()
        .map_err(|e| format!("无法请求: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&text).map_err(|_| "请求返回格式异常".to_string())
}

/// “官方生态源”：按运行时分发到其官方/权威的版本列表。
/// 仅支持已知映射；不支持的返回 Err（前端静默转为“手动输入”）。
pub fn list_remote_versions_official(tool: &str) -> Result<Vec<String>, String> {
    match tool {
        "node" | "nodejs" => node_dist_versions(),
        "go" | "golang" => go_proxy_versions(),
        "python" | "python3" => python_ftp_versions(),
        "java" | "openjdk" | "temurin" => adoptium_versions(),
        _ => Err(format!("{} 暂无官方生态源", tool)),
    }
}

/// Node 官方 dist 索引：https://nodejs.org/dist/index.json
fn node_dist_versions() -> Result<Vec<String>, String> {
    let json = curl_json("https://nodejs.org/dist/index.json")?;
    let arr = json
        .as_array()
        .ok_or_else(|| "Node 源格式异常".to_string())?;
    let mut list: Vec<String> = arr
        .iter()
        .filter_map(|v| v.get("version").and_then(|x| x.as_str()))
        .map(|s| s.trim_start_matches('v').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    list.dedup();
    list.sort_by(|a, b| normalize_version(b).cmp(&normalize_version(a)));
    if list.is_empty() {
        Err("未获取到 Node 版本".to_string())
    } else {
        Ok(list)
    }
}

/// Go 官方 proxy 模块列表：https://proxy.golang.org/golang/go/@v/list
fn go_proxy_versions() -> Result<Vec<String>, String> {
    let out = Command::new("curl")
        .args(["-s", "-L", "https://proxy.golang.org/golang/go/@v/list"])
        .output()
        .map_err(|e| format!("无法请求 Go proxy: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut list: Vec<String> = text
        .lines()
        .map(|l| l.trim().trim_start_matches('v').to_string())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .collect();
    list.dedup();
    list.sort_by(|a, b| normalize_version(b).cmp(&normalize_version(a)));
    if list.is_empty() {
        Err("未获取到 Go 版本".to_string())
    } else {
        Ok(list)
    }
}

/// Python 官方下载页目录：https://www.python.org/ftp/python/ （解析 3.N.N 目录名）
fn python_ftp_versions() -> Result<Vec<String>, String> {
    let out = Command::new("curl")
        .args(["-s", "-L", "https://www.python.org/ftp/python/"])
        .output()
        .map_err(|e| format!("无法请求 Python 源: {}", e))?;
    let html = String::from_utf8_lossy(&out.stdout);
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
    list.dedup();
    list.sort_by(|a, b| normalize_version(b).cmp(&normalize_version(a)));
    if list.is_empty() {
        Err("未获取到 Python 版本".to_string())
    } else {
        Ok(list)
    }
}

/// Adoptium（Java）可用 release 主版本：https://api.adoptium.net/v3/info/available_releases
fn adoptium_versions() -> Result<Vec<String>, String> {
    let json = curl_json("https://api.adoptium.net/v3/info/available_releases")?;
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
        let non_lts: Vec<String> = list.iter().cloned().filter(|x| !lts.contains(x)).collect();
        lts.extend(non_lts);
        list = lts;
    }
    if list.is_empty() {
        Err("未获取到 Java 版本".to_string())
    } else {
        Ok(list)
    }
}

/// 列出 mise 支持的全部运行时/插件名（用于自检覆盖完整性）。
pub fn list_registry() -> Result<Vec<String>, String> {
    let raw = match run_mise(&["registry"]) {
        Ok(o) => o,
        Err(_) => run_mise(&["plugins", "ls-remote"])
            .map_err(|_| "无法获取 mise 运行时列表".to_string())?,
    };
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
            continue;
        }
        let name = t.split_whitespace().next().unwrap_or("").to_string();
        if !name.is_empty() {
            out.push(name);
        }
    }
    out.dedup();
    Ok(out)
}

/// 一个简单的版本比较辅助（按点分段、按段数字比较）。
fn normalize_version(v: &str) -> Vec<u64> {
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

/// 安装指定工具版本。
pub fn install_version(tool: &str, version: &str) -> Result<String, String> {
    let spec = format!("{}@{}", tool, version);
    run_mise(&["install", &spec]).map(|_| format!("{} 安装完成", spec))
}

/// 流式安装指定工具版本，逐帧将输出通过 Tauri 事件推送给前端。
pub fn install_version_streaming(
    app: &AppHandle,
    tool: &str,
    version: &str,
) -> Result<String, String> {
    let spec = format!("{}@{}", tool, version);
    let mise = find_mise();

    let mut command = Command::new(&mise);
    command
        .args(["install", &spec])
        .env("PATH", build_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(h) = home_dir() {
        command.current_dir(h);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动 mise install: {}", e))?;

    let stdout = child.stdout.take().expect("stdout pipe");
    let stderr = child.stderr.take().expect("stderr pipe");

    // 两个线程分别转发 stdout / stderr 到前端事件
    let app_out = app.clone();
    let out_tool = tool.to_string();
    let out_version = version.to_string();
    let t_out = std::thread::spawn(move || {
        pump_stream(stdout, &app_out, &out_tool, &out_version);
    });

    let app_err = app.clone();
    let err_tool = tool.to_string();
    let err_version = version.to_string();
    let t_err = std::thread::spawn(move || {
        pump_stream(stderr, &app_err, &err_tool, &err_version);
    });

    let status = child
        .wait()
        .map_err(|e| format!("等待 mise 进程失败: {}", e))?;
    let _ = t_out.join();
    let _ = t_err.join();

    if status.success() {
        Ok(format!("{} 安装完成", spec))
    } else {
        Err(format!("{} 安装失败", spec))
    }
}

/// 逐字节读取子进程输出，按 `\r` / `\n` 切分成一行帧并推送事件。
fn pump_stream<R: Read>(mut reader: R, app: &AppHandle, tool: &str, version: &str) {
    let mut byte = [0u8; 1];
    let mut line = String::new();
    loop {
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                let c = byte[0] as char;
                if c == '\n' || c == '\r' {
                    if !line.trim().is_empty() {
                        let _ = app.emit(
                            "mise:install-progress",
                            InstallProgress {
                                tool: tool.to_string(),
                                version: version.to_string(),
                                line: line.trim_end().to_string(),
                            },
                        );
                    }
                    line.clear();
                } else {
                    line.push(c);
                }
            }
            Err(_) => break,
        }
    }
    // 末尾残留（可能没有换行符）
    if !line.trim().is_empty() {
        let _ = app.emit(
            "mise:install-progress",
            InstallProgress {
                tool: tool.to_string(),
                version: version.to_string(),
                line: line.trim_end().to_string(),
            },
        );
    }
}

/// 卸载指定工具版本。
pub fn uninstall_version(tool: &str, version: &str) -> Result<String, String> {
    let spec = format!("{}@{}", tool, version);
    run_mise(&["uninstall", &spec]).map(|_| format!("{} 卸载完成", spec))
}

/// 切换（激活）某个工具版本。global 为 true 时写入全局配置。
pub fn use_version(tool: &str, version: &str, global: bool) -> Result<String, String> {
    let spec = format!("{}@{}", tool, version);
    // 注意：global=false 时不要传空字符串参数，否则 `mise use ""` 可能解析异常。
    if global {
        run_mise(&["use", "-g", &spec]).map(|_| format!("已设为全局默认：{}", spec))
    } else {
        run_mise(&["use", &spec]).map(|_| format!("已在当前项目启用：{}", spec))
    }
}

/// 接管：将已存在的外部环境目录链接为 mise 管理版本（`mise link`），
/// 无需重新下载。
pub fn link_version(tool: &str, version: &str, path: &str) -> Result<String, String> {
    let spec = format!("{}@{}", tool, version);
    run_mise(&["link", &spec, path]).map(|_| format!("已接管 {} -> {}", spec, path))
}

/// 解除接管：移除某版本与外部目录的链接。
/// mise 无 `unlink` 子命令（`unlink` 会被当作未知命令报“no tasks defined”）；
/// 对由 `link` 创建的 symlink，用 `mise uninstall` 即可移除链接而不删除外部目录。
pub fn unlink_version(tool: &str, version: &str) -> Result<String, String> {
    let spec = format!("{}@{}", tool, version);
    run_mise(&["uninstall", &spec]).map(|_| format!("已解除接管（移除链接）{}", spec))
}

/// 读取项目根目录的 mise 配置文件内容。
pub fn read_project_config(path: &str) -> Result<String, String> {
    let p = PathBuf::from(path);
    if !p.exists() {
        return Err(format!("路径不存在：{}", path));
    }
    if p.is_file() {
        std::fs::read_to_string(&p).map_err(|e| format!("读取 {} 失败: {}", path, e))
    } else {
        // 目录：优先找 mise.toml，其次是 .tool-versions
        let candidates = [p.join("mise.toml"), p.join(".tool-versions")];
        for c in candidates {
            if c.exists() {
                return std::fs::read_to_string(&c)
                    .map_err(|e| format!("读取 {} 失败: {}", c.display(), e));
            }
        }
        Err(format!(
            "目录 {} 下找不到 mise.toml 或 .tool-versions",
            path
        ))
    }
}

/// 将项目配置内容写入指定路径的 mise.toml。
pub fn write_project_config(path: &str, content: &str) -> Result<String, String> {
    let p = PathBuf::from(path);
    if p.is_dir() {
        let target = p.join("mise.toml");
        std::fs::write(&target, content)
            .map_err(|e| format!("写入 {} 失败: {}", target.display(), e))?;
        Ok(format!("已保存到 {}", target.display()))
    } else {
        std::fs::write(&p, content).map_err(|e| format!("写入 {} 失败: {}", path, e))?;
        Ok(format!("已保存到 {}", path))
    }
}

/// 在指定工作目录中运行一次 mise 命令。
fn run_mise_in(cwd: &str, args: &[&str]) -> Result<String, String> {
    let mise = find_mise();
    let output = Command::new(&mise)
        .args(args)
        .current_dir(cwd)
        .env("PATH", build_path())
        .output()
        .map_err(|e| format!("无法执行 mise ({}): {}", mise.display(), e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let combined = if stderr.is_empty() { stdout } else { stderr };
        Err(combined)
    }
}

/// 将配置写入项目并一次性安装全套环境（`mise install`）。
pub fn install_all_project(dir: &str, content: &str) -> Result<String, String> {
    let p = PathBuf::from(dir);
    let target = if p.is_dir() {
        p
    } else {
        p.parent().map(|x| x.to_path_buf()).unwrap_or(p)
    };
    let _ = std::fs::create_dir_all(&target);
    let toml = target.join("mise.toml");
    std::fs::write(&toml, content).map_err(|e| format!("写入 {} 失败: {}", toml.display(), e))?;

    let out = run_mise_in(&target.display().to_string(), &["install"])?;
    let summary: Vec<&str> = out.lines().take(10).collect();
    Ok(format!(
        "已写入 {} 并安装：\n{}",
        toml.display(),
        summary.join("\n")
    ))
}
