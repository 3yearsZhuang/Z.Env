// 环境预设：可分享的编程环境配置文件（Z.Env 预设 TOML v1）的导出与导入。
// 只承载 [tools] 工具与版本，不含全局 env 与 brew 清单——那部分由「环境快照」承担。
// 导入侧容忍两种书写：本应用导出的引号键，以及用户手写 mise.toml 的裸键。
use crate::error::AppError;
use serde::Serialize;

/// 当前预设文件格式版本。
pub const FORMAT_VERSION: u32 = 1;
/// 首行标记前缀，用于识别 Z.Env 预设文件并读取格式版本。
const MARKER_PREFIX: &str = "# Z.Env 环境预设";

/// 预设中的一个工具与版本请求（版本可为 `20` / `latest` / `temurin-21` 等 mise 请求式）。
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetTool {
    pub name: String,
    pub version: String,
}

/// 导入结果：预设元数据与工具清单。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetFile {
    pub name: String,
    pub description: Option<String>,
    pub tools: Vec<PresetTool>,
    pub path: String,
}

/// 从 mise.toml 风格文本中提取 `[tools]` 段；同名键保留首个，避免重复安装。
/// 兼容 mise 的三种工具写法：`node = "20"`、`node = { version = "20" }`
/// 与 `[tools.node]` 子表（子表取其 `version` 字段）。
pub fn parse_tools(content: &str) -> Result<Vec<PresetTool>, AppError> {
    let doc: toml::Value = toml::from_str(content).map_err(|e| AppError::Parse(e.to_string()))?;
    let Some(tools) = doc.get("tools") else {
        return Ok(Vec::new());
    };
    let table = tools
        .as_table()
        .ok_or_else(|| AppError::Parse("tools 必须为表".into()))?;
    table
        .iter()
        .map(|(name, item)| {
            let value = item
                .as_table()
                .and_then(|t| t.get("version"))
                .unwrap_or(item);
            let value = match value.as_array() {
                Some(a) if a.len() == 1 => &a[0],
                Some(_) => {
                    return Err(AppError::Unsupported(format!(
                        "{name} 含多个版本，请使用项目编辑器管理"
                    )))
                }
                None => value,
            };
            let version = match value {
                toml::Value::String(s) => s.clone(),
                toml::Value::Integer(i) => i.to_string(),
                toml::Value::Float(f) if f.is_finite() => f.to_string(),
                _ => return Err(AppError::Parse(format!("{name} 缺少有效的 version"))),
            };
            crate::mise::validate_spec(name, &version)?;
            Ok(PresetTool {
                name: name.clone(),
                version,
            })
        })
        .collect()
}

/// 导出预设到指定路径，返回写出的工具数量。
/// `toml` 为 mise.toml 风格文本（内置预设经前端生成，用户预设为已存内容）。
pub fn export(
    path: &str,
    name: &str,
    description: &str,
    toml: &str,
    app_version: &str,
) -> Result<usize, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Other("预设名不能为空".into()));
    }
    let tools = parse_tools(toml)?;
    if tools.is_empty() {
        return Err(AppError::Other(
            "当前配置里没有可用的 [tools] 工具段，无法导出预设".into(),
        ));
    }

    let mut s = String::new();
    s.push_str(&format!("{MARKER_PREFIX} v{FORMAT_VERSION}\n"));
    s.push_str("# 可分享的编程环境配置；在 Z.Env「项目配置 → 环境预设」导入后可一键安装。\n\n");
    s.push_str("[preset]\n");
    s.push_str(&format!("name = \"{}\"\n", escape(name)));
    let desc = description.trim();
    if !desc.is_empty() {
        s.push_str(&format!("description = \"{}\"\n", escape(desc)));
    }
    s.push_str(&format!("app_version = \"{}\"\n", escape(app_version)));
    s.push_str(&format!(
        "created_at_unix = {}\n",
        crate::history::now_secs()
    ));

    s.push_str("\n[tools]\n");
    for t in &tools {
        // 键一律加引号：`npm:prettier` 这类后端名含冒号，裸键不是合法 TOML
        s.push_str(&format!(
            "\"{}\" = \"{}\"\n",
            escape(&t.name),
            escape(&t.version)
        ));
    }

    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    crate::config::atomic_write(std::path::Path::new(path), s.as_bytes())?;
    Ok(tools.len())
}

