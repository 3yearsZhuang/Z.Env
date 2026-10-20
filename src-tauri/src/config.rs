// 配置服务：结构化 TOML 修改、并发修改检测、备份与同目录原子替换。
use crate::error::AppError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use toml_edit::{DocumentMut, Item, Table, Value};

static CONFIG_LOCK: Mutex<()> = Mutex::new(());

pub fn parse(content: &str) -> Result<DocumentMut, AppError> {
    content
        .parse()
        .map_err(|e| AppError::Parse(format!("TOML 无效：{e}")))
}

pub fn read_optional(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".zenv-backup");
    name.into()
}

/// 不覆盖符号链接本身；失败时原文件仍可读取，备份只保存上一次成功修改前的内容。
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), AppError> {
    let target = if std::fs::symlink_metadata(path).is_ok() {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    if let Ok(meta) = std::fs::metadata(&target) {
        temp.as_file().set_permissions(meta.permissions())?;
    }
    temp.write_all(content)?;
    temp.as_file().sync_all()?;
    match std::fs::read(&target) {
        Ok(old) => {
            let mut backup = tempfile::NamedTempFile::new_in(dir)?;
            backup.write_all(&old)?;
            backup.as_file().sync_all()?;
            backup
                .persist(backup_path(&target))
                .map_err(|e| AppError::Io(e.to_string()))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    temp.persist(&target)
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

/// 所有应用内配置修改串行执行；expected 用于拒绝覆盖编辑期间的外部修改。
pub fn update(
    path: &Path,
    expected: Option<Option<&str>>,
    edit: impl FnOnce(&str) -> Result<String, AppError>,
) -> Result<String, AppError> {
    update_text(path, expected, |old| {
        let content = edit(old)?;
        parse(&content)?;
        Ok(content)
    })
}

pub fn update_text(
    path: &Path,
    expected: Option<Option<&str>>,
    edit: impl FnOnce(&str) -> Result<String, AppError>,
) -> Result<String, AppError> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let old = read_optional(path)?;
    if expected.is_some_and(|e| e != old.as_deref()) {
        return Err(AppError::Other(
            "配置已在别处修改，请重新载入后再保存；本次未写入".into(),
        ));
    }
    let content = edit(old.as_deref().unwrap_or(""))?;
    if read_optional(path)? != old {
        return Err(AppError::Other("保存前检测到外部修改，请重新载入".into()));
    }
    atomic_write(path, content.as_bytes())?;
    Ok(content)
}

/// 绑定补偿的比较后替换；删除新建配置前同样保留备份，不覆盖外部修改。
pub fn replace_checked(
    path: &Path,
    expected: Option<&str>,
    next: Option<&str>,
) -> Result<(), AppError> {
    if let Some(next) = next {
        update_text(path, Some(expected), |_| Ok(next.into()))?;
        return Ok(());
    }
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let old = read_optional(path)?;
    if old.as_deref() != expected {
        return Err(AppError::Other("配置已被外部修改，停止补偿".into()));
    }
    if let Some(old) = old {
        let parent = path
            .parent()
            .ok_or_else(|| AppError::Other("配置没有父目录".into()))?;
        let mut backup = tempfile::NamedTempFile::new_in(parent)?;
        backup.write_all(old.as_bytes())?;
        backup.as_file().sync_all()?;
        if read_optional(path)?.as_deref() != expected {
            return Err(AppError::Other("配置已被外部修改，停止补偿".into()));
        }
        backup
            .persist(backup_path(path))
            .map_err(|e| AppError::Io(e.to_string()))?;
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn table<'a>(
    doc: &'a mut DocumentMut,
    name: &str,
) -> Result<&'a mut dyn toml_edit::TableLike, AppError> {
    if !doc.contains_key(name) {
        doc[name] = Item::Table(Table::new());
    }
    doc[name]
        .as_table_like_mut()
        .ok_or_else(|| AppError::Parse(format!("{name} 必须为 TOML 表")))
}

pub fn bind_path(content: &str, path: &str) -> Result<String, AppError> {
    let mut doc = parse(content)?;
    let env = table(&mut doc, "env")?;
    if !env.contains_key("_") {
        let mut t = Table::new();
        t.set_dotted(true);
        env.insert("_", Item::Table(t));
    }
    let directives = env
        .get_mut("_")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| AppError::Parse("env._ 必须为表".into()))?;
    if !directives.contains_key("path") {
        directives.insert("path", toml_edit::value(toml_edit::Array::new()));
    }
    let paths = directives
        .get_mut("path")
        .and_then(Item::as_array_mut)
        .ok_or_else(|| AppError::Parse("env._.path 必须为数组".into()))?;
    if !paths.iter().any(|v| v.as_str() == Some(path)) {
        paths.push(path);
    }
    let result = doc.to_string();
    parse(&result)?;
    Ok(result)
}

pub fn replace_tools(
    content: &str,
    tools: &std::collections::BTreeMap<String, String>,
) -> Result<String, AppError> {
    let mut doc = parse(content)?;
    let table = table(&mut doc, "tools")?;
    let removed: Vec<_> = table
        .iter()
        .filter(|(k, _)| !tools.contains_key(*k))
        .map(|(k, _)| k.to_string())
        .collect();
    for key in removed {
        table.remove(&key);
    }
    for (name, version) in tools {
        match table.get_mut(name) {
            Some(item) if item.is_table_like() => {
                item.as_table_like_mut()
                    .unwrap()
                    .insert("version", toml_edit::value(version));
            }
            Some(Item::Value(value)) => {
                let decor = value.decor().clone();
                *value = Value::from(version.as_str());
                *value.decor_mut() = decor;
            }
            _ => {
                table.insert(name, toml_edit::value(version));
            }
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_binding_handles_windows_empty_multiline_and_comments() {
        for content in [
            "[env] # 注释\n_.path = []\n",
            "[env]\n_.path = [\n  '/old',\n]\n",
        ] {
            let path = r"C:\Users\中文\scoop\apps\nodejs\current";
            let once = bind_path(content, path).unwrap();
            let twice = bind_path(&once, path).unwrap();
            assert_eq!(once, twice);
            let value: toml::Value = toml::from_str(&twice).unwrap();
            assert!(value["env"]["_"]["path"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some(path)));
        }
    }

    #[test]
    fn tool_edit_preserves_comments_metadata_and_other_sections() {
        let content =
            "[tools] # pinned\nnode = { version = '20', os = ['linux'] }\n[env]\nA = '中文'\n";
        let out = replace_tools(content, &[("node".into(), "22".into())].into()).unwrap();
        let value: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(value["tools"]["node"]["version"].as_str(), Some("22"));
        assert_eq!(value["tools"]["node"]["os"][0].as_str(), Some("linux"));
        assert!(out.contains("# pinned"));
        assert_eq!(value["env"]["A"].as_str(), Some("中文"));
    }

    #[test]
    fn checked_write_rejects_stale_and_invalid_content_and_keeps_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        let original = "[tools]\nnode = '20'\n";
        std::fs::write(&path, original).unwrap();
        assert!(update(&path, Some(Some("stale")), |_| Ok("".into())).is_err());
        assert!(update(&path, Some(Some(original)), |_| Ok("[bad".into())).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        update(&path, Some(Some(original)), |_| {
            Ok("[tools]\nnode = '22'\n".into())
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            original
        );
    }
}
