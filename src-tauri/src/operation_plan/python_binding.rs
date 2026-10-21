//! Python mise 配置绑定：项目/全局 `[tools] python`，带差异、收据、解绑与失败回滚。
use super::*;
use crate::{config, error::AppError, providers::python::PythonScanOptions};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_RECEIPTS: usize = 32;
static LEDGER_LOCK: Mutex<()> = Mutex::new(());
static RECEIPT_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    id: String,
    installation_id: InstallationId,
    provider_package: ProviderPackage,
    version: String,
    locator: String,
    config_path: PathBuf,
    key: String,
    before: Option<String>,
    after: String,
    scope: BindingScope,
    state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u32,
    records: Vec<Receipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Facts {
    config_path: PathBuf,
    key: String,
    before: Option<String>,
    after: Option<String>,
    original: Option<String>,
}

#[derive(Clone)]
pub(super) struct BindingSpec {
    pub action: Action,
    pub installation_id: InstallationId,
    pub provider_package: ProviderPackage,
    pub version: String,
    pub locator: String,
    pub config_path: PathBuf,
    pub diff: ConfigurationDiff,
    pub impact: Vec<String>,
    facts: Facts,
    cwd: PathBuf,
    target_executable: Option<PathBuf>,
    ledger: PathBuf,
    receipt: Receipt,
    target: Option<Installation>,
}

pub(super) fn ledger(options: &PythonScanOptions) -> PathBuf {
    options.common.home.join(".zenv/python-bindings-v1.json")
}

fn denied(message: &'static str) -> PlanError {
    PlanError::Capability(message)
}

fn stable(value: &str) -> bool {
    crate::providers::python::stable(value)
}

fn load(path: &Path) -> Result<Document, PlanError> {
    let Some(text) = config::read_optional(path).map_err(|_| PlanError::Command)? else {
        return Ok(Document {
            schema_version: 1,
            records: vec![],
        });
    };
    let doc: Document = serde_json::from_str(&text).map_err(|_| PlanError::Command)?;
    if doc.schema_version != 1
        || doc.records.len() > MAX_RECEIPTS
        || doc.records.iter().any(|record| {
            !matches!(record.state.as_str(), "pending" | "bound")
                || !record.config_path.is_absolute()
                || !matches!(record.key.as_str(), "python" | "core:python")
                || !matches!(record.scope, BindingScope::Project | BindingScope::Global)
                || !stable(&record.after)
                || record
                    .before
                    .as_ref()
                    .is_some_and(|value| value.len() > 256)
                || record.id.len() > 128
        })
    {
        return Err(PlanError::Command);
    }
    let ids: std::collections::BTreeSet<_> = doc.records.iter().map(|record| &record.id).collect();
    if ids.len() != doc.records.len() {
        return Err(PlanError::Command);
    }
    Ok(doc)
}

fn save(path: &Path, doc: &Document) -> Result<(), PlanError> {
    if doc.records.len() > MAX_RECEIPTS {
        return Err(PlanError::Capability("Python 绑定收据超出预算"));
    }
    let bytes = serde_json::to_vec_pretty(doc).map_err(|_| PlanError::Command)?;
    if bytes.len() > 1024 * 1024 {
        return Err(PlanError::Capability("Python 绑定收据超出预算"));
    }
    config::atomic_write(path, &bytes).map_err(|_| PlanError::Command)
}

fn remove_receipt(ledger: &Path, id: &str) -> Result<(), PlanError> {
    let Some(text) = config::read_optional(ledger).map_err(|_| PlanError::Command)? else {
        return Ok(());
    };
    let mut doc: Document = serde_json::from_str(&text).map_err(|_| PlanError::Command)?;
    doc.records.retain(|record| record.id != id);
    save(ledger, &doc)
}

fn receipt_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = RECEIPT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("python-binding-{nanos:x}-{sequence:x}")
}

