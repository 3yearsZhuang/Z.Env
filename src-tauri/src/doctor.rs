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

/// 托管接入修复：复用对账自愈（重连到同来源最新版本或显式移除失效接入）。
fn fix_managed() -> Result<String, AppError> {
    let events = crate::managed::reconcile();
    if events.is_empty() {
        return Ok("对账完成：没有需要处理的失效接入".into());
    }
    let relinked = events.iter().filter(|e| e.action == "relinked").count();
    let removed = events.iter().filter(|e| e.action == "removed").count();
    let failed = events.iter().filter(|e| e.action == "failed").count();
    let mut msg = format!("对账完成：重连 {relinked}、移除 {removed}、失败 {failed}");
    if let Some(f) = events.iter().find(|e| e.action == "failed") {
        msg.push_str(&format!("；失败详情：{}", f.message));
    }
    Ok(msg)
}

/// shell 集成修复的目标：（rc 相对路径, 待追加的集成行）。
#[cfg(not(windows))]
fn shell_fix_target(shell: &str) -> Option<(&'static str, String)> {
    match shell {
        "zsh" => Some((".zshrc", "eval \"$(mise activate zsh)\"".to_string())),
        "bash" => Some((".bashrc", "eval \"$(mise activate bash)\"".to_string())),
        "fish" => Some((
            ".config/fish/config.fish",
            "mise activate fish | source".to_string(),
        )),
        _ => None,
    }
}

/// shell 集成修复：按 $SHELL 把 mise activate 行**追加**到对应 rc（绝不改写既有内容）。
#[cfg(not(windows))]
fn fix_shell_integration() -> Result<String, AppError> {
    let shell = std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(String::from))
        .unwrap_or_default();
    let Some((rc, line)) = shell_fix_target(&shell) else {
        return Err(AppError::Unsupported(format!(
            "暂不支持的 shell: {shell}，请参考 mise 文档手动配置 activate"
        )));
    };
    // 写前复检：用户可能刚在别处配好
    let rcs = read_rc_contents();
    let rc_refs: Vec<&str> = rcs.iter().map(String::as_str).collect();
    let path_entries = split_path(&std::env::var("PATH").unwrap_or_default());
    if shell_integrated(&rc_refs, &path_entries) {
        return Ok("已存在 mise 的 shell 集成，无需修复".into());
    }
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| AppError::Io("无法定位主目录".into()))?;
    let rc_path = home.join(rc);
    let mut new_content = std::fs::read_to_string(&rc_path).unwrap_or_default();
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&format!("\n# Z.Env: mise 环境集成\n{line}\n"));
    if let Some(dir) = rc_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&rc_path, new_content)?;
    Ok(format!(
        "已向 {} 追加集成行（{}），重启终端或重新 source 后生效",
        rc_path.display(),
        line
    ))
}

/// Windows 不涉及 unix shell 集成修复。
#[cfg(windows)]
fn fix_shell_integration() -> Result<String, AppError> {
    Err(AppError::Unsupported(
        "Windows 平台不涉及 unix shell 集成".into(),
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

/// 读取 shell 配置文件内容（存在才读，缺失跳过）。
#[cfg(not(windows))]
fn read_rc_contents() -> Vec<String> {
    let home = match std::env::var_os("HOME") {
        Some(h) => std::path::PathBuf::from(h),
        None => return Vec::new(),
    };
    [
        ".zshrc",
        ".zprofile",
        ".bashrc",
        ".bash_profile",
        ".profile",
    ]
    .iter()
    .filter_map(|f| std::fs::read_to_string(home.join(f)).ok())
    .collect()
}

/// 检测 shell 是否已接入 mise：activate 钩子或 mise 的 shims 目录二选一即可。
/// 只认路径里含 mise 的 shims，避免 pyenv 等同名结构误判。
#[cfg(not(windows))]
fn shell_integrated(rc_contents: &[&str], path_entries: &[String]) -> bool {
    let activated = rc_contents.iter().any(|c| c.contains("mise activate"));
    let shims_in_path = path_entries
        .iter()
        .any(|p| p.contains("mise") && p.ends_with("shims"));
    activated || shims_in_path
}

fn check_shell_integration() -> DoctorCheck {
    // Windows 无 unix shell rc 的概念，报告为不适用（不作为问题呈现）。
    #[cfg(windows)]
    {
        return DoctorCheck {
            id: "shell-integration".into(),
            title: "shell 集成".into(),
            level: DoctorLevel::Ok,
            detail: "Windows 平台不适用，已跳过".into(),
            hint: String::new(),
            fixable: false,
        };
    }
    #[cfg(not(windows))]
    {
        let path_entries = split_path(&std::env::var("PATH").unwrap_or_default());
        let rcs = read_rc_contents();
        let rc_refs: Vec<&str> = rcs.iter().map(String::as_str).collect();
        if shell_integrated(&rc_refs, &path_entries) {
            DoctorCheck {
                id: "shell-integration".into(),
                title: "shell 集成".into(),
                level: DoctorLevel::Ok,
                detail: "已检测到 mise activate 钩子或 mise shims 目录".into(),
                hint: String::new(),
                fixable: false,
            }
        } else {
            DoctorCheck {
                id: "shell-integration".into(),
                title: "shell 集成".into(),
                level: DoctorLevel::Warn,
                detail: "shell 配置中未发现 mise 集成，终端里 mise 命令可能不生效".into(),
                hint: "一键修复会按当前 shell 把 mise activate 行追加到对应 rc 文件".into(),
                fixable: true,
            }
        }
    }
}

/// 托管接入健康度：失效接入可经对账自愈一键修复。
fn check_managed_health() -> DoctorCheck {
    let entries = crate::managed::list();
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
            hint: "一键修复会对账自愈：自动重连到最新版本或移除失效接入".into(),
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

    #[cfg(not(windows))]
    #[test]
    fn shell_integrated_detects_both_styles_without_false_positive() {
        let plain = ["/usr/bin".to_string()];
        assert!(!shell_integrated(&["export FOO=1"], &plain));
        assert!(shell_integrated(&["eval \"$(mise activate zsh)\""], &plain));

        let shims = vec![
            "/usr/bin".to_string(),
            "/home/u/.local/share/mise/shims".to_string(),
        ];
        assert!(shell_integrated(&[], &shims));

        // pyenv 的 shims 目录结构与 mise 同名，但路径不含 mise，不应误判
        let pyenv = ["/home/u/.pyenv/shims".to_string()];
        assert!(!shell_integrated(&[], &pyenv));
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

    #[cfg(not(windows))]
    #[test]
    fn shell_fix_target_maps_common_shells() {
        let (rc, line) = shell_fix_target("zsh").unwrap();
        assert_eq!(rc, ".zshrc");
        assert!(line.contains("mise activate zsh"));
        assert!(shell_fix_target("bash").is_some());
        assert_eq!(
            shell_fix_target("fish").unwrap().0,
            ".config/fish/config.fish"
        );
        assert!(shell_fix_target("pwsh").is_none());
    }
}
