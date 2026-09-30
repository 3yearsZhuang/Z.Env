// 环境体检：对整机环境做主动巡检，输出结构化检查结果与修复建议。
// 与 managed.rs 的被动对账自愈互补——对账在运行时页扫描时触发，doctor 由用户显式全量巡检。
// 本模块只读不改状态：每项仅报告（level/detail）并给 hint，不执行任何修复动作。
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

/// 单项体检结果：id 供前端定位，title 为检查项名，detail 描述现状，hint 给出修复建议。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub title: String,
    pub level: DoctorLevel,
    pub detail: String,
    pub hint: String,
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
        }
    } else {
        DoctorCheck {
            id: "mise".into(),
            title: "mise 运行时管理器".into(),
            level: DoctorLevel::Fail,
            detail: "未检测到 mise，运行时管理与项目环境绑定均不可用".into(),
            hint: "执行 brew install mise，或参考 mise.jdx.dev/installing-mise.html 安装".into(),
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
        }
    } else {
        DoctorCheck {
            id: "path-duplicates".into(),
            title: "PATH 重复条目".into(),
            level: DoctorLevel::Warn,
            detail: format!("{} 个目录重复出现：{}", dups.len(), dups.join("、")),
            hint: "检查 shell 配置中重复的 export PATH 行，重复条目会拖慢命令查找".into(),
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
        }
    } else {
        DoctorCheck {
            id: "path-ghosts".into(),
            title: "PATH 失效目录".into(),
            level: DoctorLevel::Warn,
            detail: format!("{} 个目录已不存在：{}", ghosts.len(), ghosts.join("、")),
            hint: "多为已卸载工具的残留，可从 shell 配置或对应工具的 PATH 注入中移除".into(),
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
            }
        } else {
            DoctorCheck {
                id: "shell-integration".into(),
                title: "shell 集成".into(),
                level: DoctorLevel::Warn,
                detail: "shell 配置中未发现 mise 集成，终端里 mise 命令可能不生效".into(),
                hint: "在 shell 配置中追加 eval \"$(mise activate zsh)\"（按所用 shell 调整），\
                       或把 ~/.local/share/mise/shims 加入 PATH"
                    .into(),
            }
        }
    }
}

/// 托管接入健康度：失效接入交给运行时页的既有对账自愈，这里只报告并指引。
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
        }
    } else {
        DoctorCheck {
            id: "managed-health".into(),
            title: "托管接入健康度".into(),
            level: DoctorLevel::Warn,
            detail: format!("{} 个接入已失效：{}", broken.len(), broken.join("、")),
            hint: "打开「运行时工具」页会触发对账自愈，自动重连到最新版本或移除失效接入".into(),
        }
    }
}

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
}
