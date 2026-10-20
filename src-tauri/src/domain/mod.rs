//! 本机工具清单的纯数据契约；本模块不执行探测、路径解析或文件写入。

use std::fmt;

use serde::{Deserialize, Serialize};

pub const INVENTORY_SCHEMA_VERSION: u32 = 1;

/// 工具身份和安装身份分开，避免把同版本的不同安装视为同一实例。
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ToolId(pub String);

impl From<&str> for ToolId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for ToolId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct InstallationId(pub String);

impl From<&str> for InstallationId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for InstallationId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for InstallationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderId {
    Mise,
    Brew,
    Nvm,
    Pyenv,
    Uv,
    Path,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageKind {
    Runtime,
    Formula,
    Cask,
    Executable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Health {
    Healthy,
    Broken,
    Unverified,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Ownership {
    Owned,
    Observed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderStatus {
    Ok,
    Unavailable,
    Unsupported,
    PermissionDenied,
    Timeout,
    Cancelled,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub tool_id: ToolId,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub executable_names: Vec<String>,
}

/// 原生包身份原样保留；归一化工具名不会改写 node@22 等渠道身份。
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPackage {
    pub provider_id: ProviderId,
    pub package_id: String,
    pub kind: PackageKind,
    pub platform: Platform,
}

/// 证据种类保留来源提供的标识，允许后续适配器补充而不丢失原始线索。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationEvidence {
    pub kind: String,
    pub path: Option<String>,
    pub detail: Option<String>,
}

/// 未知版本、架构、路径均保留为 null；健康和归属不能由版本字符串推断。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationObservation {
    pub provider_package: ProviderPackage,
    pub tool_id: ToolId,
    pub locator: String,
    pub executable_path: Option<String>,
    pub canonical_executable_path: Option<String>,
    pub version: Option<String>,
    pub architecture: Option<String>,
    pub health: Health,
    pub ownership: Ownership,
    pub evidence: Vec<ObservationEvidence>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub installation_id: InstallationId,
    pub provider_package: ProviderPackage,
    pub tool_id: ToolId,
    pub locator: String,
    pub executable_path: Option<String>,
    pub canonical_executable_path: Option<String>,
    pub version: Option<String>,
    pub architecture: Option<String>,
    pub health: Health,
    pub ownership: Ownership,
    pub evidence: Vec<ObservationEvidence>,
    pub stale: bool,
}

impl Installation {
    pub fn from_observation(
        installation_id: InstallationId,
        observation: InstallationObservation,
        stale: bool,
    ) -> Self {
        Self {
            installation_id,
            provider_package: observation.provider_package,
            tool_id: observation.tool_id,
            locator: observation.locator,
            executable_path: observation.executable_path,
            canonical_executable_path: observation.canonical_executable_path,
            version: observation.version,
            architecture: observation.architecture,
            health: observation.health,
            ownership: observation.ownership,
            evidence: observation.evidence,
            stale,
        }
    }
}

/// 仅成功、完整且未截断的范围可以支持“未发现”；来源失败本身不表示未安装。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCoverage {
    pub tool_ids: Vec<ToolId>,
    pub complete: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReport {
    pub provider_id: ProviderId,
    pub status: ProviderStatus,
    pub observed_at: u64,
    pub stale: bool,
    pub coverage: ScanCoverage,
    pub error: Option<ProviderError>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderScan {
    pub report: ProviderReport,
    pub observations: Vec<InstallationObservation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AliasEvidenceKind {
    CanonicalExecutable,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalExecutableEvidence {
    pub kind: AliasEvidenceKind,
    pub canonical_executable_path: String,
    pub version: Option<String>,
    pub architecture: Option<String>,
}

/// 目标仅用于稳定展示同实体关联，关联本身不赋予目标来源卸载权。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasEvidence {
    pub installation_id: InstallationId,
    pub target_installation_id: InstallationId,
    pub evidence: CanonicalExecutableEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshot {
    pub schema_version: u32,
    pub request_id: String,
    pub observed_at: u64,
    pub tools: Vec<ToolDefinition>,
    pub providers: Vec<ProviderReport>,
    pub installations: Vec<Installation>,
    pub aliases: Vec<AliasEvidence>,
}

/// 只折叠已登记的 Node 别名；未知工具保留原名，避免猜测其它工具的身份。
pub fn canonical_tool_id(raw: &str) -> ToolId {
    let trimmed = raw.trim();
    if ["node", "nodejs", "node.js"]
        .iter()
        .any(|alias| trimmed.eq_ignore_ascii_case(alias))
    {
        ToolId::from("node")
    } else {
        ToolId::from(trimmed)
    }
}

pub fn node_tool_definition() -> ToolDefinition {
    ToolDefinition {
        tool_id: ToolId::from("node"),
        display_name: "Node.js".into(),
        aliases: ["node", "nodejs", "node.js"]
            .into_iter()
            .map(String::from)
            .collect(),
        executable_names: ["node", "nodejs", "node.exe", "nodejs.exe"]
            .into_iter()
            .map(String::from)
            .collect(),
    }
}

pub fn python_tool_definition() -> ToolDefinition {
    ToolDefinition {
        tool_id: ToolId::from("python"),
        display_name: "Python".into(),
        aliases: vec![],
        executable_names: ["python", "python3"]
            .into_iter()
            .map(String::from)
            .collect(),
    }
}
pub fn native_brew_python_formula(package_id: &str) -> Option<String> {
    let formula = package_id
        .strip_prefix("homebrew/core/")
        .unwrap_or(package_id);
    if formula == "python" || formula == "python3" {
        return Some(formula.into());
    }
    let minor = formula.strip_prefix("python@3.")?;
    if minor.is_empty()
        || minor.len() > 2
        || minor.starts_with('0')
        || !minor.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some(formula.into())
}

/// 只接受官方 core 名称及明确的主版本 formula，不将第三方 tap 当成官方包。
pub fn native_brew_node_formula(package_id: &str) -> Option<String> {
    let formula = package_id
        .strip_prefix("homebrew/core/")
        .unwrap_or(package_id);
    if formula == "node" {
        return Some(formula.to_owned());
    }
    let major = formula.strip_prefix("node@")?;
    if major.is_empty()
        || major.starts_with('0')
        || !major.bytes().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    Some(formula.to_owned())
}

/// 包到工具的映射同时检查来源和包种类，保留原生包标识及平台信息。
pub fn tool_id_for_package(package: &ProviderPackage) -> Option<ToolId> {
    let is_node = match (package.provider_id, package.kind) {
        (ProviderId::Brew, PackageKind::Formula) => {
            native_brew_node_formula(&package.package_id).is_some()
        }
        (ProviderId::Mise | ProviderId::Nvm, PackageKind::Runtime) => {
            ["node", "nodejs"].contains(&package.package_id.as_str())
        }
        (ProviderId::Path, PackageKind::Executable) => match package.platform {
            Platform::Windows => ["node", "nodejs", "node.exe", "nodejs.exe"]
                .iter()
                .any(|name| package.package_id.eq_ignore_ascii_case(name)),
            Platform::Macos | Platform::Linux => {
                ["node", "nodejs"].contains(&package.package_id.as_str())
            }
        },
        _ => false,
    };
    if is_node {
        return Some(ToolId::from("node"));
    }
    let is_python = match (package.provider_id, package.kind) {
        (ProviderId::Brew, PackageKind::Formula) => {
            native_brew_python_formula(&package.package_id).is_some()
        }
        (ProviderId::Mise | ProviderId::Pyenv | ProviderId::Uv, PackageKind::Runtime) => {
            package.package_id == "python"
        }
        (ProviderId::Path, PackageKind::Executable) => {
            package.package_id == "python"
                || package.package_id == "python3"
                || package
                    .package_id
                    .strip_prefix("python3.")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        }
        _ => false,
    };
    is_python.then(|| ToolId::from("python"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package(provider_id: ProviderId, kind: PackageKind, package_id: &str) -> ProviderPackage {
        ProviderPackage {
            provider_id,
            package_id: package_id.to_owned(),
            kind,
            platform: Platform::Macos,
        }
    }

    fn unknown_observation() -> InstallationObservation {
        InstallationObservation {
            provider_package: package(ProviderId::Nvm, PackageKind::Runtime, "node"),
            tool_id: ToolId::from("node"),
            locator: "/home/user/.nvm/versions/node/v22".into(),
            executable_path: Some("/home/user/.nvm/versions/node/v22/bin/node".into()),
            canonical_executable_path: None,
            version: None,
            architecture: None,
            health: Health::Broken,
            ownership: Ownership::Unknown,
            evidence: vec![ObservationEvidence {
                kind: "path".into(),
                path: None,
                detail: Some("符号链接断开".into()),
            }],
        }
    }

    #[test]
    fn node_aliases_are_explicit_and_unknown_names_are_preserved() {
        for raw in ["node", "nodejs", "Node.js", " NODEJS "] {
            assert_eq!(canonical_tool_id(raw), ToolId::from("node"));
        }
        for raw in ["node@22", "node-tool", "my/Node", "Python"] {
            assert_eq!(canonical_tool_id(raw), ToolId::from(raw));
        }
        assert_eq!(canonical_tool_id(" Python "), ToolId::from("Python"));
        let definition = node_tool_definition();
        assert_eq!(definition.tool_id, ToolId::from("node"));
        assert!(definition.executable_names.contains(&"node.exe".to_owned()));
    }

    #[test]
    fn brew_core_formula_mapping_preserves_channels_and_rejects_third_party_taps() {
        for (native, expected) in [
            ("node", "node"),
            ("node@22", "node@22"),
            ("homebrew/core/node", "node"),
            ("homebrew/core/node@22", "node@22"),
        ] {
            assert_eq!(native_brew_node_formula(native), Some(expected.into()));
            let native_package = package(ProviderId::Brew, PackageKind::Formula, native);
            assert_eq!(
                tool_id_for_package(&native_package),
                Some(ToolId::from("node"))
            );
            assert_eq!(native_package.package_id, native);
        }
        for native in [
            "other/tap/node",
            "other/tap/node@22",
            "homebrew/cask/node",
            "nodejs",
            "node@",
            "node@0",
            "node@022",
            "node@22.1",
            "node@22/bin/node",
            " node ",
        ] {
            assert_eq!(native_brew_node_formula(native), None, "{native}");
        }
        assert_ne!(
            package(ProviderId::Brew, PackageKind::Formula, "node"),
            package(ProviderId::Brew, PackageKind::Formula, "node@22")
        );
    }

    #[test]
    fn package_mapping_checks_provider_kind_and_platform() {
        assert_eq!(
            tool_id_for_package(&package(ProviderId::Mise, PackageKind::Runtime, "nodejs")),
            Some(ToolId::from("node"))
        );
        assert_eq!(
            tool_id_for_package(&package(ProviderId::Brew, PackageKind::Cask, "node")),
            None
        );
        assert_eq!(
            tool_id_for_package(&package(ProviderId::Mise, PackageKind::Formula, "node")),
            None
        );
        let mut executable = package(ProviderId::Path, PackageKind::Executable, "NODE.EXE");
        assert_eq!(tool_id_for_package(&executable), None);
        executable.platform = Platform::Windows;
        assert_eq!(tool_id_for_package(&executable), Some(ToolId::from("node")));
    }

    #[test]
    fn observation_serialization_keeps_unknowns_explicit_and_uses_camel_case() {
        let observation = unknown_observation();
        let encoded = serde_json::to_value(&observation).unwrap();
        assert_eq!(encoded["toolId"], "node");
        assert_eq!(encoded["providerPackage"]["providerId"], "nvm");
        for key in ["canonicalExecutablePath", "version", "architecture"] {
            assert_eq!(encoded.get(key), Some(&serde_json::Value::Null), "{key}");
        }
        assert_eq!(encoded["evidence"][0].get("path"), Some(&json!(null)));
        assert_eq!(encoded["ownership"], "unknown");
        assert_eq!(encoded["health"], "broken");
        assert!(encoded.get("tool_id").is_none());
        assert_eq!(
            serde_json::from_value::<InstallationObservation>(encoded).unwrap(),
            observation
        );
    }

    #[test]
    fn failed_provider_is_serialized_as_status_with_explicit_error_slot() {
        let mut report = ProviderReport {
            provider_id: ProviderId::Mise,
            status: ProviderStatus::PermissionDenied,
            observed_at: 1_792_339_200_000,
            stale: true,
            coverage: ScanCoverage {
                tool_ids: vec![ToolId::from("node")],
                complete: false,
                truncated: false,
            },
            error: Some(ProviderError {
                code: "EACCES".into(),
                message: "无权读取安装目录".into(),
                retryable: false,
            }),
        };
        let encoded = serde_json::to_value(&report).unwrap();
        assert_eq!(encoded["status"], "permissionDenied");
        assert_eq!(encoded["observedAt"], 1_792_339_200_000_u64);
        assert_eq!(encoded["coverage"]["complete"], false);
        assert_eq!(
            serde_json::from_value::<ProviderReport>(encoded).unwrap(),
            report
        );
        report.error = None;
        assert_eq!(
            serde_json::to_value(report).unwrap().get("error"),
            Some(&json!(null))
        );
        assert!(serde_json::from_value::<ProviderStatus>(json!("absent")).is_err());
    }

    #[test]
    fn installation_conversion_preserves_health_ownership_and_evidence() {
        let observation = unknown_observation();
        let installation = Installation::from_observation(
            InstallationId::from("stable-installation"),
            observation.clone(),
            true,
        );
        assert_eq!(installation.health, Health::Broken);
        assert_eq!(installation.ownership, Ownership::Unknown);
        assert_eq!(installation.version, None);
        assert_eq!(installation.evidence, observation.evidence);
        assert!(installation.stale);
        let encoded = serde_json::to_value(&installation).unwrap();
        assert_eq!(encoded["installationId"], "stable-installation");
        assert_eq!(
            serde_json::from_value::<Installation>(encoded).unwrap(),
            installation
        );
    }

    #[test]
    fn shared_contract_fixture_round_trips_without_renaming_or_dropping_fields() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ContractFixture {
            request_id: String,
            observed_at: u64,
            scans: Vec<ProviderScan>,
        }

        let raw = include_str!("../../../fixtures/inventory/contract-v1.json");
        let fixture: ContractFixture = serde_json::from_str(raw).unwrap();
        let document: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(fixture.request_id, "contract-node-v1");
        assert_eq!(fixture.observed_at, 1_792_339_200_000);
        assert_eq!(
            serde_json::to_value(&fixture.scans).unwrap(),
            document["scans"]
        );
        assert_eq!(fixture.scans.len(), 4);
        assert!(fixture.scans.iter().any(|scan| {
            scan.report.status == ProviderStatus::Timeout && scan.observations.is_empty()
        }));
        assert!(fixture
            .scans
            .iter()
            .flat_map(|scan| &scan.observations)
            .any(|observation| observation.health == Health::Broken
                && observation.ownership == Ownership::Unknown
                && observation.version.is_none()));
    }
}
#[cfg(test)]
mod python_mapping_tests {
    use super::*;
    #[test]
    fn python_provider_packages_keep_original_channels_and_implementations_separate() {
        for (provider, kind, name) in [
            (ProviderId::Mise, PackageKind::Runtime, "python"),
            (ProviderId::Pyenv, PackageKind::Runtime, "python"),
            (ProviderId::Uv, PackageKind::Runtime, "python"),
            (ProviderId::Brew, PackageKind::Formula, "python@3.13"),
            (ProviderId::Path, PackageKind::Executable, "python3.13"),
        ] {
            let package = ProviderPackage {
                provider_id: provider,
                package_id: name.into(),
                kind,
                platform: Platform::Macos,
            };
            assert_eq!(tool_id_for_package(&package), Some(ToolId::from("python")));
        }
        assert!(native_brew_python_formula("someone/tap/python@3.13").is_none());
        assert!(native_brew_python_formula("python@3.013").is_none());
        let package = ProviderPackage {
            provider_id: ProviderId::Uv,
            package_id: "pypy".into(),
            kind: PackageKind::Runtime,
            platform: Platform::Macos,
        };
        assert_eq!(tool_id_for_package(&package), None);
    }
}
