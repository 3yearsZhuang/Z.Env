// 整机环境变量中心：管理全局 mise config 的 [env] 段，并提供系统 env 的只读展示与冲突检测。
// 全局 config 是用户可能手工编辑的文件，因此写入采用「文本手术」——只重写 [env] 段内的目标行，
// 其余内容（含注释与排版）原样保留；不引入 toml 依赖，段解析与编辑均为纯函数便于单测。
// v1 近似：多行字符串内的 `[`/`#` 等按原文计入结构判断，异常形态提示用户手动编辑。
use crate::error::AppError;
use serde::Serialize;

/// 全局 mise config 路径：MISE_GLOBAL_CONFIG_FILE > MISE_CONFIG_DIR > XDG_CONFIG_HOME > 平台默认。
pub fn global_config_path() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("MISE_GLOBAL_CONFIG_FILE") {
        if !p.is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let home = std::path::PathBuf::from(home);
    if let Ok(dir) = std::env::var("MISE_CONFIG_DIR") {
        if !dir.is_empty() {
            let d = std::path::PathBuf::from(dir);
            return Some(if d.is_absolute() {
                d.join("config.toml")
            } else {
                home.join(d).join("config.toml")
            });
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(std::path::PathBuf::from(xdg).join("mise/config.toml"));
        }
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.is_empty() {
                return Some(std::path::PathBuf::from(appdata).join("mise/config.toml"));
            }
        }
    }
    Some(home.join(".config/mise/config.toml"))
}

/// 单条全局 env。kind = "simple"（标量，可编辑）| "complex"（表/数组/点分键，只读）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalEnvEntry {
    pub key: String,
    pub value: String,
    pub kind: String,
}

/// 同名冲突：系统/用户级 env 与全局 mise env 同键不同值（PATH 除外，mise 对 PATH 有专门语义）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflict {
    pub key: String,
    pub mise_value: String,
    pub system_value: String,
}

/// 系统/用户级 env 单条（进程环境，只读展示）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemEnvVar {
    pub key: String,
    pub value: String,
}

/// 环境变量中心快照：配置路径、是否已存在、全局 env 列表、冲突清单、系统 env 列表。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCenterSnapshot {
    pub config_path: String,
    pub exists: bool,
    pub entries: Vec<GlobalEnvEntry>,
    pub conflicts: Vec<EnvConflict>,
    pub system_env: Vec<SystemEnvVar>,
}

/// 汇总读取：全局 [env] 列表 + 与进程环境的同名冲突。
pub fn list() -> EnvCenterSnapshot {
    let path = global_config_path();
    let exists = path.as_ref().map(|p| p.exists()).unwrap_or(false);
    let content = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let entries = parse_env_section(&content);
    let conflicts = entries
        .iter()
        .filter(|e| e.kind == "simple" && e.key != "PATH")
        .filter_map(|e| {
            std::env::var(&e.key)
                .ok()
                .filter(|sys| sys != &e.value)
                .map(|sys| EnvConflict {
                    key: e.key.clone(),
                    mise_value: e.value.clone(),
                    system_value: sys,
                })
        })
        .collect();
    let mut system_env: Vec<SystemEnvVar> = std::env::vars()
        .map(|(key, value)| SystemEnvVar { key, value })
        .collect();
    system_env.sort_by(|a, b| a.key.cmp(&b.key));
    EnvCenterSnapshot {
        config_path: path.map(|p| p.display().to_string()).unwrap_or_default(),
        exists,
        entries,
        conflicts,
        system_env,
    }
}

/// 设置一个全局 env：文件不存在则创建，无 [env] 段则追加段，键已存在则替换。
pub fn set(key: &str, value: &str) -> Result<String, AppError> {
    let path =
        global_config_path().ok_or_else(|| AppError::Io("无法定位全局 mise 配置目录".into()))?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let new_content = set_env_in_config(&content, key, value)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, new_content)?;
    Ok(format!("已写入 {}（全局 [env]）", key))
}

