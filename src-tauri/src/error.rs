// 应用统一错误类型。
// 对外（Tauri command 边界）仍序列化为字符串以保持前端兼容；
// 内部用类型区分失败原因，便于针对性处理与单元测试断言。
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    /// mise 未安装或不在 PATH 中（用户可据此获得安装引导）。
    #[error("mise 未安装或未在 PATH 中找到，请先安装 mise")]
    MiseNotInstalled,
    /// 网络层失败（DNS 解析、超时、连接中断等）。
    #[error("网络请求失败: {0}")]
    Network(String),
    /// 服务端返回了 HTTP >= 400 的状态。
    #[error("请求失败（HTTP {status}）：{message}")]
    Http { status: u16, message: String },
    /// 数据解析失败（JSON / 文本格式不符合预期）。
    #[error("解析失败: {0}")]
    Parse(String),
    /// 文件系统 / 子进程 IO 失败。
    #[error("IO 错误: {0}")]
    Io(String),
    /// 工具或版本源不可用（如 asdf 未安装、运行时无对应源映射）。
    #[error("{0}")]
    Unsupported(String),
    /// 其他未归类错误。
    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

/// Tauri command 边界保持返回 String（前端兼容）：内部 AppError 经此转成用户可读文案。
impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}
