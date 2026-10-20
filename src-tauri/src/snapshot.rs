// 版本化环境快照：预览、安装、原子提交全局配置、激活验证；brew 清单只生成 Brewfile。
use crate::{config, env_center, error::AppError, mise, operations};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use tauri::AppHandle;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrewEntry {
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub format_version: u32,
    pub platform: String,
    pub tools: BTreeMap<String, Vec<String>>,
    pub global_env: toml::Table,
    pub brew: Vec<BrewEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub tools: usize,
    pub env_vars: usize,
    pub brew_packages: usize,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPlan {
    pub source: String,
    pub original: Option<String>,
    pub tools: BTreeMap<String, Vec<String>>,
    pub env_keys: Vec<String>,
    pub brew_packages: usize,
    pub warnings: Vec<String>,
}

pub fn export(path: &str, include_env: bool) -> Result<SnapshotSummary, AppError> {
    let tools = mise::list_global_tools()?
        .into_iter()
        .filter(|t| !t.active_versions.is_empty())
        .map(|t| (t.name, t.active_versions))
        .collect::<BTreeMap<_, _>>();
    let config_path =
        env_center::global_config_path().ok_or_else(|| AppError::Io("无法定位全局配置".into()))?;
    let raw = config::read_optional(&config_path)?.unwrap_or_default();
    let doc: toml::Table = toml::from_str(&raw).map_err(|e| AppError::Parse(e.to_string()))?;
    let global_env = if include_env {
        doc.get("env")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default()
    } else {
        toml::Table::new()
    };
    let brew_available = crate::env::collect_env_info()
        .pkg
        .iter()
        .any(|p| p.name == "brew" && p.available);
    let brew = if brew_available {
        crate::env::detect_system_versions("brew")?
            .into_iter()
            .map(|p| BrewEntry {
                name: p.name,
                kind: p.kind,
            })
            .collect()
    } else {
        Vec::new()
    };
    let snapshot = Snapshot {
        format_version: 2,
        platform: std::env::consts::OS.into(),
        tools,
        global_env,
        brew,
    };
    validate(&snapshot)?;
    let text = toml::to_string_pretty(&snapshot).map_err(|e| AppError::Parse(e.to_string()))?;
    config::atomic_write(Path::new(path), text.as_bytes())?;
    Ok(SnapshotSummary {
        tools: snapshot.tools.values().map(Vec::len).sum(),
        env_vars: snapshot.global_env.len(),
        brew_packages: snapshot.brew.len(),
        path: path.into(),
    })
}

fn read(path: &str) -> Result<String, AppError> {
    if std::fs::metadata(path)?.len() > 2 * 1024 * 1024 {
        return Err(AppError::Other("快照超过 2 MiB，已拒绝读取".into()));
    }
    Ok(std::fs::read_to_string(path)?)
}

fn parse(content: &str) -> Result<Snapshot, AppError> {
    let root: toml::Value = toml::from_str(content).map_err(|e| AppError::Parse(e.to_string()))?;
    let snapshot = if root.get("format_version").is_some() {
        toml::from_str::<Snapshot>(content).map_err(|e| AppError::Parse(e.to_string()))?
    } else {
        // 兼容应用此前生成的 v1；不把任意 TOML 或损坏文件当作空快照成功恢复。
        if !content.starts_with("# Z.Env 环境快照") {
            return Err(AppError::Parse("不是 Z.Env 环境快照".into()));
        }
        let tools = root
            .get("tools")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| AppError::Parse("缺少 tools".into()))?
            .iter()
            .map(|(k, v)| {
                v.as_str()
                    .map(|s| (k.clone(), vec![s.into()]))
                    .ok_or_else(|| AppError::Parse(format!("{k} 版本无效")))
            })
            .collect::<Result<_, _>>()?;
        let global_env = root
            .get("global_env")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        let brew = root
            .get("brew")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flat_map(|t| t.keys())
            .map(|name| BrewEntry {
                name: name.clone(),
                kind: "formula".into(),
            })
            .collect();
        Snapshot {
            format_version: 2,
            platform: "legacy".into(),
            tools,
            global_env,
            brew,
        }
    };
    validate(&snapshot)?;
    Ok(snapshot)
}

fn validate(snapshot: &Snapshot) -> Result<(), AppError> {
    if snapshot.format_version != 2 {
        return Err(AppError::Unsupported("快照版本不支持，请升级应用".into()));
    }
    for (tool, versions) in &snapshot.tools {
        if versions.is_empty() {
            return Err(AppError::Parse(format!("{tool} 缺少版本")));
        }
        for version in versions {
            mise::validate_spec(tool, version)?;
        }
    }
    for b in &snapshot.brew {
        crate::env::validate_package(&b.name)?;
        if !matches!(b.kind.as_str(), "formula" | "cask") {
            return Err(AppError::Parse("brew 类型无效".into()));
        }
    }
    Ok(())
}

