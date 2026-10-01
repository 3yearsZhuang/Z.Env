// 操作历史：管理动作的本地追加日志（~/.zenv/history.jsonl），仅本机留痕、不上传。
// 记录失败静默忽略——历史绝不阻塞业务动作本身；解析为纯函数便于单测。
use serde::{Deserialize, Serialize};

/// 单条操作记录。time 为 unix 秒；kind 为操作类别（前端映射中文标签）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub time: u64,
    pub kind: String,
    pub detail: String,
}

/// 日志文件：~/.zenv/history.jsonl（与托管农场同一数据根）。
fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .map(|h| h.join(".zenv/history.jsonl"))
}

/// 当前 unix 秒（历史留痕与预设导出的时间戳共用）。
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 追加一条记录；任何失败静默忽略。
pub fn record(kind: &str, detail: impl Into<String>) {
    let Some(p) = path() else { return };
    let entry = HistoryEntry {
        time: now_secs(),
        kind: kind.to_string(),
        detail: detail.into(),
    };
    let Ok(line) = serde_json::to_string(&entry) else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = append_line(&p, &line);
}

/// 最近 count 条（新 → 旧）；损坏行跳过。
pub fn list(count: usize) -> Vec<HistoryEntry> {
    let Some(p) = path() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(p) else {
        return Vec::new();
    };
    let mut out = parse_lines(&content);
    out.reverse();
    out.truncate(count);
    out
}

/// 追加一行到日志文件。
fn append_line(file: &std::path::Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    writeln!(f, "{line}")
}

/// 解析 JSONL 文本为记录列表（空行/损坏行跳过）。
fn parse_lines(content: &str) -> Vec<HistoryEntry> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lines_skips_corrupt_and_blank_lines() {
        let content = "{\"time\":100,\"kind\":\"install\",\"detail\":\"mise node@22\"}\n\nnot-json\n{\"time\":200,\"kind\":\"use\",\"detail\":\"go → 1.22\"}\n";
        let entries = parse_lines(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, "install");
        assert_eq!(entries[1].detail, "go → 1.22");
    }

    #[test]
    fn append_then_parse_roundtrip_in_tempdir() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("history.jsonl");
        append_line(
            &file,
            "{\"time\":1,\"kind\":\"env-set\",\"detail\":\"A=1\"}",
        )
        .unwrap();
        append_line(
            &file,
            "{\"time\":2,\"kind\":\"cache-clean\",\"detail\":\"npm\"}",
        )
        .unwrap();
        let content = std::fs::read_to_string(&file).unwrap();
        let entries = parse_lines(&content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].kind, "cache-clean");
    }

    #[test]
    fn list_reverses_and_truncates() {
        // 直接验证反转+截断逻辑（绕过真实 HOME）
        let mut entries = parse_lines(
            "{\"time\":1,\"kind\":\"a\",\"detail\":\"1\"}\n{\"time\":2,\"kind\":\"b\",\"detail\":\"2\"}\n{\"time\":3,\"kind\":\"c\",\"detail\":\"3\"}\n",
        );
        entries.reverse();
        entries.truncate(2);
        assert_eq!(
            entries.iter().map(|e| e.kind.clone()).collect::<Vec<_>>(),
            vec!["c", "b"]
        );
    }
}
