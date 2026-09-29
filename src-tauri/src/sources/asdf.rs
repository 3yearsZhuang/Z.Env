// ASDF 源：通过本地 asdf 的 `list-all` 获取远程版本（若安装了 asdf）。
use std::process::Command;

use super::finalize_versions;
use crate::error::AppError;

pub fn list_remote_versions(tool: &str) -> Result<Vec<String>, AppError> {
    let out = Command::new("asdf")
        .args(["list-all", tool])
        .output()
        .map_err(|e| AppError::Unsupported(format!("无法执行 asdf list-all（未安装？）: {}", e)))?;
    if !out.status.success() {
        return Err(AppError::Unsupported(
            "asdf 不可用或工具无对应插件".to_string(),
        ));
    }
    let list: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(finalize_versions(list))
}