pub fn preview(path: &str) -> Result<SnapshotPlan, AppError> {
    let source = read(path)?;
    let snapshot = parse(&source)?;
    let global =
        env_center::global_config_path().ok_or_else(|| AppError::Io("无法定位全局配置".into()))?;
    let original = config::read_optional(&global)?;
    merge_config(original.as_deref().unwrap_or(""), &snapshot)?;
    let mut warnings = Vec::new();
    if snapshot.platform != std::env::consts::OS {
        warnings.push("快照来自其他平台或旧格式；路径、工具版本和 brew 包可能不适用于本机".into());
    }
    if !snapshot.global_env.is_empty() {
        warnings.push("将覆盖同名全局变量；请确认其中的路径、密钥及 mise 环境指令可信".into());
    }
    if !snapshot.brew.is_empty() {
        warnings.push("brew 软件仅生成 Brewfile，需另行执行 brew bundle".into());
    }
    Ok(SnapshotPlan {
        source,
        original,
        tools: snapshot.tools,
        env_keys: snapshot.global_env.keys().cloned().collect(),
        brew_packages: snapshot.brew.len(),
        warnings,
    })
}

fn merge_config(original: &str, snapshot: &Snapshot) -> Result<String, AppError> {
    let mut doc = config::parse(original)?;
    if !snapshot.tools.is_empty() {
        let tools = config::table(&mut doc, "tools")?;
        for (name, versions) in &snapshot.tools {
            tools.insert(
                name,
                if versions.len() == 1 {
                    toml_edit::value(&versions[0])
                } else {
                    toml_edit::value(
                        versions
                            .iter()
                            .map(String::as_str)
                            .collect::<toml_edit::Array>(),
                    )
                },
            );
        }
    }
    if !snapshot.global_env.is_empty() {
        let serialized =
            toml::to_string(&snapshot.global_env).map_err(|e| AppError::Parse(e.to_string()))?;
        let incoming = config::parse(&serialized)?;
        let env = config::table(&mut doc, "env")?;
        for (key, value) in incoming.iter() {
            env.insert(key, value.clone());
        }
    }
    let text = doc.to_string();
    config::parse(&text)?;
    Ok(text)
}

fn brewfile_line(entry: &BrewEntry) -> String {
    format!(
        "{} \"{}\"\n",
        if entry.kind == "cask" { "cask" } else { "brew" },
        entry.name
    )
}

fn verify_active(snapshot: &Snapshot, tools: &[mise::ToolInfo]) -> Result<(), AppError> {
    for (name, versions) in &snapshot.tools {
        let tool = tools
            .iter()
            .find(|t| &t.name == name)
            .ok_or_else(|| AppError::Other(format!("{name} 未生效")))?;
        for version in versions {
            if !tool.versions.iter().any(|v| {
                v.version == *version && v.active == Some(true) && v.installed == Some(true)
            }) {
                return Err(AppError::Other(format!("{name}@{version} 激活验证失败")));
            }
        }
    }
    Ok(())
}

fn commit_config(
    global: &Path,
    original: Option<&str>,
    merged: &str,
    verify: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    config::update(global, Some(original), |_| Ok(merged.into()))?;
    if let Err(error) = verify() {
        config::update(global, Some(Some(merged)), |_| {
            Ok(original.unwrap_or("").into())
        })
        .map_err(|rollback| {
            AppError::Other(format!(
                "{error}；自动回滚失败：{rollback}，请使用配置备份恢复"
            ))
        })?;
        return Err(AppError::Other(format!("{error}；全局配置已回滚")));
    }
    Ok(())
}

