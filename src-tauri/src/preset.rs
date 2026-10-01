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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
pub fn parse_tools(toml: &str) -> Vec<PresetTool> {
    let mut out: Vec<PresetTool> = Vec::new();
    for (name, version) in crate::snapshot::parse_section(toml, "[tools]") {
        if name.is_empty() || out.iter().any(|t| t.name == name) {
            continue;
        }
        out.push(PresetTool { name, version });
    }
    out
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
    let tools = parse_tools(toml);
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
    std::fs::write(path, s)?;
    Ok(tools.len())
}

/// 导入预设文件；无 Z.Env 标记但含 `[tools]` 的纯 mise.toml 同样接受。
pub fn import(path: &str) -> Result<PresetFile, AppError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::Io(format!("读取预设文件失败: {e}")))?;

    check_format(&content)?;

    let tools = parse_tools(&content);
    if tools.is_empty() {
        return Err(AppError::Other(
            "文件里没有可用的 [tools] 段，不是有效的环境预设".into(),
        ));
    }

    let meta = crate::snapshot::parse_section(&content, "[preset]");
    let pick = |key: &str| -> Option<String> {
        meta.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
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

    /// 混合裸键与含冒号引号键的工具集，覆盖导出与解析两侧
    const MIXED: &str =
        "[tools]\nnode = \"20\"\n\"npm:prettier\" = \"3\"\ntypescript = \"latest\"\n";

    #[test]
    fn parse_tools_accepts_bare_and_quoted_keys() {
        assert_eq!(
            parse_tools(MIXED),
            vec![
                PresetTool {
                    name: "node".into(),
                    version: "20".into()
                },
                PresetTool {
                    name: "npm:prettier".into(),
                    version: "3".into()
                },
                PresetTool {
                    name: "typescript".into(),
                    version: "latest".into()
                },
            ]
        );
    }

    #[test]
    fn parse_tools_stops_at_next_section() {
        // [tools] 之后的 [env] 不能被吞进来
        let toml = "[tools]\nnode = \"20\"\n\n[env]\nEDITOR = \"vim\"\npath = \"x\"\n";
        assert_eq!(
            parse_tools(toml),
            vec![PresetTool {
                name: "node".into(),
                version: "20".into()
            }]
        );
    }

    #[test]
    fn parse_tools_keeps_first_on_duplicate() {
        let toml = "[tools]\nnode = \"20\"\nnode = \"22\"\n";
        assert_eq!(
            parse_tools(toml),
            vec![PresetTool {
                name: "node".into(),
                version: "20".into()
            }]
        );
    }

    #[test]
    fn export_import_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("my.zenv.toml");
        let p = file.to_str().unwrap();

        let n = export(p, "Node 前端", "React / Vite", MIXED, "0.7.0").unwrap();
        assert_eq!(n, 3);

        let raw = std::fs::read_to_string(p).unwrap();
        assert!(raw.starts_with("# Z.Env 环境预设 v1"));
        // 含冒号的键必须带引号，否则不是合法 TOML
        assert!(raw.contains("\"npm:prettier\" = \"3\""));
        assert!(raw.contains("name = \"Node 前端\""));

        let back = import(p).unwrap();
        assert_eq!(back.name, "Node 前端");
        assert_eq!(back.description.as_deref(), Some("React / Vite"));
        assert_eq!(back.tools, parse_tools(MIXED));
        assert_eq!(back.path, p);
    }

    #[test]
    fn export_rejects_empty_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.toml");
        let err = export(
            p.to_str().unwrap(),
            "空预设",
            "",
            "[env]\nEDITOR = \"vim\"\n",
            "0.7.0",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("[tools]"), "错误信息应点明缺少 [tools]：{err}");
    }

    #[test]
    fn export_rejects_blank_name() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.toml");
        assert!(export(p.to_str().unwrap(), "   ", "", MIXED, "0.7.0").is_err());
    }

    #[test]
    fn import_falls_back_to_plain_mise_toml() {
        // 别人不用 Z.Env，直接手写的 mise.toml 也要能导入
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("rust-cli.zenv.toml");
        std::fs::write(&file, "[tools]\nrust = \"stable\"\n").unwrap();

        let got = import(file.to_str().unwrap()).unwrap();
        assert_eq!(got.name, "rust-cli");
        assert_eq!(got.description, None);
        assert_eq!(
            got.tools,
            vec![PresetTool {
                name: "rust".into(),
                version: "stable".into()
            }]
        );
    }

    #[test]
    fn import_rejects_empty_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("bad.toml");
        std::fs::write(&file, "# Z.Env 环境预设 v1\n\n[preset]\nname = \"x\"\n").unwrap();
        let err = import(file.to_str().unwrap()).unwrap_err().to_string();
        assert!(err.contains("[tools]"), "错误信息应点明缺少 [tools]：{err}");
    }

    #[test]
    fn import_rejects_newer_format_version() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("future.toml");
        std::fs::write(&file, "# Z.Env 环境预设 v2\n\n[tools]\nnode = \"20\"\n").unwrap();
        let err = import(file.to_str().unwrap()).unwrap_err().to_string();
        assert!(err.contains("v2"), "错误信息应点明版本：{err}");
    }

    #[test]
    fn import_rejects_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("empty.toml");
        std::fs::write(&file, "\n\n").unwrap();
        assert!(import(file.to_str().unwrap()).is_err());
    }

    #[test]
    fn import_reads_metadata_from_top_of_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("m.toml");
        std::fs::write(
            &file,
            "# Z.Env 环境预设 v1\n\n[preset]\nname = \"Go 服务\"\ndescription = \"微服务 / CLI\"\napp_version = \"0.7.0\"\n\n[tools]\ngo = \"1.27\"\n",
        )
        .unwrap();
        let got = import(file.to_str().unwrap()).unwrap();
        assert_eq!(got.name, "Go 服务");
        assert_eq!(got.description.as_deref(), Some("微服务 / CLI"));
        assert_eq!(got.tools.len(), 1);
    }

    #[test]
    fn export_escapes_quotes_in_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("q.toml");
        let p = file.to_str().unwrap();
        export(p, "带\"引号\"的名字", "", MIXED, "0.7.0").unwrap();
        assert_eq!(import(p).unwrap().name, "带\"引号\"的名字");
    }
}
