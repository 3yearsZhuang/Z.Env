// 环境快照与迁移：导出整机环境清单（TOML），新机重建走安全边界——
// 全局 env 直接写入（复用 env_center）、mise 工具逐个安装、brew 清单生成
// Brewfile 交由用户执行 `brew bundle --file`（不静默批量装软件）。
use crate::error::AppError;
use serde::Serialize;
use tauri::AppHandle;

/// 导出摘要。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub tools: usize,
    pub env_vars: usize,
    pub brew_packages: usize,
    pub path: String,
}

/// 导出快照到指定路径（TOML，机器生成格式：引号键 + 基本字符串值）。
pub fn export(path: &str) -> Result<SnapshotSummary, AppError> {
    // mise 已激活工具（无激活版本的跳过）
    let mut tools: Vec<(String, String)> = Vec::new();
    for t in crate::mise::list_tools()? {
        if let Some(v) = t.active_versions.first() {
            tools.push((t.name, v.clone()));
        }
    }
    // 全局 env（仅可编辑的 simple 条目）
    let env_entries: Vec<(String, String)> = crate::env_center::list()
        .entries
        .into_iter()
        .filter(|e| e.kind == "simple")
        .map(|e| (e.key, e.value))
        .collect();
    // brew 已装清单（brew 不可用时为空并注明）
    let brew: Vec<String> = crate::env::detect_system_installed("brew").unwrap_or_default();

    let mut s = String::from("# Z.Env 环境快照（机器生成，可手工修订）\n\n[tools]\n");
    if tools.is_empty() {
        s.push_str("# (无)\n");
    }
    for (k, v) in &tools {
        s.push_str(&format!(
            "\"{}\" = \"{}\"\n",
            crate::env_center::escape_basic(k),
            crate::env_center::escape_basic(v)
        ));
    }
    s.push_str("\n[global_env]\n");
    if env_entries.is_empty() {
        s.push_str("# (无)\n");
    }
    for (k, v) in &env_entries {
        s.push_str(&format!(
            "\"{}\" = \"{}\"\n",
            crate::env_center::escape_basic(k),
            crate::env_center::escape_basic(v)
        ));
    }
    s.push_str("\n[brew]\n");
    if brew.is_empty() {
        s.push_str("# (无或 brew 不可用)\n");
    }
    for b in &brew {
        s.push_str(&format!(
            "\"{}\" = \"installed\"\n",
            crate::env_center::escape_basic(b)
        ));
    }

    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, s)?;
    Ok(SnapshotSummary {
        tools: tools.len(),
        env_vars: env_entries.len(),
        brew_packages: brew.len(),
        path: path.to_string(),
    })
}

/// 从快照重建（v1 安全边界）：全局 env 直接写入；mise 工具逐个安装（耗时可能较长）；
/// brew 清单写入 ~/.zenv/Brewfile.snapshot，由用户执行 brew bundle 安装。返回多行报告。
///
/// 工具安装复用 `install_version_streaming`，与「全局环境」页的预设整机安装走同一条通道：
/// 都有实时进度事件（`mise:install-progress`），都逐个容错、单个失败不中断。
pub fn restore(app: &AppHandle, path: &str) -> Result<String, AppError> {
    let content =
        std::fs::read_to_string(path).map_err(|e| AppError::Io(format!("读取快照失败: {e}")))?;
    let tools = parse_section(&content, "[tools]");
    let envs = parse_section(&content, "[global_env]");
    let brew = parse_section(&content, "[brew]");

    let mut report: Vec<String> = Vec::new();

    // 1) 全局 env
    let mut env_ok = 0usize;
    for (k, v) in &envs {
        match crate::env_center::set(k, v) {
            Ok(_) => env_ok += 1,
            Err(e) => report.push(format!("env {k} 写入失败：{e}")),
        }
    }
    report.push(format!("全局 env：写入 {env_ok}/{}", envs.len()));

    // 2) mise 工具（逐个流式安装，失败不阻断后续）
    let mut tool_ok = 0usize;
    for (name, version) in &tools {
        match crate::mise::install_version_streaming(app, name, version) {
            Ok(_) => {
                tool_ok += 1;
                report.push(format!("mise {name}@{version} 安装完成"));
            }
            Err(e) => report.push(format!("mise {name}@{version} 安装失败：{e}")),
        }
    }
    report.push(format!("mise 工具：安装 {tool_ok}/{}", tools.len()));

    // 3) Brewfile（不自动执行）
    if brew.is_empty() {
        report.push("brew：快照无软件清单，跳过".into());
    } else {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .ok_or_else(|| AppError::Io("无法定位主目录".into()))?;
        let brewfile = home.join(".zenv/Brewfile.snapshot");
        if let Some(dir) = brewfile.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text: String = brew.iter().map(|(k, _)| brewfile_line(k)).collect();
        std::fs::write(&brewfile, text)?;
        report.push(format!(
            "brew：清单已生成 {}（{} 个软件），执行 brew bundle --file={} 即可一次安装",
            brewfile.display(),
            brew.len(),
            brewfile.display()
        ));
    }

    let summary = format!(
        "重建完成：工具 {tool_ok}/{}、env {env_ok}/{}、brew 待执行 {}",
        tools.len(),
        envs.len(),
        brew.len()
    );
    crate::history::record("snapshot-restore", summary.clone());
    let mut out = summary;
    out.push('\n');
    for line in report {
        out.push('\n');
        out.push_str(&line);
    }
    Ok(out)
}

