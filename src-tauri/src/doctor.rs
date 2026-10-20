// 环境体检：对整机环境做主动巡检，输出结构化检查结果与修复建议。
// 与 managed.rs 的被动对账自愈互补——对账在运行时页扫描时触发，doctor 由用户显式全量巡检。
// v1 起带修复动作（doctor::fix）：仅覆盖托管对账与 shell 集成两项；PATH 等涉及用户
// 手写配置的项只报告不动手，避免应用替用户改配置。
use serde::Serialize;

/// 检查结论等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DoctorLevel {
    /// 正常或平台不适用。
    Ok,
    /// 有问题但功能仍可用。
    Warn,
    /// 关键依赖缺失，核心功能不可用。
    Fail,
}

/// 单项体检结果：id 供前端定位，title 为检查项名，detail 描述现状，hint 给出修复建议；
/// fixable 表示该项可经 doctor::fix 一键修复（前端据此显示修复按钮）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub title: String,
    pub level: DoctorLevel,
    pub detail: String,
    pub hint: String,
    pub fixable: bool,
}

/// 执行全部体检项，返回固定顺序的结果列表。
pub fn run() -> Vec<DoctorCheck> {
    vec![
        check_mise(),
        check_path_duplicates(),
        check_path_ghosts(),
        check_shell_integration(),
        check_managed_health(),
    ]
}

/// 执行一个检查项的修复动作。v1 仅覆盖两项可自动修复：托管接入对账、shell 集成追加。
pub fn fix(id: &str) -> Result<String, AppError> {
    match id {
        "managed-health" => fix_managed(),
        "shell-integration" => fix_shell_integration(),
        other => Err(AppError::Unsupported(format!(
            "检查项 {other} 暂不支持自动修复，请按建议手动处理"
        ))),
    }
}

/// 托管接入修复：复用对账自愈（只修复同来源同版本，缺失的精确版本保留待处理）。
fn fix_managed() -> Result<String, AppError> {
    let events = crate::managed::reconcile()?;
    if events.is_empty() {
        return Ok("对账完成：没有需要处理的失效接入".into());
    }
    let relinked = events.iter().filter(|e| e.action == "relinked").count();
    let pending = events.iter().filter(|e| e.action == "needs-review").count();
    let failed = events.iter().filter(|e| e.action == "failed").count();
    let mut msg = format!("对账完成：重连 {relinked}、待处理 {pending}、失败 {failed}");
    if let Some(f) = events.iter().find(|e| e.action == "failed") {
        msg.push_str(&format!("；失败详情：{}", f.message));
    }
    Ok(msg)
}

/// 按实际 shell 选择配置文件，支持 fish、自定义 ZDOTDIR/XDG 与 PowerShell。
fn shell_target() -> Result<(std::path::PathBuf, String), AppError> {
    #[cfg(not(windows))]
    let mise = crate::mise::find_mise()
        .display()
        .to_string()
        .replace('\'', "'\\''");
    #[cfg(windows)]
    {
        for program in ["pwsh", "powershell"] {
            let result = crate::process::output(
                std::process::Command::new(program).args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "[Console]::Write($PROFILE.CurrentUserAllHosts)",
                ]),
                std::time::Duration::from_secs(10),
            );
            if let Ok(output) = result {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        let mise = crate::mise::find_mise()
                            .display()
                            .to_string()
                            .replace('\'', "''");
                        return Ok((
                            path.into(),
                            format!("& '{mise}' activate pwsh | Out-String | Invoke-Expression"),
                        ));
                    }
                }
            }
        }
        Err(AppError::Unsupported(
            "未发现 PowerShell，无法配置 shell 集成".into(),
        ))
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_default();
        let shell = shell.rsplit('/').next().unwrap_or("");
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| AppError::Io("无法定位主目录".into()))?;
        let (path, line) = match shell {
            "zsh" => (
                std::env::var_os("ZDOTDIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or(home)
                    .join(".zshrc"),
                format!("eval \"$('{mise}' activate zsh)\""),
            ),
            "bash" => (
                home.join(".bashrc"),
                format!("eval \"$('{mise}' activate bash)\""),
            ),
            "fish" => (
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| home.join(".config"))
                    .join("fish/config.fish"),
                format!("'{mise}' activate fish | source"),
            ),
            _ => return Err(AppError::Unsupported(format!("暂不支持的 shell: {shell}"))),
        };
        Ok((path, line))
    }
}

fn append_integration(content: &str, line: &str) -> String {
    if shell_integrated(&[content], &[]) {
        return content.into();
    }
    format!("{}\n# Z.Env: mise 环境集成\n{line}\n", content.trim_end())
}

