use crate::error::AppError;
use serde::Serialize;
use std::process::Command;
use std::time::Duration;
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
    let o = crate::process::output(Command::new(cmd).args(args), Duration::from_secs(15)).ok()?;
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
        return OsInfo {
            name,
            version,
            arch,
        };
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
        return OsInfo {
            name,
            version,
            arch,
        };
    }
    #[cfg(target_os = "windows")]
    {
        let name = "Windows".to_string();
        let mut version = String::new();
        if let Some(v) = out("cmd", &["/C", "ver"]) {
            version = v;
        }
        return OsInfo {
            name,
            version,
            arch,
        };
    }
    #[allow(unreachable_code)]
    OsInfo {
        name: "Unknown".into(),
        version: String::new(),
        arch,
    }
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
        pkg.push(PkgInfo {
            name: name.to_string(),
            version: v.clone(),
            available: v.is_some(),
        });
    }

    EnvInfo {
        mise_version,
        git_version,
        os,
        pkg,
    }
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
pub struct ProviderPackage {
    pub tool: String,
    pub manager: String,
    pub id: String,
    pub kind: String,
    pub platforms: Vec<String>,
}

pub fn package_catalog() -> Vec<ProviderPackage> {
    let all: Vec<ProviderPackage> =
        serde_json::from_str(include_str!("../../src/data/packages.json"))
            .expect("内置包映射必须有效");
    all.into_iter()
        .filter(|p| p.platforms.iter().any(|s| s == std::env::consts::OS))
        .collect()
}

pub fn validate_package(name: &str) -> Result<(), AppError> {
    if name.is_empty()
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.+@/".contains(c))
    {
        return Err(AppError::Unsupported("软件包标识无效".into()));
    }
    Ok(())
}

/// 身份始终保留包管理器的规范 ID，不用显示名首词代替。
#[derive(Debug, Clone, Serialize)]
pub struct SystemPkg {
    pub name: String,
    pub version: Option<String>,
    pub kind: String,
    pub tool: Option<String>,
}

fn identify(manager: &str, name: &str, version: Option<String>, kind: &str) -> SystemPkg {
    let tool = package_catalog()
        .into_iter()
        .find(|p| p.manager == manager && p.id.eq_ignore_ascii_case(name))
        .map(|p| p.tool);
    SystemPkg {
        name: name.into(),
        version,
        kind: kind.into(),
        tool,
    }
}

fn checked_output(command: &mut Command) -> Result<std::process::Output, AppError> {
    let out = crate::process::output(command, Duration::from_secs(120))?;
    if !out.status.success() {
        return Err(AppError::Other(
            String::from_utf8_lossy(&out.stderr).trim().into(),
        ));
    }
    Ok(out)
}

pub fn detect_system_installed(manager: &str) -> Result<Vec<String>, AppError> {
    Ok(detect_system_versions(manager)?
        .into_iter()
        .map(|p| p.name)
        .collect())
}

pub fn detect_system_versions(manager: &str) -> Result<Vec<SystemPkg>, AppError> {
    match manager {
        "brew" => {
            let out =
                checked_output(Command::new("brew").args(["info", "--json=v2", "--installed"]))?;
            parse_brew_installed(&String::from_utf8_lossy(&out.stdout))
        }
        "winget" => {
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("installed.json");
            checked_output(
                Command::new("winget")
                    .args([
                        "export",
                        "--include-versions",
                        "--accept-source-agreements",
                        "--disable-interactivity",
                        "--output",
                    ])
                    .arg(&file),
            )?;
            parse_winget_export(&std::fs::read_to_string(file)?)
        }
        "apt" => {
            let out = checked_output(Command::new("dpkg-query").args([
                "-W",
                "-f=${binary:Package}\t${Version}\t${db:Status-Status}\n",
            ]))?;
            Ok(String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| {
                    let v: Vec<_> = l.split('\t').collect();
                    if v.len() != 3 || v[2] != "installed" {
                        return None;
                    }
                    Some(identify(
                        manager,
                        v[0].split(':').next()?,
                        Some(v[1].into()),
                        "package",
                    ))
                })
                .collect())
        }
        "pacman" => {
            let out = checked_output(Command::new("pacman").arg("-Q"))?;
            Ok(String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| {
                    let mut v = l.split_whitespace();
                    Some(identify(
                        manager,
                        v.next()?,
                        v.next().map(str::to_string),
                        "package",
                    ))
                })
                .collect())
        }
        _ => Err(AppError::Unsupported(format!(
            "不支持的包管理器: {manager}"
        ))),
    }
}

pub fn parse_brew_installed(text: &str) -> Result<Vec<SystemPkg>, AppError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| AppError::Parse(e.to_string()))?;
    let mut out = Vec::new();
    for (section, field, kind) in [
        ("formulae", "full_name", "formula"),
        ("casks", "token", "cask"),
    ] {
        let items = value[section]
            .as_array()
            .ok_or_else(|| AppError::Parse(format!("brew 输出缺少 {section}")))?;
        for item in items {
            let Some(name) = item[field].as_str().or_else(|| item["name"].as_str()) else {
                continue;
            };
            let version = if kind == "formula" {
                item["installed"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v["version"].as_str())
            } else {
                item["installed"].as_str().or_else(|| {
                    item["installed"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                })
            };
            if let Some(version) = version {
                out.push(identify("brew", name, Some(version.into()), kind));
            }
        }
    }
    Ok(out)
}

fn parse_winget_export(text: &str) -> Result<Vec<SystemPkg>, AppError> {
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let sources = value["Sources"]
        .as_array()
        .ok_or_else(|| AppError::Parse("winget 导出缺少 Sources".into()))?;
    let mut out = Vec::new();
    for source in sources {
        for pkg in source["Packages"].as_array().into_iter().flatten() {
            if let Some(id) = pkg["PackageIdentifier"].as_str() {
                out.push(identify(
                    "winget",
                    id,
                    pkg["Version"].as_str().map(str::to_string),
                    "package",
                ));
            }
        }
    }
    Ok(out)
}