/// 导入预设文件；无 Z.Env 标记但含 `[tools]` 的纯 mise.toml 同样接受。
pub fn import(path: &str) -> Result<PresetFile, AppError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::Io(format!("读取预设文件失败: {e}")))?;

    check_format(&content)?;

    let tools = parse_tools(&content)?;
    if tools.is_empty() {
        return Err(AppError::Other(
            "文件里没有可用的 [tools] 段，不是有效的环境预设".into(),
        ));
    }

    let doc: toml::Value = toml::from_str(&content).map_err(|e| AppError::Parse(e.to_string()))?;
    let pick = |key: &str| -> Option<String> {
        doc.get("preset")?
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    Ok(PresetFile {
        name: pick("name").unwrap_or_else(|| fallback_name(path)),
        description: pick("description"),
        tools,
        path: path.to_string(),
    })
}

/// 校验格式版本：无标记按纯 mise.toml 放行；标记版本高于当前支持则明确拒绝。
fn check_format(content: &str) -> Result<(), AppError> {
    let Some(first) = content.lines().find(|l| !l.trim().is_empty()) else {
        return Err(AppError::Other("预设文件是空的".into()));
    };
    let Some(rest) = first.trim().strip_prefix(MARKER_PREFIX) else {
        return Ok(());
    };
    match rest.trim().trim_start_matches('v').parse::<u32>() {
        Ok(v) if v > FORMAT_VERSION => Err(AppError::Other(format!(
            "该预设为 v{v} 格式，当前 Z.Env 仅支持 v{FORMAT_VERSION}，请升级应用"
        ))),
        // 版本号缺失（标记行被手工改坏）时按当前版本处理，不阻断导入
        _ => Ok(()),
    }
}

/// 无名预设的回退名：取文件名并去掉 `.zenv` 后缀（默认导出名为 `<名称>.zenv.toml`）。
fn fallback_name(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let cleaned = stem.strip_suffix(".zenv").unwrap_or(&stem).trim();
    if cleaned.is_empty() {
        "未命名预设".to_string()
    } else {
        cleaned.to_string()
    }
}

/// TOML 基本字符串转义，与快照导出共用同一实现。
fn escape(value: &str) -> String {
    crate::env_center::escape_basic(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preset_roundtrip_and_toml_forms() {
        let src = "[tools] # comment\nnode = { version = '20', os = ['linux'] }\n[tools.\"npm:prettier\"]\nversion = \"3\"\n";
        let tools = parse_tools(src).unwrap();
        assert_eq!(tools.len(), 2);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("demo.zenv.toml");
        export(file.to_str().unwrap(), "中文\n预设", "说明", src, "0.7.1").unwrap();
        let result = import(file.to_str().unwrap()).unwrap();
        assert_eq!(result.name, "中文\n预设");
        assert_eq!(result.tools, tools);
    }
    #[test]
    fn invalid_or_lossy_imports_are_rejected() {
        for src in [
            "[tools]\nnode='20'\nnode='22'",
            "[tools]\nnode=['20','22']",
            "[tools]\nnode={os=['linux']}",
        ] {
            assert!(parse_tools(src).is_err());
        }
        assert!(check_format("# Z.Env 环境预设 v99").is_err());
        assert!(check_format("").is_err());
    }
    #[test]
    fn plain_mise_and_unicode_escapes_are_supported() {
        assert_eq!(
            parse_tools("[tools]\nnode = '20' # version\n").unwrap()[0].version,
            "20"
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rust-cli.zenv.toml");
        std::fs::write(&file, "[tools]\nrust='stable'\n").unwrap();
        assert_eq!(import(file.to_str().unwrap()).unwrap().name, "rust-cli");
    }
}