fn fix_shell_integration() -> Result<String, AppError> {
    let (path, line) = shell_target()?;
    crate::config::update_text(&path, None, |s| Ok(append_integration(s, &line)))?;
    Ok(format!(
        "已检查并配置 {}；打开新终端后验证生效",
        path.display()
    ))
}

/// mise 是本应用所有运行时管理能力的前提，缺失记为 Fail。
fn check_mise() -> DoctorCheck {
    let s = crate::mise::mise_status();
    if s.installed {
        let version = s
            .version
            .as_deref()
            .map(|v| format!("（{v}）"))
            .unwrap_or_default();
        DoctorCheck {
            id: "mise".into(),
            title: "mise 运行时管理器".into(),
            level: DoctorLevel::Ok,
            detail: format!("已安装{version}"),
            hint: String::new(),
            fixable: false,
        }
    } else {
        DoctorCheck {
            id: "mise".into(),
            title: "mise 运行时管理器".into(),
            level: DoctorLevel::Fail,
            detail: "未检测到 mise，运行时管理与项目环境绑定均不可用".into(),
            hint: "执行 brew install mise，或参考 mise.jdx.dev/installing-mise.html 安装".into(),
            fixable: false,
        }
    }
}

/// 解析 PATH 环境变量为条目列表（Windows 用 ';' 分隔，其余 ':'），剔除空段。
fn split_path(raw: &str) -> Vec<String> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    raw.split(sep)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// PATH 中重复出现的条目（常见于多份 shell 配置重复追加）。
fn path_duplicates(entries: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::with_capacity(entries.len());
    let mut dup = std::collections::HashSet::new();
    for e in entries {
        if !seen.insert(e.as_str()) {
            dup.insert(e.clone());
        }
    }
    let mut v: Vec<String> = dup.into_iter().collect();
    v.sort();
    v
}

/// PATH 中已不存在的目录（已卸载工具的残留，拖慢命令查找且易引发误解析）。
fn path_ghosts(entries: &[String]) -> Vec<String> {
    let mut v: Vec<String> = entries
        .iter()
        .filter(|e| !std::path::Path::new(&e).exists())
        .cloned()
        .collect();
    v.sort();
    v.dedup();
    v
}

fn check_path_duplicates() -> DoctorCheck {
    let entries = split_path(&std::env::var("PATH").unwrap_or_default());
    let dups = path_duplicates(&entries);
    if dups.is_empty() {
        DoctorCheck {
            id: "path-duplicates".into(),
            title: "PATH 重复条目".into(),
            level: DoctorLevel::Ok,
            detail: "无重复条目".into(),
            hint: String::new(),
            fixable: false,
        }
    } else {
        DoctorCheck {
            id: "path-duplicates".into(),
            title: "PATH 重复条目".into(),
            level: DoctorLevel::Warn,
            detail: format!("{} 个目录重复出现：{}", dups.len(), dups.join("、")),
            hint: "检查 shell 配置中重复的 export PATH 行，重复条目会拖慢命令查找".into(),
            fixable: false,
        }
    }
}

fn check_path_ghosts() -> DoctorCheck {
    let entries = split_path(&std::env::var("PATH").unwrap_or_default());
    let ghosts = path_ghosts(&entries);
    if ghosts.is_empty() {
        DoctorCheck {
            id: "path-ghosts".into(),
            title: "PATH 失效目录".into(),
            level: DoctorLevel::Ok,
            detail: "PATH 中所有目录均存在".into(),
            hint: String::new(),
            fixable: false,
        }
    } else {
        DoctorCheck {
            id: "path-ghosts".into(),
            title: "PATH 失效目录".into(),
            level: DoctorLevel::Warn,
            detail: format!("{} 个目录已不存在：{}", ghosts.len(), ghosts.join("、")),
            hint: "多为已卸载工具的残留，可从 shell 配置或对应工具的 PATH 注入中移除".into(),
            fixable: false,
        }
    }
}

/// 只识别非注释的执行行；配置存在与真正执行可用分开报告。
fn shell_integrated(rc_contents: &[&str], _path_entries: &[String]) -> bool {
    rc_contents.iter().flat_map(|s| s.lines()).any(|line| {
        let line = line.trim();
        !line.starts_with('#')
            && !line.starts_with("echo ")
            && !line.starts_with("printf ")
            && line.contains("mise")
            && line.contains(" activate ")
            && (line.starts_with("eval ")
                || line.ends_with("| source")
                || line.ends_with("| Invoke-Expression"))
    })
}

