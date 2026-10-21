//! Python mise 配置绑定：项目/全局 `[tools] python`，带差异、备份与外部修改检测。
//! 本切片只做绑定写入；解绑、收据与自动补偿留待下一片。
use super::*;
use crate::{config, error::AppError};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Facts {
    config_path: PathBuf,
    key: String,
    before: Option<String>,
    after: String,
    original: Option<String>,
}

#[derive(Clone)]
pub(super) struct BindingSpec {
    pub version: String,
    pub locator: String,
    pub config_path: PathBuf,
    pub diff: ConfigurationDiff,
    pub impact: Vec<String>,
    facts: Facts,
}

fn denied(message: &'static str) -> PlanError {
    PlanError::Capability(message)
}

fn stable(value: &str) -> bool {
    crate::providers::python::stable(value)
}

fn config_target(binding: &BindingOptions, project: &Path) -> Result<PathBuf, PlanError> {
    match binding.scope {
        BindingScope::Project => {
            let raw = binding
                .project_path
                .as_deref()
                .ok_or(denied("项目作用域需要项目目录或配置文件"))?;
            if !Path::new(raw).is_absolute() {
                return Err(denied("项目路径必须是绝对路径"));
            }
            crate::project::target(raw).map_err(|_| PlanError::EvidenceUnavailable)
        }
        BindingScope::Global => Ok(project.to_path_buf()),
        _ => Err(denied("Python 绑定只支持项目或全局作用域")),
    }
}

fn tools_value(original: &str) -> Result<(String, Option<String>), PlanError> {
    let doc = config::parse(original).map_err(|_| PlanError::EvidenceUnavailable)?;
    let Some(tools) = doc.get("tools") else {
        return Ok(("python".into(), None));
    };
    let Some(tools) = tools.as_table_like() else {
        return Err(denied("mise tools 不是表，不能写入 Python 选择"));
    };
    let python = tools.get("python");
    let core = tools.get("core:python");
    if python.is_some() && core.is_some() {
        return Err(denied("tools 同时存在 python 与 core:python，未推断优先级"));
    }
    let (key, value) = if let Some(value) = python {
        ("python", Some(value))
    } else if let Some(value) = core {
        ("core:python", Some(value))
    } else {
        ("python", None)
    };
    match value {
        None => Ok((key.into(), None)),
        Some(value) => value
            .as_str()
            .map(|value| (key.into(), Some(value.to_owned())))
            .ok_or(denied("现有 Python 工具选择不是字符串，保持只读")),
    }
}

fn next_content(
    original: Option<&str>,
    key: &str,
    value: &str,
) -> Result<(String, Option<String>), PlanError> {
    let content = original.unwrap_or("");
    let (current_key, current_value) = tools_value(content)?;
    if current_key != key {
        return Err(PlanError::Preconditions);
    }
    let mut doc = config::parse(content).map_err(|_| PlanError::EvidenceUnavailable)?;
    let tools = config::table(&mut doc, "tools").map_err(|_| PlanError::EvidenceUnavailable)?;
    tools.insert(key, toml_edit::value(value));
    Ok((doc.to_string(), current_value))
}

pub(super) fn prepare_bind(
    target: &Installation,
    snapshot: &InventorySnapshot,
    binding: &BindingOptions,
    global_config: &Path,
) -> Result<BindingSpec, PlanError> {
    if binding.strategy != BindingStrategy::Direct {
        return Err(denied("Python 绑定只支持 mise 直接配置选择"));
    }
    if target.provider_package.provider_id != ProviderId::Mise
        || target.provider_package.package_id != "python"
        || target.tool_id != ToolId::from("python")
        || target.health != Health::Healthy
        || target.stale
    {
        return Err(denied("绑定需要当前健康的 mise 托管 Python 实例"));
    }
    let version = target
        .version
        .as_deref()
        .filter(|value| stable(value))
        .ok_or(denied("绑定需要精确稳定 CPython 版本"))?
        .to_owned();
    if !snapshot.installations.iter().any(|item| item == target) {
        return Err(PlanError::Stale);
    }
    let config_path = if binding.scope == BindingScope::Global {
        global_config.to_path_buf()
    } else {
        config_target(binding, global_config)?
    };
    if !config_path.is_absolute() {
        return Err(PlanError::EvidenceUnavailable);
    }
    let original =
        config::read_optional(&config_path).map_err(|_| PlanError::EvidenceUnavailable)?;
    let (key, before) = tools_value(original.as_deref().unwrap_or(""))?;
    if before.as_deref() == Some(version.as_str()) {
        return Err(denied("该配置已经绑定同一精确版本"));
    }
    let (next, current) = next_content(original.as_deref(), &key, &version)?;
    debug_assert_eq!(current, before);
    if next.len() > 1024 * 1024 {
        return Err(denied("配置写入超出预算"));
    }
    let diff = ConfigurationDiff {
        path: config_path.to_string_lossy().into(),
        field: format!("tools.{key}"),
        before: before.clone(),
        after: Some(version.clone()),
    };
    let scope = if binding.scope == BindingScope::Global {
        "全局"
    } else {
        "项目"
    };
    let impact = vec![
        format!("写入{scope} mise 配置：{}", config_path.display()),
        "只修改 [tools] Python 选择，保留其他工具、注释与结构；写入前保留 .zenv-backup".into(),
        "配置写入成功不等于 mise 实际解析、终端或 IDE 生效；需另行核对".into(),
        "不改 shell 配置、不卸载任何 Python，也不改写项目虚拟环境".into(),
    ];
    Ok(BindingSpec {
        version,
        locator: target.locator.clone(),
        config_path: config_path.clone(),
        diff: diff.clone(),
        impact,
        facts: Facts {
            config_path: config_path.clone(),
            key,
            before,
            after: diff.after.clone().unwrap_or_default(),
            original,
        },
    })
}

