// 项目编辑会话：读取时确定唯一目标，保存时核对原始内容；新项目可直接初始化。
use crate::{config, error::AppError};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocument {
    pub path: String,
    pub content: String,
    pub original: Option<String>,
}

pub(crate) fn target(path: &str) -> Result<PathBuf, AppError> {
    if path.trim().is_empty() {
        return Err(AppError::Other("请先选择项目".into()));
    }
    let p = Path::new(path);
    if p.is_dir() {
        let dir = p.canonicalize()?;
        return Ok(
            if !dir.join("mise.toml").exists() && dir.join(".mise.toml").exists() {
                dir.join(".mise.toml")
            } else {
                dir.join("mise.toml")
            },
        );
    }
    if !matches!(
        p.file_name().and_then(|s| s.to_str()),
        Some("mise.toml" | ".mise.toml" | ".tool-versions")
    ) {
        return Err(AppError::Unsupported(
            "请选择项目目录、mise.toml、.mise.toml 或 .tool-versions".into(),
        ));
    }
    let parent = p
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = if p.file_name().and_then(|s| s.to_str()) == Some(".tool-versions") {
        "mise.toml"
    } else {
        p.file_name().unwrap().to_str().unwrap()
    };
    Ok(parent.canonicalize()?.join(name))
}

pub fn read(path: &str) -> Result<ProjectDocument, AppError> {
    let target = target(path)?;
    let original = config::read_optional(&target)?;
    let mut content = original.clone().unwrap_or_default();
    if original.is_none() {
        if let Some(legacy) = config::read_optional(&target.with_file_name(".tool-versions"))? {
            let mut doc = toml_edit::DocumentMut::new();
            let tools = config::table(&mut doc, "tools")?;
            for line in legacy.lines() {
                let mut words = line.split('#').next().unwrap_or("").split_whitespace();
                if let Some(name) = words.next() {
                    let versions: Vec<_> = words.collect();
                    if versions.is_empty() {
                        return Err(AppError::Parse(format!(
                            ".tool-versions 中 {name} 缺少版本"
                        )));
                    }
                    let value = if versions.len() == 1 {
                        toml_edit::value(versions[0])
                    } else {
                        toml_edit::value(versions.into_iter().collect::<toml_edit::Array>())
                    };
                    tools.insert(name, value);
                }
            }
            content = doc.to_string();
        }
    }
    Ok(ProjectDocument {
        path: target.display().to_string(),
        content,
        original,
    })
}

pub fn save(path: &str, content: &str, original: Option<&str>) -> Result<String, AppError> {
    let target = target(path)?;
    config::update(&target, Some(original), |_| Ok(content.into()))?;
    Ok(format!("已保存到 {}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_load_preserves_hidden_config_destination() {
        let dir = tempfile::tempdir().unwrap();
        let hidden = dir.path().join(".mise.toml");
        std::fs::write(&hidden, "[tools]\nnode='22'\n").unwrap();
        let doc = read(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(Path::new(&doc.path), hidden.canonicalize().unwrap());
        save(&doc.path, "[tools]\nnode='24'\n", doc.original.as_deref()).unwrap();
        assert!(!dir.path().join("mise.toml").exists());
    }
    #[test]
    fn new_project_and_legacy_import_have_explicit_destination() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(".tool-versions");
        std::fs::write(&legacy, "node 20 22\npython 3.13 # comment\n").unwrap();
        let doc = read(legacy.to_str().unwrap()).unwrap();
        assert!(doc.path.ends_with("mise.toml"));
        assert!(doc.original.is_none());
        let parsed: toml::Value = toml::from_str(&doc.content).unwrap();
        assert_eq!(parsed["tools"]["node"].as_array().unwrap().len(), 2);
        save(&doc.path, &doc.content, doc.original.as_deref()).unwrap();
        assert!(save(&doc.path, "[tools]", None).is_err());
        assert_eq!(
            std::fs::read_to_string(legacy).unwrap(),
            "node 20 22\npython 3.13 # comment\n"
        );
    }
}
