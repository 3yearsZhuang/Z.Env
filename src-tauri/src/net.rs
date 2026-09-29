// 网络抓取层：统一 HTTP 客户端（ureq + rustls），不再依赖外部 curl。
// 好处：Windows 老系统无需自带 curl；能拿到真实状态码与响应体，便于区分限流/未找到。
use std::sync::OnceLock;
use std::time::Duration;

use ureq::{Agent, AgentBuilder};

use crate::error::AppError;

/// 请求超时：版本列表都是小响应，10 秒足够；避免网络挂起拖慢界面。
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// 共享 HTTP 客户端（默认跟随重定向）。
fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| AgentBuilder::new().timeout(HTTP_TIMEOUT).build())
}

/// 通用 HTTP GET：返回（HTTP 状态码, 响应体）。
/// 非 2xx 状态也返回 Ok(status, body)，由调用方按状态码决定语义（如解析 GitHub 错误消息）。
pub fn http_get(url: &str) -> Result<(u16, String), AppError> {
    let result = agent().get(url).set("User-Agent", "mise-gui").call();
    match result {
        Ok(resp) => read_body(resp.status(), resp),
        Err(ureq::Error::Status(code, resp)) => read_body(code, resp),
        Err(e) => Err(AppError::Network(format!("网络请求失败: {}", e))),
    }
}

/// 读取响应体。
fn read_body(status: u16, resp: ureq::Response) -> Result<(u16, String), AppError> {
    let body = resp
        .into_string()
        .map_err(|e| AppError::Network(format!("读取响应失败: {}", e)))?;
    Ok((status, body))
}

/// 抓取并解析 JSON。HTTP >= 400 时报错并带上服务端 message（如 GitHub 限流提示）。
pub fn get_json(url: &str) -> Result<serde_json::Value, AppError> {
    let (status, body) = http_get(url)?;
    if status >= 400 {
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|j| {
                j.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "服务返回错误".to_string());
        let hint = match status {
            403 => "（未认证 GitHub API 每小时限 60 次，请稍后再试）",
            404 => "（仓库不存在或已迁移）",
            _ => "",
        };
        return Err(AppError::Http {
            status,
            message: format!("{}{}", msg, hint),
        });
    }
    serde_json::from_str(&body)
        .map_err(|_| AppError::Parse(format!("请求返回格式异常（HTTP {}）", status)))
}