impl BindingSpec {
    pub fn revalidate(
        &self,
        target: Option<&Installation>,
        snapshot: &InventorySnapshot,
    ) -> Result<(), PlanError> {
        let target = target.ok_or(PlanError::Preconditions)?;
        if target.version.as_deref() != Some(self.version.as_str())
            || target.locator != self.locator
            || !snapshot.installations.iter().any(|item| item == target)
        {
            return Err(PlanError::Preconditions);
        }
        let current = config::read_optional(&self.facts.config_path)
            .map_err(|_| PlanError::EvidenceUnavailable)?;
        if current != self.facts.original {
            return Err(PlanError::Preconditions);
        }
        let (key, value) = tools_value(current.as_deref().unwrap_or(""))?;
        if key != self.facts.key || value != self.facts.before {
            return Err(PlanError::Preconditions);
        }
        Ok(())
    }
    pub fn apply(&self) -> Result<(), PlanError> {
        let current = config::read_optional(&self.facts.config_path)
            .map_err(|_| PlanError::EvidenceUnavailable)?;
        if current != self.facts.original {
            return Err(PlanError::Preconditions);
        }
        let (next, _) = next_content(current.as_deref(), &self.facts.key, &self.facts.after)?;
        config::replace_checked(
            &self.facts.config_path,
            self.facts.original.as_deref(),
            Some(&next),
        )
        .map_err(|error| match error {
            AppError::Other(message) if message.contains("外部修改") => {
                PlanError::Preconditions
            }
            _ => PlanError::Command,
        })
    }
    pub fn verify(&self, snapshot: &InventorySnapshot) -> Result<(), PlanError> {
        let current =
            config::read_optional(&self.facts.config_path).map_err(|_| PlanError::Verification)?;
        let (key, value) =
            tools_value(current.as_deref().unwrap_or("")).map_err(|_| PlanError::Verification)?;
        if key != self.facts.key || value.as_deref() != Some(self.facts.after.as_str()) {
            return Err(PlanError::Verification);
        }
        snapshot
            .installations
            .iter()
            .find(|item| {
                item.provider_package.provider_id == ProviderId::Mise
                    && item.tool_id == ToolId::from("python")
                    && item.version.as_deref() == Some(self.version.as_str())
                    && item.locator == self.locator
                    && !item.stale
                    && item.health == Health::Healthy
            })
            .map(|_| ())
            .ok_or(PlanError::Verification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Health, InstallationId, Ownership, PackageKind, Platform, ProviderPackage, ProviderReport,
        ScanCoverage,
    };
    use std::fs;

    fn global() -> PathBuf {
        tempfile::tempdir().unwrap().keep().join("global.toml")
    }

    fn target() -> Installation {
        Installation {
            installation_id: InstallationId::from("python-mise-3.13.15"),
            provider_package: ProviderPackage {
                provider_id: ProviderId::Mise,
                package_id: "python".into(),
                kind: PackageKind::Runtime,
                platform: Platform::Macos,
            },
            tool_id: ToolId::from("python"),
            locator: "/fixture/.local/share/mise/installs/python/3.13.15".into(),
            executable_path: None,
            canonical_executable_path: None,
            version: Some("3.13.15".into()),
            architecture: Some("arm64".into()),
            health: Health::Healthy,
            ownership: Ownership::Observed,
            evidence: vec![],
            stale: false,
        }
    }

    fn snapshot(target: &Installation) -> InventorySnapshot {
        InventorySnapshot {
            schema_version: 1,
            request_id: "snapshot".into(),
            observed_at: 0,
            tools: vec![],
            providers: vec![ProviderReport {
                provider_id: ProviderId::Mise,
                status: ProviderStatus::Ok,
                observed_at: 0,
                stale: false,
                coverage: ScanCoverage {
                    tool_ids: vec![ToolId::from("python")],
                    complete: true,
                    truncated: false,
                },
                error: None,
            }],
            installations: vec![target.clone()],
            aliases: vec![],
        }
    }

    fn options(scope: BindingScope, project: Option<&Path>) -> BindingOptions {
        BindingOptions {
            strategy: BindingStrategy::Direct,
            scope,
            project_path: project.map(|path| path.to_string_lossy().into()),
        }
    }

    #[test]
    fn project_binding_writes_exact_version_and_preserves_other_tools() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("mise.toml"),
            "[tools]\nnode = \"22\"\n\n[env]\nA = \"1\"\n",
        )
        .unwrap();
        let target = target();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
        )
        .unwrap();
        assert_eq!(spec.diff.field, "tools.python");
        assert_eq!(spec.diff.before, None);
        assert_eq!(spec.diff.after.as_deref(), Some("3.13.15"));
        spec.apply().unwrap();
        let text = fs::read_to_string(directory.path().join("mise.toml")).unwrap();
        assert!(text.contains("python = \"3.13.15\""));
        assert!(text.contains("node = \"22\""));
        assert!(text.contains("A = \"1\""));
        spec.verify(&snapshot(&target)).unwrap();
    }

    #[test]
    fn binding_replaces_existing_value_and_updates_diff() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("mise.toml"),
            "[tools]\npython = \"3.12\"\n",
        )
        .unwrap();
        let target = target();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
        )
        .unwrap();
        assert_eq!(spec.diff.before.as_deref(), Some("3.12"));
        spec.apply().unwrap();
        let text = fs::read_to_string(directory.path().join("mise.toml")).unwrap();
        assert!(text.contains("python = \"3.13.15\""));
        assert!(!text.contains("3.12"));
    }

    #[test]
    fn global_binding_uses_given_global_config_path() {
        let global = global();
        let target = target();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Global, None),
            &global,
        )
        .unwrap();
        spec.apply().unwrap();
        let text = fs::read_to_string(&global).unwrap();
        assert!(text.contains("python = \"3.13.15\""));
    }

    #[test]
    fn same_version_relative_project_and_non_mise_target_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("mise.toml"),
            "[tools]\npython = \"3.13.15\"\n",
        )
        .unwrap();
        let target = target();
        assert!(prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
        )
        .is_err());
        assert!(prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(Path::new("relative"))),
            &global(),
        )
        .is_err());
        let mut uv = target.clone();
        uv.provider_package.provider_id = ProviderId::Uv;
        assert!(prepare_bind(
            &uv,
            &snapshot(&uv),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
        )
        .is_err());
    }

    #[test]
    fn external_config_change_stops_apply_and_verify() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mise.toml");
        fs::write(&path, "[tools]\nnode = \"22\"\n").unwrap();
        let target = target();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
        )
        .unwrap();
        fs::write(&path, "[tools]\nnode = \"20\"\n").unwrap();
        assert!(matches!(spec.apply(), Err(PlanError::Preconditions)));
        assert!(matches!(
            spec.revalidate(Some(&target), &snapshot(&target)),
            Err(PlanError::Preconditions)
        ));
    }

    #[test]
    fn existing_core_python_key_is_reused_and_conflicting_keys_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("mise.toml"),
            "[tools]\n\"core:python\" = \"3.12\"\n",
        )
        .unwrap();
        let target = target();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
        )
        .unwrap();
        assert_eq!(spec.diff.field, "tools.core:python");
        spec.apply().unwrap();
        let text = fs::read_to_string(directory.path().join("mise.toml")).unwrap();
        assert!(text.contains("\"core:python\" = \"3.13.15\""));
        assert!(!text.contains("python = \"3.13.15\"\n"));

        fs::write(
            directory.path().join("mise.toml"),
            "[tools]\npython = \"3.12\"\n\"core:python\" = \"3.13\"\n",
        )
        .unwrap();
        assert!(prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
        )
        .is_err());
    }
}
