use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::error::AppError;

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
    pub package_id: Option<String>,
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
            package_id: None,
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
                        package_id: None,
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
                        package_id: None,
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
                        package_id: None,
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
                    package_id: Some(formula.clone()),
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
        "node" | "python" | "go" | "ruby" | "rust" | "php" | "bun" | "deno" | "dotnet" => base,
        _ => return None,
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
            package_id: None,
            tool: tool.to_string(),
            version,
            manager: "system".to_string(),
            path: bin.to_string(),
        });
    }
    // 系统自带多版本能力：查询 java_home 的 JDK 列表
    let jh = crate::process::output(
        std::process::Command::new("/usr/libexec/java_home").args(["-V"]),
        Duration::from_secs(10),
    );
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
                    package_id: None,
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
    let out = crate::process::output(
        std::process::Command::new(bin).arg("--version"),
        Duration::from_secs(10),
    )
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
    pub job_id: String,
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
pub fn run_mise(args: &[&str]) -> Result<String, AppError> {
    let mise = find_mise();
    let mut command = Command::new(&mise);
    command.args(args).env("PATH", build_path());
    if let Some(h) = home_dir() {
        command.current_dir(h);
    }
    let output = crate::process::output_with_environment(
        &mut command,
        Duration::from_secs(120),
        crate::process::EnvironmentPolicy::AsConfigured,
    )?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let combined = if stderr.is_empty() { stdout } else { stderr };
        if combined.is_empty() {
            Err(AppError::Other(
                "mise 命令失败（无输出，请检查工具名或是否已安装对应插件）".to_string(),
            ))
        } else {
            Err(AppError::Other(combined))
        }
    }
}

/// 列出所有已安装的工具及其版本。
pub fn list_tools() -> Result<Vec<ToolInfo>, AppError> {
    parse_tools_output(&run_mise(&["ls", "--json"])?)
}

pub fn list_global_tools() -> Result<Vec<ToolInfo>, AppError> {
    parse_tools_output(&run_mise(&["ls", "--global", "--json"])?)
}

fn parse_tools_output(output: &str) -> Result<Vec<ToolInfo>, AppError> {
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(output)
        .map_err(|e| AppError::Parse(format!("解析 mise ls 输出失败: {}", e)))?;

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

/// 列出 mise 支持的全部运行时/插件名（用于自检覆盖完整性）。
pub fn list_registry() -> Result<Vec<String>, AppError> {
    let raw = match run_mise(&["registry"]) {
        Ok(o) => o,
        Err(_) => run_mise(&["plugins", "ls-remote"])
            .map_err(|_| AppError::Other("无法获取 mise 运行时列表".to_string()))?,
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

/// 安装指定工具版本。
pub fn install_version(tool: &str, version: &str) -> Result<String, AppError> {
    validate_spec(tool, version)?;
    let spec = format!("{}@{}", tool, version);
    run_mise(&["install", &spec]).map(|_| format!("{} 安装完成", spec))
}

/// 任务 ID 贯穿日志与命令；仅安装内容，不隐式改变用户默认版本。
pub fn install_version_streaming(
    app: &AppHandle,
    tool: &str,
    version: &str,
    job_id: &str,
) -> Result<String, AppError> {
    validate_spec(tool, version)?;
    let spec = format!("{tool}@{version}");
    let mut command = Command::new(find_mise());
    command.args(["install", &spec]);
    if let Some(h) = home_dir() {
        command.current_dir(h);
    }
    let output = crate::process::run(
        &mut command,
        Duration::from_secs(3600),
        Some(job_id),
        |line| {
            let _ = app.emit(
                "mise:install-progress",
                InstallProgress {
                    job_id: job_id.into(),
                    tool: tool.into(),
                    version: version.into(),
                    line: line.into(),
                },
            );
        },
    )?;
    if output.status.success() {
        Ok(format!("{spec} 安装完成"))
    } else {
        Err(AppError::Other(format!(
            "{spec} 安装失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// 参数仍以独立 argv 传递；拒绝选项、控制字符与空请求。
pub fn validate_spec(tool: &str, version: &str) -> Result<(), AppError> {
    if [tool, version].iter().any(|s| {
        s.is_empty()
            || s.starts_with('-')
            || s.contains(char::is_whitespace)
            || s.contains(char::is_control)
    }) {
        return Err(AppError::Unsupported("工具名或版本请求无效".into()));
    }
    Ok(())
}

/// 卸载指定工具版本。
pub fn uninstall_version(tool: &str, version: &str) -> Result<String, AppError> {
    validate_spec(tool, version)?;
    let spec = format!("{}@{}", tool, version);
    run_mise(&["uninstall", &spec]).map(|_| format!("{} 卸载完成", spec))
}

/// 切换（激活）某个工具版本。global 为 true 时写入全局配置。
pub fn use_version(tool: &str, version: &str, global: bool) -> Result<String, AppError> {
    validate_spec(tool, version)?;
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
pub fn link_version(tool: &str, version: &str, path: &str) -> Result<String, AppError> {
    validate_spec(tool, version)?;
    crate::managed::adopt(tool, version, "external", path)
}

/// 解除接管：移除某版本与外部目录的链接。
/// mise 无 `unlink` 子命令（`unlink` 会被当作未知命令报“no tasks defined”）；
/// 对由 `link` 创建的 symlink，用 `mise uninstall` 即可移除链接而不删除外部目录。
pub fn unlink_version(tool: &str, version: &str) -> Result<String, AppError> {
    validate_spec(tool, version)?;
    crate::managed::unadopt(tool, version)
}

/// 已经由 project 服务校验、保存配置；这里只在明确的项目目录执行安装。
pub fn install_all_project(app: &AppHandle, path: &str, job: &str) -> Result<String, AppError> {
    let target = std::path::Path::new(path)
        .parent()
        .ok_or_else(|| AppError::Other("项目目录无效".into()))?;
    let mut command = Command::new(find_mise());
    command.arg("install").current_dir(target);
    let output = crate::process::run(&mut command, Duration::from_secs(3600), Some(job), |line| {
        let _ = app.emit(
            "mise:install-progress",
            InstallProgress {
                job_id: job.into(),
                tool: "project".into(),
                version: String::new(),
                line: line.into(),
            },
        );
    })?;
    if !output.status.success() {
        return Err(AppError::Other(format!(
            "项目安装失败：{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok("项目配置已保存，工具安装完成".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// scan_versions：有 bin/ 的一级子目录识别为版本，隐藏目录与无 bin/ 的跳过
    #[test]
    fn scan_versions_detects_tool_dirs_with_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("versions");
        // 合法：含 bin/ 的版本目录（version_fn 去掉 v 前缀）
        std::fs::create_dir_all(base.join("v20.0.0/bin")).unwrap();
        std::fs::write(base.join("v20.0.0/bin/node"), "").unwrap();
        // 非法：隐藏目录、无 bin/ 的目录
        std::fs::create_dir_all(base.join(".hidden/bin")).unwrap();
        std::fs::create_dir_all(base.join("no-bin")).unwrap();

        let mut out = Vec::new();
        scan_versions(&mut out, &base, "node", "test", |n| {
            n.trim_start_matches('v').to_string()
        });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool, "node");
        assert_eq!(out[0].version, "20.0.0");
        assert_eq!(out[0].manager, "test");
    }
}
