// 环境变量使用 TOML 语法树编辑，保留其他配置；文件错误不再伪装成空配置。
use crate::{config, error::AppError};
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

pub fn list() -> Result<EnvCenterSnapshot, AppError> {
    let path = global_config_path().ok_or_else(|| AppError::Io("无法定位全局配置".into()))?;
    let content = config::read_optional(&path)?;
    let entries = parse_env_section(content.as_deref().unwrap_or(""))?;
    let conflicts = entries
        .iter()
        .filter(|e| e.kind == "simple" && e.key != "PATH")
        .filter_map(|e| {
            std::env::var(&e.key)
                .ok()
                .filter(|s| s != &e.value)
                .map(|system_value| EnvConflict {
                    key: e.key.clone(),
                    mise_value: e.value.clone(),
                    system_value,
                })
        })
        .collect();
    let mut system_env: Vec<_> = std::env::vars()
        .map(|(key, value)| SystemEnvVar { key, value })
        .collect();
    system_env.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(EnvCenterSnapshot {
        config_path: path.display().to_string(),
        exists: content.is_some(),
        entries,
        conflicts,
        system_env,
    })
}

pub fn validate_key(key: &str) -> Result<(), AppError> {
    let mut chars = key.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        || key == "_"
    {
        return Err(AppError::Unsupported(
            "变量名必须由字母、数字、下划线组成，且不能以数字开头".into(),
        ));
    }
    Ok(())
}

pub fn set(key: &str, value: &str) -> Result<String, AppError> {
    let path = global_config_path().ok_or_else(|| AppError::Io("无法定位全局配置".into()))?;
    config::update(&path, None, |s| set_env_in_config(s, key, value))?;
    Ok(format!("已写入 {key}（全局 [env]）"))
}

pub fn remove(key: &str) -> Result<String, AppError> {
    let path = global_config_path().ok_or_else(|| AppError::Io("无法定位全局配置".into()))?;
    config::update(&path, None, |s| remove_env_from_config(s, key))?;
    Ok(format!("已移除 {key}（全局 [env]）"))
}

pub fn set_env_in_config(content: &str, key: &str, value: &str) -> Result<String, AppError> {
    validate_key(key)?;
    let mut doc = config::parse(content)?;
    let table = config::table(&mut doc, "env")?;
    if table
        .get(key)
        .is_some_and(|i| i.is_table_like() || i.is_array())
    {
        return Err(AppError::Unsupported("高级环境变量请在编辑器中修改".into()));
    }
    let mut replacement = toml_edit::Value::from(value);
    if let Some(old) = table.get(key).and_then(toml_edit::Item::as_value) {
        *replacement.decor_mut() = old.decor().clone();
    }
    table.insert(key, toml_edit::Item::Value(replacement));
    Ok(doc.to_string())
}

pub fn remove_env_from_config(content: &str, key: &str) -> Result<String, AppError> {
    validate_key(key)?;
    let mut doc = config::parse(content)?;
    let table = config::table(&mut doc, "env")?;
    if table.remove(key).is_none() {
        return Err(AppError::Unsupported(format!("变量 {key} 不存在")));
    }
    Ok(doc.to_string())
}

pub fn parse_env_section(content: &str) -> Result<Vec<GlobalEnvEntry>, AppError> {
    let doc = config::parse(content)?;
    let Some(item) = doc.get("env") else {
        return Ok(Vec::new());
    };
    let table = item
        .as_table_like()
        .ok_or_else(|| AppError::Parse("env 必须为表".into()))?;
    Ok(table
        .iter()
        .map(|(key, value)| {
            let scalar = value
                .as_value()
                .filter(|v| !v.is_array() && !v.is_inline_table());
            GlobalEnvEntry {
                key: key.into(),
                value: scalar
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string().trim().to_string())
                    })
                    .unwrap_or_else(|| "表/数组等高级值，请在文件中手动编辑".into()),
                kind: if scalar.is_some() && key != "_" {
                    "simple"
                } else {
                    "complex"
                }
                .into(),
            }
        })
        .collect())
}

/// 导出文件使用的基本字符串转义，覆盖控制字符与 Unicode。
pub(crate) fn escape_basic(value: &str) -> String {
    let encoded = serde_json::to_string(value).expect("字符串序列化不会失败");
    encoded[1..encoded.len() - 1].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_roundtrips_unicode_newlines_and_commented_headers() {
        let original = "[tools]\nnode = '20'\n[env] # 保留\nGREETING = 'old'\n";
        let text = set_env_in_config(original, "GREETING", "中文\n第二行\t").unwrap();
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(parsed["env"]["GREETING"].as_str(), Some("中文\n第二行\t"));
        assert_eq!(parse_env_section(&text).unwrap()[0].value, "中文\n第二行\t");
        assert!(text.contains("# 保留"));
        assert!(remove_env_from_config(&text, "GREETING")
            .unwrap()
            .contains("node = '20'"));
    }
    #[test]
    fn malformed_documents_and_unsafe_keys_are_rejected() {
        assert!(set_env_in_config("[bad", "A", "b").is_err());
        for key in ["", "A B", "A.B", "1A", "_"] {
            assert!(set_env_in_config("", key, "x").is_err());
        }
        assert!(set_env_in_config("[env]\nX = { value = 'complex' }", "X", "x").is_err());
    }
    #[test]
    fn complex_values_and_quoted_keys_are_not_lost() {
        let text = "[env]\n'A' = '中文'\n_.path = [\n 'one', 'two'\n]\nB = 3\n";
        let rows = parse_env_section(text).unwrap();
        assert_eq!(rows[0].key, "A");
        assert_eq!(rows[1].kind, "complex");
        let out = set_env_in_config(text, "A", "new").unwrap();
        let value: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(value["env"]["_"]["path"].as_array().unwrap().len(), 2);
    }
}