/// 删除一个全局 env：键不存在时报错。
pub fn remove(key: &str) -> Result<String, AppError> {
    let path =
        global_config_path().ok_or_else(|| AppError::Io("无法定位全局 mise 配置目录".into()))?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let new_content = remove_env_from_config(&content, key)?;
    std::fs::write(&path, new_content)?;
    Ok(format!("已移除 {}（全局 [env]）", key))
}

/// 校验可托管的键名：拒绝空、含空白收尾、等号、换行、`#`/`.`/`[` 开头结构（点分键属高级值，只读）。
fn validate_key(key: &str) -> Result<(), AppError> {
    if key.is_empty()
        || key.trim() != key
        || key.contains('=')
        || key.contains('\n')
        || key.starts_with('#')
        || key.contains('.')
        || key.contains('[')
    {
        return Err(AppError::Unsupported(format!("不支持的键名: {}", key)));
    }
    Ok(())
}

/// 截取顶层 `[env]` 段行区间 [start, end)：start 为段头行下标，end 为下一个顶层表头行下标或文件尾。
fn env_section_bounds(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| l.trim() == "[env]")?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.trim_start().starts_with('[') {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// 一行 env 赋值的解析结果：Simple 可编辑；Complex 只读（表/数组/点分键/多行字符串）。
#[derive(Debug, PartialEq)]
enum EnvValue {
    Simple(String),
    Complex,
}

/// 解析 `[env]` 段内的一行赋值；空行与注释返回 None。
fn parse_env_line(line: &str) -> Option<(String, EnvValue)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let eq = trimmed.find('=')?;
    let key = trimmed[..eq].trim();
    if key.is_empty() {
        return None;
    }
    let raw = trimmed[eq + 1..].trim();
    // 点分键（如 _.path）、表、数组、多行基本字符串一律按高级值只读处理
    if key.contains('.')
        || key.contains('[')
        || raw.starts_with('{')
        || raw.starts_with('[')
        || raw.starts_with("\"\"\"")
    {
        return Some((key.to_string(), EnvValue::Complex));
    }
    if let Some(v) = parse_basic_string(raw) {
        return Some((key.to_string(), EnvValue::Simple(v)));
    }
    if let Some(rest) = raw.strip_prefix('\'') {
        let end = rest.find('\'')?;
        return Some((key.to_string(), EnvValue::Simple(rest[..end].to_string())));
    }
    if !raw.is_empty() {
        // 裸标量（数字/布尔等）按原文展示与编辑
        return Some((key.to_string(), EnvValue::Simple(raw.to_string())));
    }
    None
}

/// 解析 TOML 基本字符串（整行闭合的单引号对 `"..."`），支持 \\、\"、\n、\t 转义。
fn parse_basic_string(raw: &str) -> Option<String> {
    if !raw.starts_with('"') {
        return None;
    }
    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some(out),
            b'\\' => {
                i += 1;
                let c = *bytes.get(i)?;
                match c {
                    b'\\' => out.push('\\'),
                    b'"' => out.push('"'),
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    _ => return None,
                }
            }
            c => out.push(c as char),
        }
        i += 1;
    }
    None
}

/// 从第 i 行起计算一条复杂值占用的行数：方/花括号配平即止（字符串内括号按原文计入，v1 近似）。
fn complex_span(lines: &[&str], i: usize, end: usize) -> usize {
    let mut bal: isize = 0;
    let mut j = i;
    while j < end {
        for c in lines[j].chars() {
            match c {
                '[' | '{' => bal += 1,
                ']' | '}' => bal -= 1,
                _ => {}
            }
        }
        j += 1;
        if bal <= 0 {
            break;
        }
    }
    j - i
}

