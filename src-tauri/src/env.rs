use serde::Serialize;
use std::io::Read;
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

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

/// 在子线程运行命令并带超时返回输出，避免 winget/apt 等首启过慢导致界面卡顿。
fn run_limited(prog: &str, argv: Vec<String>, label: &str) -> Result<std::process::Output, String> {
    let (prog2, argv2) = (prog.to_string(), argv);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(Command::new(&prog2).args(&argv2).output());
    });
    rx.recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| format!("{} 执行超时（可能环境未就绪）", label))?
        .map_err(|e| format!("无法执行 {}: {}", prog, e))
}

/// 探测各系统包管理器当前已安装的软件列表（用于跨启动持久展示“已安装”状态）。
/// brew 用 `brew list --formula`；winget/apt/pacman 尽力解析首列名称。
pub fn detect_system_installed(manager: &str) -> Result<Vec<String>, String> {
    let (prog, argv): (String, Vec<String>) = match manager {
        "brew" => ("brew".to_string(), ["list", "--formula"].map(String::from).to_vec()),
        "winget" => (
            "winget".to_string(),
            ["list", "--disable-interactivity"].map(String::from).to_vec(),
        ),
        "apt" => ("apt".to_string(), ["list", "--installed"].map(String::from).to_vec()),
        "pacman" => ("pacman".to_string(), ["-Q"].map(String::from).to_vec()),
        _ => return Err(format!("不支持的包管理器: {}", manager)),
    };
    let output = run_limited(&prog, argv, manager)?;
    if !output.status.success() {
        return Err(format!("{} 查询已安装列表失败", manager));
    }
    let out = String::from_utf8_lossy(&output.stdout);
    let mut names: Vec<String> = Vec::new();
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        // apt 格式为 "pkg/arch version ..."，取 '/' 前部分；其余取首个空白分隔词
        let word = if manager == "apt" {
            t.split('/').next().unwrap_or("").trim()
        } else {
            t.split_whitespace().next().unwrap_or("").trim()
        };
        let name = word.to_string();
        if name.is_empty() {
            continue;
        }
        // winget 输出前几行是列标题，跳过常见标题词
        if matches!(
            name.as_str(),
            "名称" | "Name" | "Id" | "Version" | "Available" | "package"
        ) {
            continue;
        }
        names.push(name);
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// 系统包管理器已装的一个软件及其版本。
#[derive(Debug, Clone, Serialize)]
pub struct SystemPkg {
    pub name: String,
    pub version: Option<String>,
}

/// 探测各系统包管理器已装软件及其版本（供运行时页列出“其他渠道”的版本）。
/// brew 用 `brew list --versions`；winget/apt/pacman 尽力解析名称与版本列。
pub fn detect_system_versions(manager: &str) -> Result<Vec<SystemPkg>, String> {
    let (prog, argv): (String, Vec<String>) = match manager {
        "brew" => ("brew".to_string(), ["list", "--versions"].map(String::from).to_vec()),
        "winget" => (
            "winget".to_string(),
            ["list", "--disable-interactivity"].map(String::from).to_vec(),
        ),
        "apt" => ("apt".to_string(), ["list", "--installed"].map(String::from).to_vec()),
        "pacman" => ("pacman".to_string(), ["-Q"].map(String::from).to_vec()),
        _ => return Err(format!("不支持的包管理器: {}", manager)),
    };
    let output = run_limited(&prog, argv, manager)?;
    if !output.status.success() {
        return Err(format!("{} 查询已装版本失败", manager));
    }
    let out = String::from_utf8_lossy(&output.stdout);
    let mut pkgs: Vec<SystemPkg> = Vec::new();
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = t.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let (name, version) = match manager {
            // apt："pkg/arch version ..." → 名称取 '/' 前，版本取第二列
            "apt" => (t.split('/').next().unwrap_or("").trim().to_string(), tokens.get(1).map(|s| s.to_string())),
            // winget：列次序 [名称, Id, 版本, ...]，跳过列标题
            "winget" => {
                let n = tokens[0].to_string();
                if matches!(n.as_str(), "名称" | "Name") {
                    continue;
                }
                (n, tokens.get(2).map(|s| s.to_string()))
            }
            // brew / pacman："名称 版本 ..."
            _ => (tokens[0].to_string(), tokens.get(1).map(|s| s.to_string())),
        };
        if name.is_empty() {
            continue;
        }
        pkgs.push(SystemPkg { name, version });
    }
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    pkgs.dedup_by(|a, b| a.name == b.name);
    Ok(pkgs)
}

