use serde::Serialize;
use std::process::Command;

/// 操作系统信息
#[derive(Serialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub arch: String,
}

/// 包管理器信息
#[derive(Serialize)]
pub struct PkgInfo {
    pub name: String,
    pub version: Option<String>,
    pub available: bool,
}

/// 环境汇总信息
#[derive(Serialize)]
pub struct EnvInfo {
    pub mise_version: Option<String>,
    pub git_version: Option<String>,
    pub os: OsInfo,
    pub pkg: Vec<PkgInfo>,
}

/// 运行命令并返回首行输出（失败或为空返回 None）。
fn out(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    if !o.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&o.stdout);
    s.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// 根据当前系统检测操作系统.
fn os_info() -> OsInfo {
    let arch = std::env::consts::ARCH.to_string();
    #[cfg(target_os = "macos")]
    {
        let name = out("sw_vers", &["-productName"]).unwrap_or_else(|| "macOS".into());
        let version = out("sw_vers", &["-productVersion"]).unwrap_or_default();
        return OsInfo { name, version, arch };
    }
    #[cfg(target_os = "linux")]
    {
        let body = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let mut name = "Linux".to_string();
        let mut version = String::new();
        for line in body.lines() {
            if let Some(v) = line.strip_prefix("NAME=") {
                name = v.trim_matches('"').to_string();
            }
            if let Some(v) = line.strip_prefix("VERSION_ID=") {
                version = v.trim_matches('"').to_string();
            }
        }
        return OsInfo { name, version, arch };
    }
    #[cfg(target_os = "windows")]
    {
        let mut name = "Windows".to_string();
        let mut version = String::new();
        if let Some(v) = out("cmd", &["/C", "ver"]) {
            version = v;
        }
        // 尝试从 (OS 注册表/CMD ver) 提取时，name 统一为 Windows
        name = "Windows".into();
        return OsInfo { name, version, arch };
    }
    #[allow(unreachable_code)]
    OsInfo { name: "Unknown".into(), version: String::new(), arch }
}

/// 采集一次性环境信息（mise/git/系统/各包管理器版本）。
pub fn collect_env_info() -> EnvInfo {
    let mise_version = out("mise", &["--version"]);
    let git_version = out("git", &["--version"]);
    let os = os_info();

    let mut pkg = Vec::new();
    for (name, arg) in [
        ("brew", "--version"),
        ("winget", "--version"),
        ("apt", "--version"),
        ("pacman", "--version"),
    ] {
        let v = out(name, &[arg]);
        pkg.push(PkgInfo { name: name.to_string(), version: v.clone(), available: v.is_some() });
    }

    EnvInfo { mise_version, git_version, os, pkg }
}