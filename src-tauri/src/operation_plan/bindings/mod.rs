//! 可撤销的 Node 绑定；完整配置备份仅在私有绑定收据，不进入最小 journal。
mod resources;
use super::{guards, model::*, ready, writes, PlanError};
use crate::{
    domain::*,
    process::probe::{run_probe, ProbeLimits},
    providers::NodeScanOptions,
};
pub(super) use resources::Transaction;
use resources::{capture, Resource, State};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    id: String,
    source: Installation,
    options: BindingOptions,
    configuration: Option<PathBuf>,
    state: String,
    resources: Vec<Resource>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u32,
    records: Vec<Receipt>,
}
fn ledger(options: &NodeScanOptions) -> PathBuf {
    options.home.join(".zenv/bindings-v1.json")
}
fn encode(doc: &Document) -> Result<State, PlanError> {
    let text = serde_json::to_string_pretty(doc).map_err(|_| PlanError::EvidenceUnavailable)?;
    if text.len() > 1024 * 1024 || doc.records.len() > 32 {
        return Err(PlanError::Capability(
            "绑定记录或配置备份超出预算，请先处理现有绑定",
        ));
    }
    Ok(State::file(text))
}
fn load(options: &NodeScanOptions) -> Result<(State, Document), PlanError> {
    let state = capture(&ledger(options))?;
    #[cfg(unix)]
    if matches!(state, State::File { .. }) {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(ledger(options))
            .map_err(|_| PlanError::BindingConflict)?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(PlanError::BindingConflict);
        }
    }
    let doc = match &state {
        State::Missing => Document {
            schema_version: 1,
            records: vec![],
        },
        State::File { text } => {
            serde_json::from_str::<Document>(text).map_err(|_| PlanError::BindingConflict)?
        }
        _ => return Err(PlanError::BindingConflict),
    };
    if doc.schema_version != 1
        || doc.records.len() > 32
        || doc
            .records
            .iter()
            .any(|r| r.resources.len() > 3 || !matches!(r.state.as_str(), "pending" | "bound"))
    {
        return Err(PlanError::BindingConflict);
    }
    let ids: std::collections::BTreeSet<_> = doc.records.iter().map(|r| &r.id).collect();
    if ids.len() != doc.records.len() {
        return Err(PlanError::BindingConflict);
    }
    Ok((state, doc))
}
pub fn list(options: &NodeScanOptions) -> Result<Vec<BindingSummary>, PlanError> {
    Ok(load(options)?
        .1
        .records
        .into_iter()
        .map(|r| BindingSummary {
            binding_id: r.id,
            provider_package: r.source.provider_package,
            version: r.source.version.unwrap_or_default(),
            locator: r.source.locator,
            strategy: r.options.strategy,
            scope: r.options.scope,
            configuration_path: r.configuration.map(|p| p.to_string_lossy().into()),
            state: r.state,
        })
        .collect())
}
fn canonical(path: &Path) -> Result<PathBuf, PlanError> {
    path.canonicalize().map_err(|_| PlanError::Preconditions)
}
fn version(source: &Installation) -> Result<&str, PlanError> {
    source
        .version
        .as_deref()
        .filter(|v| super::exact_version(v))
        .ok_or(PlanError::Unsupported)
}
fn configuration_path(
    binding: &BindingOptions,
    options: &NodeScanOptions,
    global: &Path,
) -> Result<Option<PathBuf>, PlanError> {
    match binding.scope {
        BindingScope::Registration
            if binding.strategy != BindingStrategy::ProjectPath
                && binding.project_path.is_none() =>
        {
            Ok(None)
        }
        BindingScope::Global
            if binding.strategy != BindingStrategy::ProjectPath
                && binding.project_path.is_none() =>
        {
            Ok(Some(global.into()))
        }
        BindingScope::Project => {
            let path = Path::new(
                binding
                    .project_path
                    .as_deref()
                    .ok_or(PlanError::Capability("请选择项目目录或 mise.toml 文件"))?,
            );
            if !path.is_absolute() || path.to_string_lossy().len() > 4096 {
                return Err(PlanError::Unsupported);
            }
            let document = crate::project::read(path.to_str().ok_or(PlanError::Unsupported)?)
                .map_err(|_| PlanError::EvidenceUnavailable)?;
            let target = PathBuf::from(document.path);
            // 不把项目选择误当成全局配置；旧版本文件需先在项目编辑器显式迁移。
            if guards::configuration(&target)?.0.canonical_path
                == guards::configuration(global)?.0.canonical_path
                || target.parent() == Some(options.home.as_path())
                || path.file_name().is_some_and(|n| n == ".tool-versions")
            {
                return Err(PlanError::Capability(
                    "此选择不是独立项目配置，或需要先处理旧版本文件",
                ));
            }
            Ok(Some(target))
        }
        _ => Err(PlanError::Unsupported),
    }
}
fn edit_configuration(
    old: &str,
    source: &Installation,
    binding: &BindingOptions,
) -> Result<(String, Option<String>, Option<String>, String), PlanError> {
    let mut doc = crate::config::parse(old).map_err(|_| PlanError::EvidenceUnavailable)?;
    if binding.strategy == BindingStrategy::ProjectPath {
        if doc
            .get("tools")
            .and_then(|v| v.as_table_like())
            .is_some_and(|t| {
                t.iter()
                    .any(|(k, _)| matches!(k, "node" | "nodejs" | "core:node"))
            })
        {
            return Err(PlanError::Capability(
                "项目已有 Node 工具声明，请使用 mise 项目选择或先处理冲突",
            ));
        }
        let bin = Path::new(&source.locator)
            .join("bin")
            .to_string_lossy()
            .into_owned();
        let env =
            crate::config::table(&mut doc, "env").map_err(|_| PlanError::EvidenceUnavailable)?;
        if !env.contains_key("_") {
            let mut t = toml_edit::Table::new();
            t.set_dotted(true);
            env.insert("_", toml_edit::Item::Table(t));
        }
        let directives = env
            .get_mut("_")
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or(PlanError::Unsupported)?;
        let old = directives.get("path").map(ToString::to_string);
        let mut array = toml_edit::Array::new();
        array.push(bin.as_str());
        if let Some(item) = directives.get("path") {
            for entry in item.as_array().ok_or(PlanError::Unsupported)?.iter() {
                let value = entry
                    .as_str()
                    .filter(|s| Path::new(s).is_absolute() && !s.contains(['{', '$', '\n']))
                    .ok_or(PlanError::Capability(
                        "项目 PATH 含动态表达式，当前不能确定生效顺序",
                    ))?;
                if value == bin {
                    return Err(PlanError::Capability(
                        "项目已经含有该路径，不能将既有配置登记为新绑定",
                    ));
                }
                array.push(value);
            }
        }
        directives.insert("path", toml_edit::value(array));
        let next = directives.get("path").map(ToString::to_string);
        return Ok((doc.to_string(), old, next, "env._.path".into()));
    }
    let tools =
        crate::config::table(&mut doc, "tools").map_err(|_| PlanError::EvidenceUnavailable)?;
    if tools
        .iter()
        .any(|(k, _)| matches!(k, "nodejs" | "core:node"))
    {
        return Err(PlanError::Capability(
            "存在其它 Node 后端声明，请先在配置编辑器处理",
        ));
    }
    let before = tools.get("node").map(ToString::to_string);
    if tools.get("node").is_some_and(|i| i.as_str().is_none()) {
        return Err(PlanError::Capability(
            "Node 使用复杂工具配置，请先在配置编辑器处理",
        ));
    }
    tools.insert("node", toml_edit::value(version(source)?));
    let after = tools.get("node").map(ToString::to_string);
    Ok((doc.to_string(), before, after, "tools.node".into()))
}
fn binding_id(
    source: &Installation,
    binding: &BindingOptions,
    path: &Option<PathBuf>,
) -> Result<String, PlanError> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "binding-{:x}",
        Sha256::digest(
            serde_json::to_vec(&(source.installation_id.clone(), binding, path))
                .map_err(|_| PlanError::Unsupported)?
        )
    ))
}
fn source_allowed(source: &Installation, options: &NodeScanOptions) -> Result<(), PlanError> {
    let v = version(source)?;
    let path = Path::new(&source.locator);
    let valid = match source.provider_package.provider_id {
        ProviderId::Nvm => path == options.nvm_dir.join("versions/node").join(format!("v{v}")),
        ProviderId::Mise => path == options.mise_data_dir.join("installs/node").join(v),
        ProviderId::Brew => options
            .brew_cellars
            .iter()
            .any(|c| path.parent() == Some(c.join(&source.provider_package.package_id).as_path())),
        _ => false,
    };
    if !valid || source.provider_package.platform != Platform::Macos {
        return Err(PlanError::Unsupported);
    }
    Ok(())
}
#[derive(Clone)]
pub(super) struct BindingSpec {
    pub action: Action,
    pub source: Installation,
    pub program: Option<PathBuf>,
    pub impact: Vec<String>,
    pub diff: Vec<ConfigurationDiff>,
    changes: Vec<Resource>,
    final_record: Resource,
    guards: Vec<(PathBuf, State)>,
    receipt: Receipt,
}
impl BindingSpec {
    pub fn revalidate(
        &self,
        snapshot: &InventorySnapshot,
        options: &NodeScanOptions,
    ) -> Result<(), PlanError> {
        for resource in &self.changes {
            resource.check(&resource.before)?;
        }
        for (path, state) in &self.guards {
            if capture(path)? != *state {
                return Err(PlanError::BindingConflict);
            }
        }
        if self.action == Action::Bind {
            if !ready(snapshot, self.source.provider_package.provider_id)
                || !snapshot.installations.iter().any(|s| {
                    s.installation_id == self.source.installation_id
                        && s.health == Health::Healthy
                        && !s.stale
                        && s.ownership == self.source.ownership
                })
            {
                return Err(PlanError::Preconditions);
            }
            if let Some(program) = &self.program {
                check_registration(
                    program,
                    &self.source,
                    options,
                    self.receipt.options.strategy == BindingStrategy::ProjectPath,
                )?;
            }
        }
        Ok(())
    }
    pub fn apply(&self, transaction: &mut Transaction, id: &str) -> Result<(), PlanError> {
        transaction.recovery_path = Some(self.changes[0].path.clone());
        for resource in &self.changes {
            if crate::operations::cancelled(id) {
                return Err(PlanError::Cancelled);
            }
            transaction.apply(resource)?;
        }
        Ok(())
    }
    pub fn verify(
        &self,
        options: &NodeScanOptions,
        transaction: &mut Transaction,
        id: &str,
    ) -> Result<(), PlanError> {
        for resource in &self.changes {
            resource.check(&resource.after)?;
        }
        if self.action == Action::Bind {
            self.verify_context(options, id)?;
        } else {
            // 解除只确认本绑定资源恢复；不执行已删除或不健康的来源文件。
            for resource in &self.receipt.resources {
                resource.check(&resource.before)?;
            }
        }
        if crate::operations::cancelled(id) {
            return Err(PlanError::Cancelled);
        }
        transaction.apply(&self.final_record)
    }
    fn verify_context(&self, options: &NodeScanOptions, id: &str) -> Result<(), PlanError> {
        let source = &self.source;
        let expected = canonical(Path::new(
            source
                .canonical_executable_path
                .as_ref()
                .ok_or(PlanError::Verification)?,
        ))?;
        let args = [
            "-p",
            "JSON.stringify({version:process.versions.node,execPath:process.execPath})",
        ];
        let mut command = if let Some(program) = &self.program {
            let rows = writes::registered_nodes(program, options)?;
            let registration = options
                .mise_data_dir
                .join("installs/node")
                .join(version(source)?);
            if rows
                .iter()
                .filter(|(v, _)| v == version(source).unwrap_or(""))
                .count()
                != 1
                || !rows.iter().any(|(v, p)| {
                    v == version(source).unwrap_or("")
                        && Path::new(p) == registration
                        && canonical(&registration.join("bin/node")).ok() == Some(expected.clone())
                })
            {
                return Err(PlanError::Verification);
            }
            let mut cmd = writes::command(program, ProviderId::Mise, options);
            cmd.args([
                "--no-config",
                "--no-env",
                "--no-hooks",
                "exec",
                &format!("core:node@{}", version(source)?),
                "--",
                "node",
            ]);
            cmd
        } else {
            let mut cmd = writes::command(Path::new("/usr/bin/env"), ProviderId::Mise, options);
            cmd.env(
                "PATH",
                format!(
                    "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                    Path::new(&source.locator).join("bin").display()
                ),
            );
            cmd.arg("node");
            cmd
        };
        command.args(args);
        // exec 默认可能安装缺失工具；受控验证必须显式禁止任何自动安装。
        command
            .env("MISE_AUTO_INSTALL", "0")
            .env("MISE_EXEC_AUTO_INSTALL", "0")
            .env("MISE_NOT_FOUND_AUTO_INSTALL", "0");
        if let Some(path) = &self.receipt.configuration {
            command.current_dir(path.parent().ok_or(PlanError::Verification)?);
        }
        let token = crate::providers::CancellationToken::default();
        let watch = super::CancelWatch::start(id, token.clone());
        let result = run_probe(
            &mut command,
            &ProbeLimits {
                timeout: Duration::from_secs(10),
                max_output_bytes: 64 * 1024,
            },
            &token,
        )
        .map_err(|e| {
            if matches!(e, crate::process::probe::ProbeError::Cancelled) {
                PlanError::Cancelled
            } else {
                PlanError::Verification
            }
        });
        drop(watch);
        let result = result?;
        if !result.status.success() {
            return Err(PlanError::Verification);
        }
        let result: serde_json::Value =
            serde_json::from_slice(&result.stdout).map_err(|_| PlanError::Verification)?;
        if result["version"].as_str() != Some(version(source)?)
            || result["execPath"]
                .as_str()
                .and_then(|p| canonical(Path::new(p)).ok())
                != Some(expected)
        {
            return Err(PlanError::Verification);
        }
        Ok(())
    }
}
fn check_registration(
    program: &Path,
    source: &Installation,
    options: &NodeScanOptions,
    path_only: bool,
) -> Result<(), PlanError> {
    if path_only {
        return Ok(());
    }
    let rows = writes::registered_nodes(program, options)?;
    let v = version(source)?;
    let matching: Vec<_> = rows.iter().filter(|(version, _)| version == v).collect();
    if source.provider_package.provider_id == ProviderId::Mise {
        if matching.len() != 1
            || canonical(Path::new(&matching[0].1))? != canonical(Path::new(&source.locator))?
        {
            return Err(PlanError::Preconditions);
        }
    } else if !matching.is_empty()
        || std::fs::symlink_metadata(options.mise_data_dir.join("installs/node").join(v)).is_ok()
    {
        return Err(PlanError::Capability(
            "mise 同版本已有安装或链接，不覆盖独立安装和其它绑定",
        ));
    }
    Ok(())
}
pub(super) fn prepare_bind(
    source: &Installation,
    binding: &BindingOptions,
    snapshot: &InventorySnapshot,
    options: &NodeScanOptions,
    global: &Path,
) -> Result<BindingSpec, PlanError> {
    if options.platform != Platform::Macos {
        return Err(PlanError::Unsupported);
    }
    source_allowed(source, options)?;
    if !ready(snapshot, source.provider_package.provider_id)
        || source.stale
        || source.health != Health::Healthy
    {
        return Err(PlanError::Capability("来源未完成验证，请先完整刷新"));
    }
    match binding.strategy {
        BindingStrategy::Direct
            if !matches!(
                source.provider_package.provider_id,
                ProviderId::Mise | ProviderId::Nvm
            ) =>
        {
            return Err(PlanError::Capability(
                "直连只用于原生 mise 或 nvm 精确实例；Homebrew 请选择托管或项目 PATH",
            ))
        }
        BindingStrategy::Managed
            if source.provider_package.provider_id != ProviderId::Brew
                || source.ownership != Ownership::Owned =>
        {
            return Err(PlanError::Capability(
                "托管需要已确认 Homebrew core 归属的实例",
            ))
        }
        _ => {}
    }
    if std::fs::symlink_metadata(&source.locator)
        .map_err(|_| PlanError::EvidenceUnavailable)?
        .file_type()
        .is_symlink()
    {
        return Err(PlanError::Capability(
            "请选择精确的原始安装目录，不能将版本别名再次登记为绑定",
        ));
    }
    if canonical(Path::new(&source.locator))?
        != canonical(Path::new(
            source
                .canonical_executable_path
                .as_ref()
                .ok_or(PlanError::Verification)?,
        ))?
        .parent()
        .and_then(Path::parent)
        .ok_or(PlanError::Verification)?
    {
        return Err(PlanError::Capability(
            "运行时文件不在原生 bin 布局内，当前不能确认绑定边界",
        ));
    }
    let (ledger_before, mut doc) = load(options)?;
    let config = configuration_path(binding, options, global)?;
    let v = version(source)?;
    if doc.records.iter().any(|r| {
        r.source.version.as_deref() == Some(v)
            && r.options.strategy != BindingStrategy::ProjectPath
            && binding.strategy != BindingStrategy::ProjectPath
            || config.is_some() && r.configuration == config
    }) {
        return Err(PlanError::Capability(
            "此版本注册或配置文件已有绑定，请先解除现有绑定",
        ));
    }
    let program = if binding.strategy == BindingStrategy::ProjectPath {
        None
    } else {
        Some(
            guards::executable(options, "mise").ok_or(PlanError::Capability(
                "未找到 mise；可使用独立项目 PATH 绑定",
            ))?,
        )
    };
    if let Some(program) = &program {
        check_registration(program, source, options, false)?;
    }
    let mut resources = vec![];
    let mut diff = vec![];
    if binding.strategy != BindingStrategy::ProjectPath
        && source.provider_package.provider_id != ProviderId::Mise
    {
        let mut destination = canonical(Path::new(&source.locator))?;
        if binding.strategy == BindingStrategy::Managed {
            let farm = options.home.join(".zenv/managed/node").join(v);
            let resource = Resource::new(
                farm.clone(),
                State::Link {
                    target: destination,
                },
            )?;
            if resource.before != State::Missing {
                return Err(PlanError::BindingConflict);
            }
            resources.push(resource);
            destination = farm;
        }
        let registration = Resource::new(
            options.mise_data_dir.join("installs/node").join(v),
            State::Link {
                target: destination,
            },
        )?;
        if registration.before != State::Missing {
            return Err(PlanError::BindingConflict);
        }
        resources.push(registration);
    }
    if let Some(path) = &config {
        let original = Resource::configuration(path.clone(), State::Missing)?;
        let old = original.before.text().unwrap_or("");
        let (next, before, after, field) = edit_configuration(old, source, binding)?;
        diff.push(ConfigurationDiff {
            path: path.to_string_lossy().into(),
            field,
            before,
            after,
        });
        resources.push(Resource {
            after: State::file(next),
            ..original
        });
    }
    let receipt = Receipt {
        id: binding_id(source, binding, &config)?,
        source: source.clone(),
        options: binding.clone(),
        configuration: config,
        state: "pending".into(),
        resources,
    };
    let manifest = options.home.join(".zenv/managed/manifest.json");
    let manifest_state = capture(&manifest)?;
    if let State::File { text } = &manifest_state {
        let entries: Vec<crate::managed::ManagedEntry> =
            serde_json::from_str(text).map_err(|_| PlanError::BindingConflict)?;
        if entries.iter().any(|e| e.tool == "node" && e.version == v) {
            return Err(PlanError::Capability(
                "旧托管清单已有同版本接入，请先在旧入口解除并确认",
            ));
        }
    } else if manifest_state != State::Missing {
        return Err(PlanError::BindingConflict);
    }
    doc.records.push(receipt.clone());
    let pending = encode(&doc)?;
    doc.records.last_mut().unwrap().state = "bound".into();
    let completed = encode(&doc)?;
    let initial = Resource::new(ledger(options), pending.clone())?;
    if initial.before != ledger_before {
        return Err(PlanError::Preconditions);
    }
    let final_record = Resource {
        before: pending,
        after: completed,
        ..initial.clone()
    };
    let mut changes = vec![initial];
    changes.extend(receipt.resources.clone());
    let mut impact=vec![format!("来源保持原管理器所有权，精确固定 {}：{}",v,source.locator),format!("绑定记录：{}；先记录待确认状态，完成受控验证后才标记已绑定。",ledger(options).display()),"只操作预览中的链接和配置；不写 shell 配置，不启动安装，也不通过 mise uninstall 解除链接。".into(),"完整配置备份保存在私有绑定记录与 .zenv-backup；外部修改时停止自动恢复，保留记录供核对。用户终端与 IDE 的实际生效仍需手动确认。".into()];
    for resource in &receipt.resources {
        if let State::Link { target } = &resource.after {
            impact.push(format!(
                "创建链接 {} → {}；原位置必须不存在",
                resource.path.display(),
                target.display()
            ));
        }
    }
    Ok(BindingSpec {
        action: Action::Bind,
        source: source.clone(),
        program,
        impact,
        diff,
        changes,
        final_record,
        guards: vec![(manifest, manifest_state)],
        receipt,
    })
}
pub(super) fn prepare_unbind(
    id: &str,
    options: &NodeScanOptions,
    global: &Path,
) -> Result<BindingSpec, PlanError> {
    if options.platform != Platform::Macos {
        return Err(PlanError::Unsupported);
    }
    let (before, mut doc) = load(options)?;
    let receipt = doc
        .records
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or(PlanError::Stale)?;
    validate_receipt(&receipt, options, global)?;
    let mut changes = vec![];
    let mut diff = vec![];
    // 完成或中断的收据都可显式解除；只接受仍处于原状态或本绑定写入状态的资源。
    for resource in receipt.resources.iter().rev() {
        let current = capture(&resource.destination)?;
        resource.check(&current)?;
        if current != resource.before && current != resource.after {
            return Err(PlanError::BindingConflict);
        }
        if let Some(path) = &receipt.configuration {
            if &resource.path == path {
                let (_, old, new, field) = edit_configuration(
                    resource.before.text().unwrap_or(""),
                    &receipt.source,
                    &receipt.options,
                )?;
                diff.push(ConfigurationDiff {
                    path: path.to_string_lossy().into(),
                    field,
                    before: new,
                    after: old,
                });
            }
        }
        changes.push(Resource {
            before: current,
            after: resource.before.clone(),
            ..resource.clone()
        });
    }
    let initial = Resource::new(ledger(options), before.clone())?;
    if initial.before != before {
        return Err(PlanError::Preconditions);
    }
    changes.insert(0, initial.clone());
    doc.records.retain(|r| r.id != id);
    let final_record = Resource {
        after: encode(&doc)?,
        ..initial
    };
    Ok(BindingSpec {
        action: Action::Unbind,
        source: receipt.source.clone(),
        program: None,
        impact: vec![
            "仅解除该收据确认的链接和配置选择，保留原安装目录；不调用任何卸载命令。".into(),
            format!(
                "绑定记录与配置备份：{}。若外部修改导致冲突，停止自动覆盖。",
                ledger(options).display()
            ),
            "解除选择不会保证其它 Node 版本立即生效；用户终端、项目和 IDE 需重新确认。".into(),
        ],
        diff,
        changes,
        final_record,
        guards: vec![],
        receipt,
    })
}
fn validate_receipt(
    receipt: &Receipt,
    options: &NodeScanOptions,
    global: &Path,
) -> Result<(), PlanError> {
    source_allowed(&receipt.source, options)?;
    let config = configuration_path(&receipt.options, options, global)?;
    if config != receipt.configuration
        || binding_id(&receipt.source, &receipt.options, &config)? != receipt.id
    {
        return Err(PlanError::BindingConflict);
    }
    let v = version(&receipt.source)?;
    let farm = options.home.join(".zenv/managed/node").join(v);
    let registration = options.mise_data_dir.join("installs/node").join(v);
    let mut expected = vec![];
    if receipt.options.strategy != BindingStrategy::ProjectPath
        && receipt.source.provider_package.provider_id != ProviderId::Mise
    {
        if receipt.options.strategy == BindingStrategy::Managed {
            expected.push((
                farm.clone(),
                State::Link {
                    target: PathBuf::from(
                        receipt
                            .source
                            .canonical_executable_path
                            .as_ref()
                            .ok_or(PlanError::BindingConflict)?,
                    )
                    .parent()
                    .and_then(Path::parent)
                    .ok_or(PlanError::BindingConflict)?
                    .into(),
                },
            ));
        }
        expected.push((
            registration,
            State::Link {
                target: if receipt.options.strategy == BindingStrategy::Managed {
                    farm
                } else {
                    PathBuf::from(
                        receipt
                            .source
                            .canonical_executable_path
                            .as_ref()
                            .ok_or(PlanError::BindingConflict)?,
                    )
                    .parent()
                    .and_then(Path::parent)
                    .ok_or(PlanError::BindingConflict)?
                    .into()
                },
            },
        ));
    }
    if receipt.resources.len() != expected.len() + usize::from(config.is_some()) {
        return Err(PlanError::BindingConflict);
    }
    for (resource, (path, after)) in receipt.resources.iter().zip(expected) {
        if resource.path != path
            || resource.before != State::Missing
            || resource.after != after
            || resource.follow_configuration
        {
            return Err(PlanError::BindingConflict);
        }
    }
    if let Some(config) = config {
        let resource = receipt.resources.last().ok_or(PlanError::BindingConflict)?;
        let (next, _, _, _) = edit_configuration(
            resource.before.text().unwrap_or(""),
            &receipt.source,
            &receipt.options,
        )?;
        if resource.path != config
            || !resource.follow_configuration
            || resource.after != State::file(next)
            || matches!(resource.before, State::Link { .. })
        {
            return Err(PlanError::BindingConflict);
        }
    }
    Ok(())
}
