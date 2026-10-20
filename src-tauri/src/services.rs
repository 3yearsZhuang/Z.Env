// 本地开发服务管理：brew services（macOS / Linuxbrew）与 systemd 用户级服务（Linux）。
// 只暴露常规启停与重启，不做 enable/disable 等开机级变更；Windows 服务暂缺实机验证，
// 列表处以说明替代。JSON / 文本解析均为纯函数，便于单测。
use crate::error::AppError;
use serde::Serialize;

/// 单个服务。state 取 brew 的 started/stopped/error/unknown/scheduled 或 systemd 的
/// enabled/disabled/indirect/static 等（原样透传给前端展示）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub name: String,
    pub state: String,
    /// 管理来源：brew | systemd
    pub manager: String,
    pub pid: Option<u64>,
}

/// 服务总览：列表 + 平台说明（如 Windows 暂不支持）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceOverview {
    pub services: Vec<ServiceInfo>,
    pub notes: Vec<String>,
}

/// 聚合本机可管理的服务列表（brew / systemd 均探测，缺哪个跳哪个）。
pub fn list() -> ServiceOverview {
    let mut services = Vec::new();
    let notes: Vec<String> = if cfg!(target_os = "windows") {
        vec!["Windows 服务管理将在有实机验证后提供，当前版本不列出系统服务。".to_string()]
    } else {
        Vec::new()
    };

    if let Some(json) = quiet_output("brew", &["services", "list", "--json"]) {
        services.extend(parse_brew_services(&json));
    }
    #[cfg(not(windows))]
    if let Some(text) = quiet_output(
        "systemctl",
        &[
            "--user",
            "list-units",
            "--all",
            "--type=service",
            "--no-legend",
            "--no-pager",
        ],
    ) {
        services.extend(parse_systemd_units(&text));
    }

    ServiceOverview { services, notes }
}

/// 对服务执行操作：brew 走 `brew services <act> <name>`；systemd 走 `systemctl --user <act> <name>`。
pub fn action(manager: &str, name: &str, act: &str) -> Result<String, AppError> {
    if !matches!(act, "start" | "stop" | "restart") {
        return Err(AppError::Unsupported(format!("不支持的服务操作: {act}")));
    }
    validate_unit_name(name)?;
    let mut command = match manager {
        "brew" => {
            let mut c = std::process::Command::new("brew");
            c.args(["services", act, name]);
            c
        }
        "systemd" => {
            let mut c = std::process::Command::new("systemctl");
            c.args(["--user", act, name]);
            c
        }
        _ => {
            return Err(AppError::Unsupported(format!(
                "不支持的服务来源: {manager}"
            )))
        }
    };
    let out = crate::process::output(&mut command, std::time::Duration::from_secs(60))?;
    if !out.status.success() {
        return Err(AppError::Other(format!(
            "服务操作失败：{}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(format!("已 {act} {name}（{manager}）"))
}

/// systemd 单元名白名单校验：字母数字与 -_.@ 组成，且不含路径分隔。
/// 纯字符串校验，不随平台裁剪——`action()` 的 systemd 分支在所有平台都参与编译，
/// Windows 下若被 cfg 掉会在 release 构建时报 not found（CI 只在 Linux 跑，查不出来）。
fn validate_unit_name(name: &str) -> Result<(), AppError> {
    let ok = !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.@".contains(c));
    if ok {
        Ok(())
    } else {
        Err(AppError::Unsupported(format!("非法单元名: {name}")))
    }
}

/// 解析 `brew services list --json`。防御式解析：新版本字段 running(bool)/pid，
/// 旧版本可能带 state 字符串，二者兼容。
fn parse_brew_services(json: &str) -> Vec<ServiceInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(items) = v.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|it| {
            let obj = it.as_object()?;
            let name = obj.get("name")?.as_str()?.to_string();
            let state = obj
                .get("state")
                .and_then(|s| s.as_str())
                .map(String::from)
                .unwrap_or_else(|| {
                    let running = obj
                        .get("running")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    if running { "started" } else { "stopped" }.to_string()
                });
            let pid = obj.get("pid").and_then(|p| p.as_u64());
            Some(ServiceInfo {
                name,
                state,
                manager: "brew".to_string(),
                pid,
            })
        })
        .collect()
}

/// 解析 `systemctl --user list-unit-files --no-legend` 输出：每行首 token 为单元名，
/// 第二个 token 为状态（enabled/disabled/static/…）。
#[cfg(not(windows))]
fn parse_systemd_units(text: &str) -> Vec<ServiceInfo> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let unit = it.next()?.to_string();
            if !unit.ends_with(".service") {
                return None;
            }
            let _load = it.next();
            let active = it.next().unwrap_or("unknown");
            let state = match active {
                "active" => "started",
                "inactive" => "stopped",
                "failed" => "error",
                other => other,
            }
            .to_string();
            Some(ServiceInfo {
                name: unit,
                state,
                manager: "systemd".to_string(),
                pid: None,
            })
        })
        .collect()
}

/// 静默运行命令并取 stdout（命令缺失/失败返回 None；10 秒超时防首启卡顿）。
fn quiet_output(prog: &str, args: &[&str]) -> Option<String> {
    let out = crate::process::output(
        std::process::Command::new(prog).args(args),
        std::time::Duration::from_secs(15),
    )
    .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_brew_services_handles_running_bool_and_state_field() {
        let json = r#"[
            {"name":"redis","service_name":"homebrew.mxcl.redis","running":true,"pid":123},
            {"name":"nginx","service_name":"homebrew.mxcl.nginx","running":false,"pid":null},
            {"name":"legacy","state":"error","pid":null},
            {"broken": true}
        ]"#;
        let list = parse_brew_services(json);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].state, "started");
        assert_eq!(list[0].pid, Some(123));
        assert_eq!(list[1].state, "stopped");
        assert_eq!(list[2].state, "error");
    }

    #[test]
    fn parse_brew_services_rejects_garbage() {
        assert!(parse_brew_services("not json").is_empty());
        assert!(parse_brew_services("{\"a\":1}").is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn parse_systemd_units_filters_non_service_units() {
        let text = "nginx.service          loaded active running Nginx\nssh.socket             loaded active listening SSH\napp.service            loaded inactive dead App\n";
        let list = parse_systemd_units(text);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "nginx.service");
        assert_eq!(list[0].state, "started");
        assert_eq!(list[1].state, "stopped");
        assert_eq!(list[1].name, "app.service");
    }

    #[test]
    fn unit_name_validation_blocks_injection() {
        assert!(validate_unit_name("nginx.service").is_ok());
        assert!(validate_unit_name("user@1000.service").is_ok());
        assert!(validate_unit_name("a;rm -rf").is_err());
        assert!(validate_unit_name("../etc").is_err());
        assert!(validate_unit_name("").is_err());
    }

    #[test]
    fn action_rejects_bad_inputs() {
        assert!(matches!(
            action("brew", "redis", "enable"),
            Err(AppError::Unsupported(_))
        ));
        assert!(matches!(
            action("npm", "redis", "start"),
            Err(AppError::Unsupported(_))
        ));
        assert!(matches!(
            action("brew", "a b", "start"),
            Err(AppError::Unsupported(_))
        ));
    }
}