/// Linux 使用桌面授权代理；不通过无终端的 sudo 请求密码。
fn package_command(
    manager: &str,
    name: &str,
    kind: &str,
    install: bool,
) -> Result<Command, AppError> {
    validate_package(name)?;
    let mut command = match manager {
        "brew" => {
            let mut c = Command::new("brew");
            c.arg(if install { "install" } else { "uninstall" });
            if !matches!(kind, "formula" | "cask") {
                return Err(AppError::Unsupported("brew 包类型无效".into()));
            }
            c.arg(format!("--{kind}")).arg("--").arg(name);
            c
        }
        "winget" => {
            let mut c = Command::new("winget");
            c.args([
                if install { "install" } else { "uninstall" },
                "--id",
                name,
                "--exact",
                "--disable-interactivity",
                "--accept-source-agreements",
            ]);
            if install {
                c.arg("--accept-package-agreements");
            }
            c
        }
        "apt" | "pacman" => {
            let mut c = Command::new("pkexec");
            let executable = if manager == "apt" {
                "/usr/bin/apt-get"
            } else {
                "/usr/bin/pacman"
            };
            c.arg(executable);
            if manager == "apt" {
                c.args([if install { "install" } else { "remove" }, "-y", "--", name]);
            } else {
                c.args([if install { "-S" } else { "-R" }, "--noconfirm", "--", name]);
            }
            c
        }
        _ => {
            return Err(AppError::Unsupported(format!(
                "不支持的包管理器: {manager}"
            )))
        }
    };
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        command.current_dir(home);
    }
    Ok(command)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SysInstallProgress {
    pub job_id: String,
    pub manager: String,
    pub name: String,
    pub line: String,
}

pub fn install_system_package(
    app: &AppHandle,
    manager: &str,
    name: &str,
    kind: &str,
    job: &str,
) -> Result<String, AppError> {
    if !package_catalog()
        .iter()
        .any(|p| p.manager == manager && p.id == name && p.kind == kind)
    {
        return Err(AppError::Unsupported(
            "当前平台未提供该软件的安装映射，请使用官方下载渠道".into(),
        ));
    }
    let mut command = package_command(manager, name, kind, true)?;
    let out = crate::process::run(&mut command, Duration::from_secs(3600), Some(job), |line| {
        let _ = app.emit(
            "sys:install-progress",
            SysInstallProgress {
                job_id: job.into(),
                manager: manager.into(),
                name: name.into(),
                line: line.into(),
            },
        );
    })
    .map_err(|e| {
        AppError::Other(format!(
            "{e}{}",
            if matches!(manager, "apt" | "pacman") {
                "；Linux 需要 polkit/pkexec 和可用的桌面授权会话"
            } else {
                ""
            }
        ))
    })?;
    if !out.status.success() {
        return Err(AppError::Other(format!(
            "安装失败：{}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let installed = detect_system_installed(manager)?;
    if !installed.iter().any(|id| id.eq_ignore_ascii_case(name)) {
        return Err(AppError::Other(
            "安装命令已结束，但尚未检测到该包，请刷新检查".into(),
        ));
    }
    Ok(format!("{name} 已安装并验证"))
}

pub fn uninstall_system_package(manager: &str, name: &str) -> Result<String, AppError> {
    let pkgs = detect_system_versions(manager)?;
    let pkg = pkgs
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| AppError::Other("未找到指定安装包，已取消卸载".into()))?;
    let mut command = package_command(manager, &pkg.name, &pkg.kind, false)?;
    let out = crate::process::output(&mut command, Duration::from_secs(600))?;
    if !out.status.success() {
        return Err(AppError::Other(format!(
            "卸载失败：{}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(format!("{name} 已卸载"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_commands_include_manager_and_authorization() {
        for (manager, executable, verb) in [
            ("apt", "/usr/bin/apt-get", "install"),
            ("pacman", "/usr/bin/pacman", "-S"),
        ] {
            let cmd = package_command(manager, "nodejs", "package", true).unwrap();
            assert_eq!(cmd.get_program(), "pkexec");
            let args: Vec<_> = cmd.get_args().map(|s| s.to_str().unwrap()).collect();
            assert_eq!(&args[..2], &[executable, verb]);
            assert!(package_command(manager, "--root", "package", false).is_err());
        }
        let cmd = package_command("apt", "nodejs", "package", false).unwrap();
        assert_eq!(cmd.get_args().nth(1).unwrap(), "remove");
    }
    #[test]
    fn brew_retains_casks_and_formula_identity() {
        let parsed = parse_brew_installed(r#"{"formulae":[{"full_name":"node@20","installed":[{"version":"20.1"}]}],"casks":[{"token":"docker-desktop","installed":"4.0"}]}"#).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "node@20");
        assert_eq!(parsed[1].kind, "cask");
    }
    #[test]
    fn winget_uses_identifier_not_localized_display_name() {
        let parsed = parse_winget_export(r#"{"Sources":[{"Packages":[{"PackageIdentifier":"Microsoft.VisualStudioCode","Version":"1.0"}]}]}"#).unwrap();
        assert_eq!(parsed[0].name, "Microsoft.VisualStudioCode");
    }
    #[test]
    fn catalog_ids_are_safe_and_unique_per_platform() {
        let rows: Vec<ProviderPackage> =
            serde_json::from_str(include_str!("../../src/data/packages.json")).unwrap();
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            validate_package(&row.id).unwrap();
            assert!(seen.insert((row.tool, row.manager)));
        }
    }
}