/// Brewfile 单行。
fn brewfile_line(name: &str) -> String {
    format!("brew \"{}\"\n", name)
}

/// 解析快照的一个段：引号键或裸键 + 标量值（见 `parse_value`）；遇到下一个顶层表头即止；注释与占位行跳过。
pub(crate) fn parse_section(content: &str, header: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim() == header) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for l in lines.iter().skip(start + 1) {
        let t = l.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            break;
        }
        let Some(eq) = t.find('=') else { continue };
        let Some(key) = parse_key(t[..eq].trim()) else {
            continue;
        };
        let Some(val) = parse_value(t[eq + 1..].trim()) else {
            continue;
        };
        out.push((key, val));
    }
    out
}

/// 解析 TOML 键：引号键走 `parse_basic_value`，裸键要求全部为 `[A-Za-z0-9_-]`。
/// 机器生成的快照用引号键，用户手写的 mise.toml 用裸键，两者都要吃。
pub(crate) fn parse_key(raw: &str) -> Option<String> {
    if raw.starts_with('"') {
        return parse_basic_value(raw);
    }
    if !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Some(raw.to_string());
    }
    None
}

/// 解析一个 TOML 基本字符串字面量（含转义还原），要求整体为一对引号包裹；
/// 能正确处理值内含 `\"` 的情形（trim_matches 会误剥转义引号，故逐字符扫描）。
pub(crate) fn parse_basic_value(raw: &str) -> Option<String> {
    let rest = raw.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                o => out.push(o),
            },
            '"' => return Some(out),
            o => out.push(o),
        }
    }
    None
}

/// 把一段 TOML 值解析为字符串：覆盖手写 mise.toml 的常见版本写法——
/// - 双引号基本串 / 单引号字面串（无转义）；
/// - 裸整数或浮点（`node = 20`、`go = 1.27` 均为合法 TOML）；
/// - mise 工具表形式 `{ version = ... }`：取 `version` 字段；
/// - 数组版本 `{ version = ["20", "22"] }`：取首个（预设一条工具只装一个版本）。
///
/// 布尔、日期等其余形式不是合法的版本请求，返回 `None`（跳过该行）。
pub(crate) fn parse_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.starts_with('"') {
        return parse_basic_value(raw);
    }
    if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        // 单引号字面串：值内不允许再出现单引号（多行 ''' 不在版本场景里）
        let inner = &raw[1..raw.len() - 1];
        return if inner.contains('\'') {
            None
        } else {
            Some(inner.to_string())
        };
    }
    if raw.starts_with('[') && raw.ends_with(']') {
        let inner = &raw[1..raw.len() - 1];
        for part in split_top_level(inner) {
            if let Some(v) = parse_value(&part) {
                return Some(v);
            }
        }
        return None;
    }
    if raw.starts_with('{') && raw.ends_with('}') {
        let inner = &raw[1..raw.len() - 1];
        for part in split_top_level(inner) {
            let Some(eq) = part.find('=') else { continue };
            let Some(key) = parse_key(part[..eq].trim()) else {
                continue;
            };
            if key == "version" {
                return parse_value(&part[eq + 1..]);
            }
        }
        return None;
    }
    if let Ok(i) = raw.parse::<i64>() {
        return Some(i.to_string());
    }
    raw.parse::<f64>()
        .ok()
        .filter(|f| f.is_finite())
        .map(|f| f.to_string())
}