pub fn restore(
    app: &AppHandle,
    path: &str,
    expected: &str,
    original: Option<&str>,
    job: &str,
) -> Result<String, AppError> {
    let source = read(path)?;
    if source != expected {
        return Err(AppError::Other("快照已改变，请重新预览".into()));
    }
    let snapshot = parse(&source)?;
    let global =
        env_center::global_config_path().ok_or_else(|| AppError::Io("无法定位全局配置".into()))?;
    if config::read_optional(&global)?.as_deref() != original {
        return Err(AppError::Other("全局配置已改变，请重新预览".into()));
    }
    let merged = merge_config(original.unwrap_or(""), &snapshot)?;
    let mut failures = Vec::new();
    for (tool, versions) in &snapshot.tools {
        for version in versions {
            if operations::cancelled(job) {
                return Err(AppError::Other("恢复已取消，全局配置未修改".into()));
            }
            if mise::install_version_streaming(app, tool, version, job).is_err() {
                failures.push(format!("{tool}@{version}"));
            }
        }
    }
    if !failures.is_empty() {
        return Err(AppError::Other(format!(
            "这些工具安装失败：{}；全局配置未修改，已成功下载的工具予以保留",
            failures.join("、")
        )));
    }
    if operations::cancelled(job) {
        return Err(AppError::Other("恢复已取消，全局配置未修改".into()));
    }
    let mut report = String::new();
    if !snapshot.brew.is_empty() {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| AppError::Io("无法定位主目录".into()))?;
        let file = Path::new(&home).join(".zenv/Brewfile.snapshot");
        let text: String = snapshot.brew.iter().map(brewfile_line).collect();
        config::atomic_write(&file, text.as_bytes())?;
        report = format!(
            "\nbrew 清单已生成 {}，请执行 brew bundle --file='{}'",
            file.display(),
            file.display().to_string().replace('\'', "'\\''")
        );
    }
    commit_config(&global, original, &merged, || {
        mise::list_global_tools().and_then(|tools| verify_active(&snapshot, &tools))
    })?;
    Ok(format!("运行时激活与全局配置恢复已验证{report}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Snapshot {
        Snapshot {
            format_version: 2,
            platform: "macos".into(),
            tools: [("node".into(), vec!["20.1".into(), "22.1".into()])].into(),
            global_env: toml::from_str("A='中文'\n_.path=['/opt/bin']").unwrap(),
            brew: vec![BrewEntry {
                name: "docker-desktop".into(),
                kind: "cask".into(),
            }],
        }
    }
    #[test]
    fn roundtrip_preserves_all_versions_and_complex_env() {
        let snapshot = sample();
        let encoded = toml::to_string(&snapshot).unwrap();
        let decoded = parse(&encoded).unwrap();
        let merged = merge_config(
            "[tools]\npython='3.13'\n[settings]\nexperimental=true",
            &decoded,
        )
        .unwrap();
        let doc: toml::Value = toml::from_str(&merged).unwrap();
        assert_eq!(doc["tools"]["node"].as_array().unwrap().len(), 2);
        assert_eq!(doc["tools"]["python"].as_str(), Some("3.13"));
        assert_eq!(doc["env"]["A"].as_str(), Some("中文"));
        assert_eq!(brewfile_line(&decoded.brew[0]), "cask \"docker-desktop\"\n");
    }
    #[test]
    fn legacy_supported_invalid_future_and_injected_package_rejected() {
        let legacy = "# Z.Env 环境快照（机器生成）\n[tools]\nnode='20.1'\n[global_env]\nA='b'\n[brew]\nwget='installed'";
        assert_eq!(parse(legacy).unwrap().tools["node"], vec!["20.1"]);
        assert!(parse("").is_err());
        let mut snapshot = sample();
        snapshot.format_version = 99;
        assert!(parse(&toml::to_string(&snapshot).unwrap()).is_err());
        snapshot.format_version = 2;
        snapshot.brew[0].name = "#{system('whoami')}".into();
        assert!(validate(&snapshot).is_err());
    }
    #[test]
    fn installed_but_inactive_does_not_pass_restore_verification() {
        let snapshot = sample();
        let tools = vec![mise::ToolInfo {
            name: "node".into(),
            active_versions: vec![],
            versions: vec![mise::ToolVersion {
                version: "20.1".into(),
                requested_version: None,
                install_path: None,
                installed: Some(true),
                active: Some(false),
            }],
        }];
        assert!(verify_active(&snapshot, &tools).is_err());
    }
    #[test]
    fn commit_checks_activation_and_rolls_back_without_overwriting_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "[tools]\nnode='20'\n";
        let merged = "[tools]\nnode='22'\n";
        std::fs::write(&path, original).unwrap();
        let failure = || Err(AppError::Other("激活失败".into()));
        assert!(commit_config(&path, Some(original), merged, failure).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let result = commit_config(&path, Some(original), merged, || {
            std::fs::write(&path, "# external\n").unwrap();
            failure()
        });
        assert!(result.unwrap_err().to_string().contains("回滚失败"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# external\n");
        commit_config(&path, Some("# external\n"), merged, || Ok(())).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), merged);
    }
    #[test]
    fn all_requested_versions_must_be_active_and_installed() {
        let snapshot = sample();
        let mut tools = vec![mise::ToolInfo {
            name: "node".into(),
            active_versions: snapshot.tools["node"].clone(),
            versions: snapshot.tools["node"]
                .iter()
                .map(|version| mise::ToolVersion {
                    version: version.clone(),
                    requested_version: None,
                    install_path: None,
                    installed: Some(true),
                    active: Some(true),
                })
                .collect(),
        }];
        assert!(verify_active(&snapshot, &tools).is_ok());
        tools[0].versions[1].installed = Some(false);
        assert!(verify_active(&snapshot, &tools).is_err());
    }
}