fn config_target(binding: &BindingOptions, global: &Path) -> Result<PathBuf, PlanError> {
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
        BindingScope::Global => Ok(global.to_path_buf()),
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
    after: Option<&str>,
) -> Result<(String, Option<String>), PlanError> {
    let content = original.unwrap_or("");
    let (current_key, current_value) = tools_value(content)?;
    if current_key != key {
        return Err(PlanError::Preconditions);
    }
    let mut doc = config::parse(content).map_err(|_| PlanError::EvidenceUnavailable)?;
    let tools = config::table(&mut doc, "tools").map_err(|_| PlanError::EvidenceUnavailable)?;
    match after {
        Some(value) => {
            tools.insert(key, toml_edit::value(value));
        }
        None => {
            tools.remove(key);
        }
    }
    Ok((doc.to_string(), current_value))
}

fn map_config_error(error: AppError) -> PlanError {
    match error {
        AppError::Other(message) if message.contains("外部修改") => PlanError::Preconditions,
        _ => PlanError::Command,
    }
}

pub(super) fn prepare_bind(
    target: &Installation,
    snapshot: &InventorySnapshot,
    binding: &BindingOptions,
    global_config: &Path,
    ledger_path: &Path,
    home: &Path,
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
    let config_path = config_target(binding, global_config)?;
    if !config_path.is_absolute() {
        return Err(PlanError::EvidenceUnavailable);
    }
    let original =
        config::read_optional(&config_path).map_err(|_| PlanError::EvidenceUnavailable)?;
    let (key, before) = tools_value(original.as_deref().unwrap_or(""))?;
    if before.as_deref() == Some(version.as_str()) {
        return Err(denied("该配置已经绑定同一精确版本"));
    }
    let (next, current) = next_content(original.as_deref(), &key, Some(&version))?;
    debug_assert_eq!(current, before);
    if next.len() > 1024 * 1024 {
        return Err(denied("配置写入超出预算"));
    }
    let doc = load(ledger_path)?;
    if doc
        .records
        .iter()
        .any(|record| record.config_path == config_path && record.key == key)
    {
        return Err(denied("该配置已有 Python 绑定收据，请先解绑"));
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
    let cwd = if binding.scope == BindingScope::Global {
        home.to_path_buf()
    } else {
        config_path
            .parent()
            .ok_or(PlanError::EvidenceUnavailable)?
            .to_path_buf()
    };
    let target_executable = target
        .canonical_executable_path
        .as_deref()
        .or(target.executable_path.as_deref())
        .and_then(|path| PathBuf::from(path).canonicalize().ok());
    let impact = vec![
        format!("写入{scope} mise 配置：{}", config_path.display()),
        "只修改 [tools] Python 选择，保留其他工具、注释与结构；写入前保留 .zenv-backup".into(),
        "同时写入本应用私有绑定收据，解绑只处理本应用写入的值；不改 shell，不卸载实例".into(),
        "配置写入成功不等于 mise 实际解析、终端或 IDE 生效；需另行核对".into(),
    ];
    let receipt = Receipt {
        id: receipt_id(),
        installation_id: target.installation_id.clone(),
        provider_package: target.provider_package.clone(),
        version: version.clone(),
        locator: target.locator.clone(),
        config_path: config_path.clone(),
        key: key.clone(),
        before: before.clone(),
        after: version.clone(),
        scope: binding.scope,
        state: "pending".into(),
    };
    Ok(BindingSpec {
        action: Action::Bind,
        installation_id: target.installation_id.clone(),
        provider_package: target.provider_package.clone(),
        version,
        locator: target.locator.clone(),
        config_path: config_path.clone(),
        diff,
        impact,
        facts: Facts {
            config_path,
            key,
            before,
            after: Some(receipt.after.clone()),
            original,
        },
        ledger: ledger_path.to_path_buf(),
        receipt,
        target: Some(target.clone()),
        cwd,
        target_executable,
    })
}

pub(super) fn prepare_unbind(
    binding_id: &str,
    ledger_path: &Path,
) -> Result<BindingSpec, PlanError> {
    let doc = load(ledger_path)?;
    let receipt = doc
        .records
        .iter()
        .find(|record| record.id == binding_id && record.state == "bound")
        .cloned()
        .ok_or(denied("没有找到可解除的 Python 绑定"))?;
    if !receipt.config_path.is_absolute() {
        return Err(PlanError::EvidenceUnavailable);
    }
    let original =
        config::read_optional(&receipt.config_path).map_err(|_| PlanError::EvidenceUnavailable)?;
    let (key, value) = tools_value(original.as_deref().unwrap_or(""))?;
    if key != receipt.key || value.as_deref() != Some(receipt.after.as_str()) {
        return Err(PlanError::BindingConflict);
    }
    let (next, _) = next_content(original.as_deref(), &receipt.key, receipt.before.as_deref())?;
    if next.len() > 1024 * 1024 {
        return Err(denied("配置写入超出预算"));
    }
    let diff = ConfigurationDiff {
        path: receipt.config_path.to_string_lossy().into(),
        field: format!("tools.{}", receipt.key),
        before: Some(receipt.after.clone()),
        after: receipt.before.clone(),
    };
    let impact = vec![
        format!(
            "恢复绑定前的 Python 选择：{}",
            receipt.config_path.display()
        ),
        "只移除或恢复本应用收据记录的原值；外部修改时停止并保留收据".into(),
        "不改 shell、不卸载 Python，也不改写项目虚拟环境".into(),
    ];
    Ok(BindingSpec {
        action: Action::Unbind,
        installation_id: receipt.installation_id.clone(),
        provider_package: receipt.provider_package.clone(),
        version: receipt.version.clone(),
        locator: receipt.locator.clone(),
        config_path: receipt.config_path.clone(),
        diff,
        impact,
        facts: Facts {
            config_path: receipt.config_path.clone(),
            key: receipt.key.clone(),
            before: Some(receipt.after.clone()),
            after: receipt.before.clone(),
            original,
        },
        ledger: ledger_path.to_path_buf(),
        receipt,
        target: None,
        cwd: PathBuf::new(),
        target_executable: None,
    })
}

impl BindingSpec {
    pub fn revalidate(
        &self,
        target: Option<&Installation>,
        snapshot: &InventorySnapshot,
    ) -> Result<(), PlanError> {
        if self.action == Action::Bind {
            let target = target.ok_or(PlanError::Preconditions)?;
            if target.version.as_deref() != Some(self.version.as_str())
                || target.locator != self.locator
                || !snapshot.installations.iter().any(|item| item == target)
            {
                return Err(PlanError::Preconditions);
            }
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
        if self.action == Action::Unbind {
            let doc = load(&self.ledger)?;
            if !doc
                .records
                .iter()
                .any(|record| record.id == self.receipt.id && record.state == "bound")
            {
                return Err(PlanError::Preconditions);
            }
        }
        Ok(())
    }
    pub fn apply(&self) -> Result<(), PlanError> {
        let _guard = LEDGER_LOCK.lock().map_err(|_| PlanError::Command)?;
        let current = config::read_optional(&self.facts.config_path)
            .map_err(|_| PlanError::EvidenceUnavailable)?;
        if current != self.facts.original {
            return Err(PlanError::Preconditions);
        }
        let (next, _) = next_content(
            current.as_deref(),
            &self.facts.key,
            self.facts.after.as_deref(),
        )?;
        if self.action == Action::Bind {
            let mut doc = load(&self.ledger)?;
            doc.records.retain(|record| record.id != self.receipt.id);
            let mut receipt = self.receipt.clone();
            receipt.state = "pending".into();
            doc.records.push(receipt);
            save(&self.ledger, &doc)?;
            if let Err(error) = config::replace_checked(
                &self.facts.config_path,
                self.facts.original.as_deref(),
                Some(&next),
            ) {
                let _ = remove_receipt(&self.ledger, &self.receipt.id);
                return Err(map_config_error(error));
            }
            let mut doc = load(&self.ledger)?;
            let Some(receipt) = doc
                .records
                .iter_mut()
                .find(|record| record.id == self.receipt.id)
            else {
                let _ = config::replace_checked(
                    &self.facts.config_path,
                    Some(&next),
                    self.facts.original.as_deref(),
                );
                return Err(PlanError::Command);
            };
            receipt.state = "bound".into();
            if save(&self.ledger, &doc).is_err() {
                let _ = config::replace_checked(
                    &self.facts.config_path,
                    Some(&next),
                    self.facts.original.as_deref(),
                );
                let _ = remove_receipt(&self.ledger, &self.receipt.id);
                return Err(PlanError::Command);
            }
            return Ok(());
        }
        config::replace_checked(
            &self.facts.config_path,
            self.facts.original.as_deref(),
            Some(&next),
        )
        .map_err(map_config_error)?;
        remove_receipt(&self.ledger, &self.receipt.id)
    }
    pub fn verify(&self, snapshot: &InventorySnapshot) -> Result<(), PlanError> {
        let current =
            config::read_optional(&self.facts.config_path).map_err(|_| PlanError::Verification)?;
        let (key, value) =
            tools_value(current.as_deref().unwrap_or("")).map_err(|_| PlanError::Verification)?;
        if self.facts.after.is_some() {
            if key != self.facts.key || value != self.facts.after {
                return Err(PlanError::Verification);
            }
        } else if value.is_some() {
            return Err(PlanError::Verification);
        }
        let doc = load(&self.ledger).map_err(|_| PlanError::Verification)?;
        let receipt = doc
            .records
            .iter()
            .find(|record| record.id == self.receipt.id);
        match self.action {
            Action::Bind => {
                if receipt.is_none_or(|record| record.state != "bound") {
                    return Err(PlanError::Verification);
                }
                let target = self.target.as_ref().ok_or(PlanError::Verification)?;
                snapshot
                    .installations
                    .iter()
                    .find(|item| {
                        item.provider_package.provider_id == ProviderId::Mise
                            && item.tool_id == ToolId::from("python")
                            && item.installation_id == target.installation_id
                            && item.version.as_deref() == Some(self.version.as_str())
                            && item.locator == self.locator
                            && !item.stale
                            && item.health == Health::Healthy
                    })
                    .map(|_| ())
                    .ok_or(PlanError::Verification)
            }
            Action::Unbind if receipt.is_some() => Err(PlanError::Verification),
            _ => Ok(()),
        }
    }
    /// 配置写入后执行真实 mise 解析；终端/IDE 是否继承仍需另行确认。
    pub fn verify_resolution(&self, options: &PythonScanOptions) -> Result<(), PlanError> {
        if self.action != Action::Bind {
            return Ok(());
        }
        let expected = self
            .target_executable
            .as_ref()
            .ok_or(PlanError::Verification)?;
        let program = guards::executable(&options.common, "mise").ok_or(PlanError::Verification)?;
        let mut cmd = writes::mise_resolution_command(&program, &options.common);
        cmd.current_dir(&self.cwd).args(["which", "python"]);
        let result = crate::process::probe::run_probe(
            &mut cmd,
            &crate::process::probe::ProbeLimits {
                timeout: Duration::from_secs(30),
                max_output_bytes: 64 * 1024,
            },
            &CancellationToken::default(),
        )
        .map_err(|_| PlanError::Verification)?;
        if !result.status.success() {
            return Err(PlanError::Verification);
        }
        let resolved = String::from_utf8(result.stdout).map_err(|_| PlanError::Verification)?;
        let resolved = PathBuf::from(resolved.trim());
        if !resolved.is_absolute() || resolved.canonicalize().ok() != Some(expected.clone()) {
            return Err(PlanError::Verification);
        }
        Ok(())
    }

    pub fn rollback(&self) -> Result<(), PlanError> {
        let _guard = LEDGER_LOCK.lock().map_err(|_| PlanError::Command)?;
        let current = config::read_optional(&self.facts.config_path)
            .map_err(|_| PlanError::EvidenceUnavailable)?;
        let (key, value) = tools_value(current.as_deref().unwrap_or(""))?;
        match self.action {
            Action::Bind => {
                if key != self.facts.key {
                    return Err(PlanError::BindingConflict);
                }
                if value == self.facts.after {
                    config::replace_checked(
                        &self.facts.config_path,
                        current.as_deref(),
                        self.facts.original.as_deref(),
                    )
                    .map_err(map_config_error)?;
                    let _ = remove_receipt(&self.ledger, &self.receipt.id);
                    return Ok(());
                }
                if current == self.facts.original {
                    let _ = remove_receipt(&self.ledger, &self.receipt.id);
                    return Ok(());
                }
                Err(PlanError::BindingConflict)
            }
            Action::Unbind => {
                if value == self.facts.after {
                    let (next, _) = next_content(
                        current.as_deref(),
                        &self.facts.key,
                        self.facts.before.as_deref(),
                    )?;
                    config::replace_checked(
                        &self.facts.config_path,
                        current.as_deref(),
                        Some(&next),
                    )
                    .map_err(map_config_error)?;
                    let mut doc = load(&self.ledger)?;
                    doc.records.retain(|record| record.id != self.receipt.id);
                    let mut receipt = self.receipt.clone();
                    receipt.state = "bound".into();
                    doc.records.push(receipt);
                    save(&self.ledger, &doc)?;
                    return Ok(());
                }
                if value == self.facts.before {
                    return Ok(());
                }
                Err(PlanError::BindingConflict)
            }
            _ => Err(PlanError::Unsupported),
        }
    }
}

pub(super) fn list(ledger_path: &Path) -> Result<Vec<PythonBinding>, PlanError> {
    let doc = load(ledger_path)?;
    Ok(doc
        .records
        .iter()
        .filter(|record| record.state == "bound")
        .map(|record| PythonBinding {
            binding_id: record.id.clone(),
            installation_id: record.installation_id.clone(),
            version: record.version.clone(),
            locator: record.locator.clone(),
            config_path: record.config_path.to_string_lossy().into(),
            scope: record.scope,
            state: record.state.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Health, InstallationId, Ownership, PackageKind, Platform, ProviderPackage, ProviderReport,
        ScanCoverage,
    };
    use std::fs;

    fn ledger() -> PathBuf {
        tempfile::tempdir().unwrap().keep().join("bindings.json")
    }

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
            executable_path: Some(
                "/fixture/.local/share/mise/installs/python/3.13.15/bin/python3".into(),
            ),
            canonical_executable_path: Some(
                "/fixture/.local/share/mise/installs/python/3.13.15/bin/python3".into(),
            ),
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
    fn project_binding_writes_receipt_and_unbind_restores_previous_value() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("mise.toml"),
            "[tools]\nnode = \"22\"\npython = \"3.12\"\n\n[env]\nA = \"1\"\n",
        )
        .unwrap();
        let target = target();
        let ledger = ledger();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
        )
        .unwrap();
        assert_eq!(spec.diff.before.as_deref(), Some("3.12"));
        spec.apply().unwrap();
        let text = fs::read_to_string(directory.path().join("mise.toml")).unwrap();
        assert!(text.contains("python = \"3.13.15\""));
        assert!(text.contains("node = \"22\""));
        assert!(text.contains("A = \"1\""));
        let doc = load(&ledger).unwrap();
        assert_eq!(doc.records.len(), 1);
        assert_eq!(doc.records[0].state, "bound");
        spec.verify(&snapshot(&target)).unwrap();
        assert_eq!(list(&ledger).unwrap().len(), 1);

        let unbind = prepare_unbind(&spec.receipt.id, &ledger).unwrap();
        assert_eq!(unbind.action, Action::Unbind);
        unbind.apply().unwrap();
        let text = fs::read_to_string(directory.path().join("mise.toml")).unwrap();
        assert!(text.contains("python = \"3.12\""));
        assert!(!text.contains("3.13.15"));
        assert!(load(&ledger).unwrap().records.is_empty());
        assert!(list(&ledger).unwrap().is_empty());
        unbind.verify(&snapshot(&target)).unwrap();
    }

    #[test]
    fn global_binding_uses_given_global_config_path() {
        let global = global();
        let target = target();
        let ledger = ledger();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Global, None),
            &global,
            &ledger,
            &std::env::temp_dir(),
        )
        .unwrap();
        spec.apply().unwrap();
        let text = fs::read_to_string(&global).unwrap();
        assert!(text.contains("python = \"3.13.15\""));
        assert_eq!(load(&ledger).unwrap().records.len(), 1);
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
            &ledger(),
            &std::env::temp_dir(),
        )
        .is_err());
        assert!(prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(Path::new("relative"))),
            &global(),
            &ledger(),
            &std::env::temp_dir(),
        )
        .is_err());
        let mut uv = target.clone();
        uv.provider_package.provider_id = ProviderId::Uv;
        assert!(prepare_bind(
            &uv,
            &snapshot(&uv),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger(),
            &std::env::temp_dir(),
        )
        .is_err());
    }

    #[test]
    fn external_config_change_stops_apply_and_unbind() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mise.toml");
        fs::write(&path, "[tools]\nnode = \"22\"\n").unwrap();
        let target = target();
        let ledger = ledger();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
        )
        .unwrap();
        fs::write(&path, "[tools]\nnode = \"20\"\n").unwrap();
        assert!(matches!(spec.apply(), Err(PlanError::Preconditions)));
        assert!(matches!(
            spec.revalidate(Some(&target), &snapshot(&target)),
            Err(PlanError::Preconditions)
        ));
        assert!(load(&ledger).unwrap().records.is_empty());

        fs::write(&path, "[tools]\nnode = \"22\"\n").unwrap();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
        )
        .unwrap();
        spec.apply().unwrap();
        let unbind = prepare_unbind(&spec.receipt.id, &ledger).unwrap();
        fs::write(&path, "[tools]\npython = \"3.12\"\n").unwrap();
        assert!(matches!(
            prepare_unbind(&spec.receipt.id, &ledger),
            Err(PlanError::BindingConflict)
        ));
        assert!(matches!(unbind.apply(), Err(PlanError::Preconditions)));
        assert_eq!(load(&ledger).unwrap().records.len(), 1);
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
        let ledger = ledger();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
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
            &ledger,
            &std::env::temp_dir(),
        )
        .is_err());
    }

    #[test]
    fn bind_rollback_restores_configuration_and_removes_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mise.toml");
        fs::write(&path, "[tools]\nnode = \"22\"\n").unwrap();
        let target = target();
        let ledger = ledger();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
        )
        .unwrap();
        spec.apply().unwrap();
        let mut broken = snapshot(&target);
        broken.installations.clear();
        assert!(matches!(spec.verify(&broken), Err(PlanError::Verification)));
        spec.rollback().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("python"));
        assert!(text.contains("node = \"22\""));
        assert!(load(&ledger).unwrap().records.is_empty());
    }

    #[test]
    fn unbind_rollback_reapplies_and_restores_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mise.toml");
        fs::write(&path, "[tools]\npython = \"3.12\"\n").unwrap();
        let target = target();
        let ledger = ledger();
        let spec = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
        )
        .unwrap();
        spec.apply().unwrap();
        let unbind = prepare_unbind(&spec.receipt.id, &ledger).unwrap();
        unbind.apply().unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("3.12"));
        unbind.rollback().unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("3.13.15"));
        let doc = load(&ledger).unwrap();
        assert_eq!(doc.records.len(), 1);
        assert_eq!(doc.records[0].state, "bound");
    }

    #[test]
    fn duplicate_receipt_and_missing_unbind_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mise.toml");
        fs::write(&path, "[tools]\nnode = \"22\"\n").unwrap();
        let target = target();
        let ledger = ledger();
        let first = prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
        )
        .unwrap();
        first.apply().unwrap();
        assert!(prepare_bind(
            &target,
            &snapshot(&target),
            &options(BindingScope::Project, Some(directory.path())),
            &global(),
            &ledger,
            &std::env::temp_dir(),
        )
        .is_err());
        assert!(prepare_unbind("missing", &ledger).is_err());
    }
}