/// 按顶层逗号切分内联表 / 数组的成员（忽略 `[]`、`{}` 与引号内的逗号）。
fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut cur = String::new();
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => out.push(cur.trim().to_string()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_value_covers_handwritten_toml_forms() {
        // 双引号基本串与转义
        assert_eq!(parse_value("\"say \\\"hi\\\"\""), Some("say \"hi\"".into()));
        // 单引号字面串（无转义）
        assert_eq!(parse_value("'20'"), Some("20".into()));
        // 裸整数 / 浮点
        assert_eq!(parse_value("3"), Some("3".into()));
        assert_eq!(parse_value("1.27"), Some("1.27".into()));
        // mise 工具表：取 version 字段，数组取首个
        assert_eq!(parse_value("{ version = \"20\" }"), Some("20".into()));
        assert_eq!(
            parse_value("{ version = [\"20\", \"22\"] }"),
            Some("20".into())
        );
        // 布尔、日期、裸单词不是合法版本请求
        assert_eq!(parse_value("true"), None);
        assert_eq!(parse_value("1979-05-27"), None);
        assert_eq!(parse_value("latest"), None);
    }

    #[test]
    fn parse_section_accepts_literal_and_number_values() {
        let toml = "[tools]\nnode = '20'\ngo = 1.27\n";
        assert_eq!(
            parse_section(toml, "[tools]"),
            vec![
                ("node".to_string(), "20".to_string()),
                ("go".to_string(), "1.27".to_string())
            ]
        );
    }

    #[test]
    fn parse_section_reads_quoted_keys_and_stops_at_next_header() {
        let snap = "# 注释\n\n[tools]\n\"node\" = \"22.1.0\"\n\"go\" = \"1.22\"\n\n[global_env]\n\"EDITOR\" = \"say \\\"hi\\\"\"\n# (无)\n\n[brew]\n\"wget\" = \"installed\"\n";
        assert_eq!(
            parse_section(snap, "[tools]"),
            vec![
                ("node".to_string(), "22.1.0".to_string()),
                ("go".to_string(), "1.22".to_string())
            ]
        );
        assert_eq!(
            parse_section(snap, "[global_env]"),
            vec![("EDITOR".to_string(), "say \"hi\"".to_string())]
        );
        assert_eq!(
            parse_section(snap, "[brew]"),
            vec![("wget".to_string(), "installed".to_string())]
        );
        assert!(parse_section(snap, "[nope]").is_empty());
    }

    #[test]
    fn parse_section_accepts_bare_keys() {
        // 用户手写的 mise.toml 用裸键；带冒号的后端名必须加引号
        let toml = "[tools]\nnode = \"20\"\n\"npm:prettier\" = \"3\"\n\n[env]\nEDITOR = \"vim\"\n";
        assert_eq!(
            parse_section(toml, "[tools]"),
            vec![
                ("node".to_string(), "20".to_string()),
                ("npm:prettier".to_string(), "3".to_string())
            ]
        );
        // [env] 段不被吞进 [tools]
        assert_eq!(
            parse_section(toml, "[env]"),
            vec![("EDITOR".to_string(), "vim".to_string())]
        );
    }

    #[test]
    fn escape_parse_roundtrip_handles_escapes() {
        let raw = "say \"hi\"\\ok\n";
        let escaped = crate::env_center::escape_basic(raw);
        let quoted = format!("\"{escaped}\"");
        assert_eq!(parse_basic_value(&quoted).unwrap(), raw);
    }

    #[test]
    fn brewfile_line_quotes_name() {
        assert_eq!(brewfile_line("wget"), "brew \"wget\"\n");
        assert_eq!(brewfile_line("imagemagick@7"), "brew \"imagemagick@7\"\n");
    }
}
