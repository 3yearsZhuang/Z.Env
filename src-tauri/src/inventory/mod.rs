//! 多来源清单的纯合并内核；输入均为观测，不访问文件、启动进程或修复环境。

pub mod query;

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    canonical_tool_id, node_tool_definition, python_tool_definition, tool_id_for_package,
    AliasEvidence, AliasEvidenceKind, CanonicalExecutableEvidence, Health, Installation,
    InstallationId, InstallationObservation, InventorySnapshot, Ownership, PackageKind, Platform,
    ProviderId, ProviderScan, ProviderStatus, ToolDefinition, ToolId, INVENTORY_SCHEMA_VERSION,
};

/// 时间以毫秒数经 JSON 传输，必须能被 JavaScript number 精确表示。
const MAX_SAFE_TIMESTAMP: u64 = 9_007_199_254_740_991;

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum MergeError {
    #[error("清单请求 ID 不能为空")]
    EmptyRequestId,
    #[error("清单时间超出 IPC 可精确表示的范围")]
    InvalidTimestamp,
    #[error("同一清单不能包含重复来源：{0:?}")]
    DuplicateProvider(ProviderId),
    #[error("观测来源与扫描来源不一致")]
    ProviderMismatch,
    #[error("来源包与工具身份不一致")]
    ToolMismatch,
    #[error("观测字段无效：{0}")]
    InvalidObservation(&'static str),
    #[error("PATH 发现不能声明安装所有权")]
    PathCannotOwnInstallation,
}

fn provider_key(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Mise => "mise",
        ProviderId::Brew => "brew",
        ProviderId::Nvm => "nvm",
        ProviderId::Pyenv => "pyenv",
        ProviderId::Uv => "uv",
        ProviderId::Path => "path",
    }
}

fn kind_key(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Runtime => "runtime",
        PackageKind::Formula => "formula",
        PackageKind::Cask => "cask",
        PackageKind::Executable => "executable",
    }
}

fn platform_key(platform: Platform) -> &'static str {
    match platform {
        Platform::Macos => "macos",
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    }
}

/// 长度使用 UTF-8 字节数，各字段保留边界与 null，避免分隔符和空值产生身份碰撞。
/// 不做路径大小写转换，也不把相同版本的不同目录合并；目标或版本变化会改变身份。
pub fn installation_id(observation: &InstallationObservation) -> InstallationId {
    let tool = canonical_tool_id(&observation.tool_id.0);
    let package = &observation.provider_package;
    let parts = [
        Some(tool.0.as_str()),
        Some(provider_key(package.provider_id)),
        Some(package.package_id.as_str()),
        Some(kind_key(package.kind)),
        Some(platform_key(package.platform)),
        Some(observation.locator.as_str()),
        observation.executable_path.as_deref(),
        observation.canonical_executable_path.as_deref(),
        observation.version.as_deref(),
        observation.architecture.as_deref(),
    ];
    let mut id = String::from("installation:v1:");
    for part in parts {
        match part {
            Some(value) => id.push_str(&format!("{}:{value};", value.len())),
            None => id.push_str("-;"),
        }
    }
    InstallationId(id)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains('\0')
}

fn validate_observation(
    provider: ProviderId,
    observation: &InstallationObservation,
) -> Result<(), MergeError> {
    if observation.provider_package.provider_id != provider {
        return Err(MergeError::ProviderMismatch);
    }
    for (field, value) in [
        ("toolId", observation.tool_id.0.as_str()),
        (
            "packageId",
            observation.provider_package.package_id.as_str(),
        ),
        ("locator", observation.locator.as_str()),
    ] {
        if !valid_text(value) {
            return Err(MergeError::InvalidObservation(field));
        }
    }
    for (field, value) in [
        ("executablePath", &observation.executable_path),
        (
            "canonicalExecutablePath",
            &observation.canonical_executable_path,
        ),
        ("version", &observation.version),
        ("architecture", &observation.architecture),
    ] {
        if value.as_ref().is_some_and(|text| !valid_text(text)) {
            return Err(MergeError::InvalidObservation(field));
        }
    }
    if tool_id_for_package(&observation.provider_package)
        .is_some_and(|expected| expected != observation.tool_id)
    {
        return Err(MergeError::ToolMismatch);
    }
    if provider == ProviderId::Path && observation.ownership == Ownership::Owned {
        return Err(MergeError::PathCannotOwnInstallation);
    }
    Ok(())
}

/// 完整且新鲜的成功覆盖仅代表可判断该来源；仍需结合本次实例才能判断是否未发现。
pub fn has_complete_coverage(scan: &crate::domain::ProviderReport, tool: &ToolId) -> bool {
    scan.status == ProviderStatus::Ok
        && scan.error.is_none()
        && !scan.stale
        && scan.coverage.complete
        && !scan.coverage.truncated
        && scan
            .coverage
            .tool_ids
            .iter()
            .any(|covered| canonical_tool_id(&covered.0) == canonical_tool_id(&tool.0))
}