/// 原生卸载系统软件包（brew/winget/apt/pacman），用于管理其他渠道安装的环境。
pub fn uninstall_system_package(manager: &str, name: &str) -> Result<String, String> {
    let mut prog = manager.to_string();
    let mut argv: Vec<String> = Vec::new();
    match manager {
        "brew" => argv.extend(["uninstall", name].map(String::from)),
        "winget" => argv.extend(["uninstall", "--id", name].map(String::from)),
        "apt" => {
            argv.extend(["remove", "-y", name].map(String::from));
            prog = "sudo".into();
        }
        "pacman" => {
            argv.extend(["-R", "--noconfirm", name].map(String::from));
            prog = "sudo".into();
        }
        _ => return Err(format!("不支持的包管理器: {}", manager)),
    }
    let output = run_limited(&prog, argv, &format!("{manager} uninstall"))?;
    if output.status.success() {
        Ok(format!("{} 已卸载", name))
    } else {
        Err(format!("{} 卸载失败：{}", name, String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// 原生安装进度事件负载，通过 `sys:install-progress` 事件推送给前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SysInstallProgress {
    pub manager: String,
    pub name: String,
    pub line: String,
}

/// 当前用户主目录（GUI 从 Finder/Dock 启动时 cwd 可能不可写，统一以 home 为工作目录）。
fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

/// 原生安装系统软件包：调用当前系统对应的包管理器真实安装，并把输出逐帧推送给前端。
/// apt / pacman 需要管理员权限，自动用 `sudo` 前置（GUI 场景下可能要求输入密码）。
pub fn install_system_package(
    app: &AppHandle,
    manager: &str,
    name: &str,
) -> Result<String, String> {
    let mut prog = manager.to_string();
    let mut argv: Vec<String> = Vec::new();
    match manager {
        "brew" => argv.extend(["install", name].map(String::from)),
        "winget" => argv.extend(
            ["install", "--accept-source-agreements", "--accept-package-agreements", name]
                .map(String::from),
        ),
        "apt" => {
            argv.extend(["install", "-y", name].map(String::from));
            prog = "sudo".into();
        }
        "pacman" => {
            argv.extend(["-S", "--noconfirm", name].map(String::from));
            prog = "sudo".into();
        }
        _ => return Err(format!("不支持的包管理器: {}", manager)),
    }

    let mut command = Command::new(&prog);
    command.args(&argv).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(h) = home() {
        command.current_dir(h);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动 {} 安装: {}", prog, e))?;

    let stdout = child.stdout.take().expect("stdout pipe");
    let stderr = child.stderr.take().expect("stderr pipe");
    let manager_out = manager.to_string();
    let name_out = name.to_string();
    let app_out = app.clone();
    let t_out = std::thread::spawn(move || {
        pump_sys_stream(stdout, &app_out, &manager_out, &name_out);
    });
    let manager_err = manager.to_string();
    let name_err = name.to_string();
    let app_err = app.clone();
    let t_err = std::thread::spawn(move || {
        pump_sys_stream(stderr, &app_err, &manager_err, &name_err);
    });

    let status = child
        .wait()
        .map_err(|e| format!("等待 {} 进程失败: {}", prog, e))?;
    let _ = t_out.join();
    let _ = t_err.join();

    if status.success() {
        Ok(format!("{} install {}", manager, name))
    } else {
        Err(format!("{} install {} 失败", manager, name))
    }
}

/// 逐字节读取子进程输出，按换行切分后推送 `sys:install-progress` 事件。
fn pump_sys_stream<R: Read>(mut reader: R, app: &AppHandle, manager: &str, name: &str) {
    let mut byte = [0u8; 1];
    let mut line = String::new();
    let flush = |line: &mut String| {
        if !line.trim().is_empty() {
            let _ = app.emit(
                "sys:install-progress",
                SysInstallProgress {
                    manager: manager.to_string(),
                    name: name.to_string(),
                    line: line.trim_end().to_string(),
                },
            );
        }
        line.clear();
    };
    loop {
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                let c = byte[0] as char;
                if c == '\n' || c == '\r' {
                    flush(&mut line);
                } else {
                    line.push(c);
                }
            }
            Err(_) => break,
        }
    }
    flush(&mut line);
}