fn check_shell_integration() -> DoctorCheck {
    let result = shell_target().and_then(|(path, _)| {
        let content = crate::config::read_optional(&path)?.unwrap_or_default();
        Ok((path, shell_integrated(&[&content], &[])))
    });
    match result {
        Ok((path, true)) => DoctorCheck {
            id: "shell-integration".into(),
            title: "shell 集成配置".into(),
            level: DoctorLevel::Ok,
            detail: format!(
                "{} 中存在激活配置；当前检查不执行用户 shell 脚本",
                path.display()
            ),
            hint: "请在新终端检查工具路径与版本，确认实际生效".into(),
            fixable: false,
        },
        Ok((path, false)) => DoctorCheck {
            id: "shell-integration".into(),
            title: "shell 集成配置".into(),
            level: DoctorLevel::Warn,
            detail: format!("{} 尚未配置激活", path.display()),
            hint: "可追加集成并保留原文件备份".into(),
            fixable: true,
        },
        Err(error) => DoctorCheck {
            id: "shell-integration".into(),
            title: "shell 集成配置".into(),
            level: DoctorLevel::Warn,
            detail: error.to_string(),
            hint: "请选择受支持的 shell 后重试".into(),
            fixable: false,
        },
    }
}

/// 托管接入健康度：失效接入可经对账自愈一键修复。
fn check_managed_health() -> DoctorCheck {
    let entries = match crate::managed::list() {
        Ok(entries) => entries,
        Err(error) => {
            return DoctorCheck {
                id: "managed-health".into(),
                title: "托管清单".into(),
                level: DoctorLevel::Fail,
                detail: error.to_string(),
                hint: "请先恢复清单备份，不会自动覆盖损坏文件".into(),
                fixable: false,
            }
        }
    };
    let broken: Vec<String> = entries
        .iter()
        .filter(|e| !e.healthy)
        .map(|e| format!("{}/{}", e.tool, e.version))
        .collect();
    if broken.is_empty() {
        DoctorCheck {
            id: "managed-health".into(),
            title: "托管接入健康度".into(),
            level: DoctorLevel::Ok,
            detail: "全部接入有效".into(),
            hint: String::new(),
            fixable: false,
        }
    } else {
        DoctorCheck {
            id: "managed-health".into(),
            title: "托管接入健康度".into(),
            level: DoctorLevel::Warn,
            detail: format!("{} 个接入已失效：{}", broken.len(), broken.join("、")),
            hint: "一键修复只重连同软件同版本；来源消失时保留绑定并提示人工处理".into(),
            fixable: true,
        }
    }
}

use crate::error::AppError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_path_uses_platform_separator_and_trims() {
        if cfg!(windows) {
            assert_eq!(
                split_path(" C:\\a ; C:\\b;;C:\\a"),
                vec!["C:\\a", "C:\\b", "C:\\a"]
            );
        } else {
            assert_eq!(split_path(" /a : /b ::/a"), vec!["/a", "/b", "/a"]);
        }
    }

    #[test]
    fn path_duplicates_reports_only_repeats() {
        let e: Vec<String> = ["/a", "/b", "/a", "/a"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(path_duplicates(&e), vec!["/a"]);
        let e2: Vec<String> = ["/a", "/b"].iter().map(|s| s.to_string()).collect();
        assert!(path_duplicates(&e2).is_empty());
    }

    #[test]
    fn path_ghosts_detects_missing_dirs_only() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().to_str().unwrap().to_string();
        let ghost = "/zenv-doctor-ghost-should-not-exist".to_string();
        let e = vec![real, ghost.clone()];
        assert_eq!(path_ghosts(&e), vec![ghost]);
    }

    #[test]
    fn commented_or_other_shell_configs_do_not_count_as_integration() {
        assert!(!shell_integrated(&["# eval \"$(mise activate zsh)\""], &[]));
        assert!(!shell_integrated(&["echo 'mise activate zsh'"], &[]));
        assert!(shell_integrated(&["eval \"$(mise activate zsh)\""], &[]));
        let fish = append_integration("# existing", "mise activate fish | source");
        assert_eq!(
            append_integration(&fish, "mise activate fish | source"),
            fish
        );
        assert!(shell_integrated(
            &["& 'C:/mise.exe' activate pwsh | Out-String | Invoke-Expression"],
            &[]
        ));
    }

    #[test]
    fn fix_rejects_unknown_and_readonly_items() {
        // PATH 类项目有意不做自动修复
        for id in ["path-duplicates", "path-ghosts", "mise", "no-such-item"] {
            assert!(
                matches!(fix(id), Err(AppError::Unsupported(_))),
                "{id} 应返回 Unsupported"
            );
        }
    }
}