fn merge_duplicate(existing: &mut Installation, incoming: Installation) {
    // 重复证据只去重；互相矛盾的健康或归属不能升级为更强保证。
    existing.health = match (existing.health, incoming.health) {
        (Health::Broken, _) | (_, Health::Broken) => Health::Broken,
        (Health::Healthy, Health::Healthy) => Health::Healthy,
        _ => Health::Unverified,
    };
    if existing.ownership != incoming.ownership {
        existing.ownership = Ownership::Unknown;
    }
    existing.stale |= incoming.stale;
    existing.evidence.extend(incoming.evidence);
    existing.evidence.sort();
    existing.evidence.dedup();
}

fn aliases(installations: &[Installation]) -> Vec<AliasEvidence> {
    let mut groups: BTreeMap<(ToolId, Platform, String), Vec<&Installation>> = BTreeMap::new();
    for installation in installations {
        // 过期或损坏的路径不能证明当下仍为同一实体，留待重新扫描。
        if installation.stale || installation.health == Health::Broken {
            continue;
        }
        if let Some(path) = &installation.canonical_executable_path {
            groups
                .entry((
                    installation.tool_id.clone(),
                    installation.provider_package.platform,
                    path.clone(),
                ))
                .or_default()
                .push(installation);
        }
    }
    let mut aliases = Vec::new();
    for ((_, _, path), mut group) in groups {
        if group.len() < 2 {
            continue;
        }
        let versions: BTreeSet<_> = group
            .iter()
            .filter_map(|item| item.version.clone())
            .collect();
        let architectures: BTreeSet<_> = group
            .iter()
            .filter_map(|item| item.architecture.clone())
            .collect();
        // 同一路径存在矛盾观测时不选边；尤其不能借未知版本把两个冲突实例串在一起。
        if versions.len() > 1 || architectures.len() > 1 {
            continue;
        }
        group.sort_by(|a, b| {
            (a.ownership != Ownership::Owned, &a.installation_id)
                .cmp(&(b.ownership != Ownership::Owned, &b.installation_id))
        });
        let target = group[0];
        for source in group.into_iter().skip(1) {
            aliases.push(AliasEvidence {
                installation_id: source.installation_id.clone(),
                target_installation_id: target.installation_id.clone(),
                evidence: CanonicalExecutableEvidence {
                    kind: AliasEvidenceKind::CanonicalExecutable,
                    canonical_executable_path: path.clone(),
                    version: versions.first().cloned(),
                    architecture: architectures.first().cloned(),
                },
            });
        }
    }
    aliases.sort();
    aliases
}

/// 稳定合并来源结果，不判断项目满足、不选择激活版本，也不因失败来源移除其它结果。
pub fn merge_inventory(
    request_id: String,
    observed_at: u64,
    scans: Vec<ProviderScan>,
) -> Result<InventorySnapshot, MergeError> {
    if !valid_text(&request_id) {
        return Err(MergeError::EmptyRequestId);
    }
    if observed_at > MAX_SAFE_TIMESTAMP {
        return Err(MergeError::InvalidTimestamp);
    }
    let mut providers = BTreeMap::new();
    let mut installations: BTreeMap<InstallationId, Installation> = BTreeMap::new();
    let mut tools = BTreeSet::new();
    for scan in scans {
        let mut report = scan.report;
        if report.observed_at > MAX_SAFE_TIMESTAMP {
            return Err(MergeError::InvalidTimestamp);
        }
        let provider = report.provider_id;
        if providers.contains_key(&provider) {
            return Err(MergeError::DuplicateProvider(provider));
        }
        for tool in &mut report.coverage.tool_ids {
            *tool = canonical_tool_id(&tool.0);
            if !valid_text(&tool.0) {
                return Err(MergeError::InvalidObservation("coverage.toolIds"));
            }
            tools.insert(tool.clone());
        }
        report.coverage.tool_ids.sort();
        report.coverage.tool_ids.dedup();
        for mut observation in scan.observations {
            observation.tool_id = canonical_tool_id(&observation.tool_id.0);
            validate_observation(provider, &observation)?;
            tools.insert(observation.tool_id.clone());
            observation.evidence.sort();
            observation.evidence.dedup();
            let id = installation_id(&observation);
            let installation =
                Installation::from_observation(id.clone(), observation, report.stale);
            if let Some(existing) = installations.get_mut(&id) {
                merge_duplicate(existing, installation);
            } else {
                installations.insert(id, installation);
            }
        }
        providers.insert(provider, report);
    }
    let mut installations: Vec<_> = installations.into_values().collect();
    installations.sort_by(|a, b| {
        (
            &a.tool_id,
            &a.provider_package,
            &a.locator,
            &a.installation_id,
        )
            .cmp(&(
                &b.tool_id,
                &b.provider_package,
                &b.locator,
                &b.installation_id,
            ))
    });
    let tools = tools
        .into_iter()
        .map(|tool_id| {
            if tool_id == ToolId::from("node") {
                node_tool_definition()
            } else if tool_id == ToolId::from("python") {
                python_tool_definition()
            } else {
                ToolDefinition {
                    display_name: tool_id.0.clone(),
                    tool_id,
                    aliases: Vec::new(),
                    executable_names: Vec::new(),
                }
            }
        })
        .collect();
    Ok(InventorySnapshot {
        schema_version: INVENTORY_SCHEMA_VERSION,
        request_id,
        observed_at,
        tools,
        providers: providers.into_values().collect(),
        aliases: aliases(&installations),
        installations,
    })
}

#[cfg(test)]
mod tests;