/// 在 `[env]` 段内查找键所在行：返回 (行下标, 占用行数)。找不到返回 None。
fn find_key_span(lines: &[&str], start: usize, end: usize, key: &str) -> Option<(usize, usize)> {
    let mut i = start + 1;
    while i < end {
        match parse_env_line(lines[i]) {
            None => i += 1,
            Some((k, v)) => {
                let span = match v {
                    EnvValue::Complex => complex_span(lines, i, end),
                    EnvValue::Simple(_) => 1,
                };
                if k == key {
                    return Some((i, span));
                }
                i += span;
            }
        }
    }
    None
}

/// 重组文件文本：保留原有行内容与结尾换行风格（空文件/以换行结尾 → 补结尾换行）。
fn join_preserving_tail(original: &str, lines: Vec<String>) -> String {
    let mut s = lines.join("\n");
    if original.is_empty() || original.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// 在 config 文本中设置 `key = "value"`：无 [env] 段则文件尾追加段；其余行原样保留。
pub fn set_env_in_config(content: &str, key: &str, value: &str) -> Result<String, AppError> {
    validate_key(key)?;
    let new_line = format!("{} = \"{}\"", key, escape_basic(value));
    let lines: Vec<String> = content.lines().map(String::from).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();

    if let Some((start, end)) = env_section_bounds(&refs) {
        if let Some((i, span)) = find_key_span(&refs, start, end, key) {
            let mut out = lines[..i].to_vec();
            out.push(new_line);
            out.extend_from_slice(&lines[i + span..]);
            return Ok(join_preserving_tail(content, out));
        }
        // 新键：插到段头之后的第一行
        let mut out = lines[..start + 1].to_vec();
        out.push(new_line);
        out.extend_from_slice(&lines[start + 1..]);
        return Ok(join_preserving_tail(content, out));
    }

    // 无 [env] 段：文件尾追加（与既有内容间留一空行）
    let mut out = lines.clone();
    if let Some(last) = out.last() {
        if !last.trim().is_empty() {
            out.push(String::new());
        }
    }
    out.push("[env]".to_string());
    out.push(new_line);
    Ok(join_preserving_tail(content, out))
}

/// 从 config 文本中移除 `[env]` 段内的键（连同其续行）；键不存在时报 Unsupported。
pub fn remove_env_from_config(content: &str, key: &str) -> Result<String, AppError> {
    validate_key(key)?;
    let lines: Vec<String> = content.lines().map(String::from).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let (start, end) = env_section_bounds(&refs)
        .ok_or_else(|| AppError::Unsupported("全局配置中不存在 [env] 段".to_string()))?;
    let (i, span) = find_key_span(&refs, start, end, key)
        .ok_or_else(|| AppError::Unsupported(format!("[env] 中不存在键 {}", key)))?;
    let mut out = lines[..i].to_vec();
    out.extend_from_slice(&lines[i + span..]);
    Ok(join_preserving_tail(content, out))
}

/// 解析 `[env]` 段为条目列表（复杂值整块跳过，只读展示）。
pub fn parse_env_section(content: &str) -> Vec<GlobalEnvEntry> {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = env_section_bounds(&lines) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = start + 1;
    while i < end {
        match parse_env_line(lines[i]) {
            None => i += 1,
            Some((key, EnvValue::Simple(v))) => {
                out.push(GlobalEnvEntry {
                    key,
                    value: v,
                    kind: "simple".into(),
                });
                i += 1;
            }
            Some((key, EnvValue::Complex)) => {
                out.push(GlobalEnvEntry {
                    key,
                    value: "表/数组等高级值，请在文件中手动编辑".into(),
                    kind: "complex".into(),
                });
                i += complex_span(&lines, i, end);
            }
        }
    }
    out
}

/// TOML 基本字符串转义：反斜杠与双引号（快照导出同样复用）。
pub(crate) fn escape_basic(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# 全局配置\nmin_version = \"2024.1\"\n\n[tools]\nnode = \"22\"\n\n[env]\n# 注释保留\nEDITOR = \"vim\"\nVERBOSE = 1\n_.path = [\"/opt/bin\"]\nFLAGS = \"-a \\\"x\\\"\"\n\n[settings]\nexperimental = true\n";

    #[test]
    fn parse_env_section_extracts_scalars_and_marks_complex() {
        let entries = parse_env_section(SAMPLE);
        let get = |k: &str| entries.iter().find(|e| e.key == k).unwrap();
        assert_eq!(get("EDITOR").value, "vim");
        assert_eq!(get("EDITOR").kind, "simple");
        assert_eq!(get("VERBOSE").value, "1");
        assert_eq!(get("FLAGS").value, "-a \"x\"");
        assert_eq!(get("_.path").kind, "complex");
        // [tools]/[settings] 段不误入
        assert!(entries
            .iter()
            .all(|e| e.key != "node" && e.key != "experimental"));
    }

    #[test]
    fn set_replaces_existing_and_preserves_comments_and_other_sections() {
        let out = set_env_in_config(SAMPLE, "EDITOR", "nvim").unwrap();
        assert!(out.contains("EDITOR = \"nvim\""));
        assert!(!out.contains("EDITOR = \"vim\""));
        assert!(out.contains("# 注释保留"));
        assert!(out.contains("[tools]"));
        assert!(out.contains("node = \"22\""));
        assert!(out.contains("[settings]"));
        // 复杂值不受影响
        assert!(out.contains("_.path"));
    }

    #[test]
    fn set_new_key_inserts_right_after_section_header() {
        let out = set_env_in_config(SAMPLE, "HTTP_PROXY", "http://127.0.0.1:7890").unwrap();
        let env_pos = out.find("[env]").unwrap();
        let inserted = out.find("HTTP_PROXY = \"http://127.0.0.1:7890\"").unwrap();
        let editor = out.find("EDITOR").unwrap();
        assert!(env_pos < inserted && inserted < editor);
    }

    #[test]
    fn set_appends_section_when_missing_and_escapes_value() {
        let out = set_env_in_config("foo = \"bar\"\n", "A", "say \"hi\"\\ok").unwrap();
        assert!(out.contains("[env]"));
        assert!(out.contains("A = \"say \\\"hi\\\"\\\\ok\""));
        assert!(out.starts_with("foo = \"bar\""));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn set_on_empty_content_creates_minimal_file() {
        let out = set_env_in_config("", "K", "v").unwrap();
        assert_eq!(out, "[env]\nK = \"v\"\n");
    }

    #[test]
    fn remove_deletes_key_and_reports_missing() {
        let out = remove_env_from_config(SAMPLE, "EDITOR").unwrap();
        assert!(!out.contains("EDITOR"));
        assert!(out.contains("VERBOSE"));
        assert!(remove_env_from_config(SAMPLE, "NOPE").is_err());
        assert!(remove_env_from_config("a = 1\n", "A").is_err());
    }

    #[test]
    fn roundtrip_set_two_remove_one_leaves_other_intact() {
        let s1 = set_env_in_config(SAMPLE, "A", "1").unwrap();
        let s2 = set_env_in_config(&s1, "B", "2").unwrap();
        let s3 = remove_env_from_config(&s2, "A").unwrap();
        let entries = parse_env_section(&s3);
        assert!(entries.iter().any(|e| e.key == "B" && e.value == "2"));
        assert!(!entries.iter().any(|e| e.key == "A"));
        assert!(entries.iter().any(|e| e.key == "EDITOR"));
    }

    #[test]
    fn validate_key_rejects_unsafe_names() {
        for bad in ["", " a", "a ", "a=b", "a.b", "#a", "a[0]"] {
            assert!(validate_key(bad).is_err(), "应拒绝: {bad}");
        }
        assert!(validate_key("GOOD_KEY").is_ok());
    }

    #[test]
    fn complex_multiline_value_is_skipped_whole() {
        let cfg = "[env]\nMATRIX = [\n  \"a\",\n  \"b\",\n]\nOK = \"1\"\n";
        let entries = parse_env_section(cfg);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "MATRIX");
        assert_eq!(entries[0].kind, "complex");
        assert_eq!(entries[1].key, "OK");
    }
}
