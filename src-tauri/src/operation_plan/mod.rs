//! 后端签发计划、单次执行与最小持久记录；Node 写渠道按原生证据逐项开放。
mod bindings;
mod guards;
mod journal;
pub mod model;
mod python;
mod python_binding;
mod python_mise;
mod python_uv;
mod v2;
mod writes;
use crate::{
    domain::*,
    inventory::{
        has_complete_coverage, merge_inventory,
        query::{InventoryQueryService, InventoryTool},
    },
    operations,
    providers::{scan_node_provider, CancellationToken, NodeScanOptions},
};
use guards::{configuration, file_stamp, FileStamp};
use journal::Journal;
pub use model::*;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub use v2::{OperationPlanV2, OperationSubjectV2, PlanRequestV2, StepGraphEntryV2};
const TTL_MS: u64 = 120_000;
const MAX_PLANS: usize = 64;

#[derive(Clone, Debug, thiserror::Error)]
pub enum PlanError {
    #[error("清单正在刷新或尚未就绪，请刷新完成后重新预览")]
    Inventory,
    #[error("清单已变化，请刷新清单并重新预览")]
    Stale,
    #[error("计划已过期，请重新预览")]
    Expired,
    #[error("计划不存在、已执行或已被替换，请重新预览")]
    Consumed,
    #[error("目标、配置或来源上下文已变化，请重新预览；已完成步骤与补偿结果见任务记录")]
    Preconditions,
    #[error("无法完整读取目标或配置证据，不能生成可执行计划")]
    EvidenceUnavailable,
    #[error("此意图或参数不受支持")]
    Unsupported,
    #[error("此操作尚未开放执行，请查看计划中的能力说明")]
    Disabled,
    #[error("{0}")]
    Capability(&'static str),
    #[error("渠道命令未完成；已保留完成步骤，结果以执行后来源重扫为准")]
    Command,
    #[error("执行后的来源扫描未完整完成；保留旧数据但不能确认最终状态")]
    ScanIncomplete,
    #[error("操作记录无法可靠读写，已停止后续步骤；请检查本地记录文件")]
    Journal,
    #[error("其他应用实例正在使用操作记录，请先在该实例处理任务")]
    JournalBusy,
    #[error("任务已取消；已完成的步骤见任务记录")]
    Cancelled,
    #[error("未能确认计划要求的文件、版本或绑定结果，不能报告验证通过")]
    Verification,
    #[error("执行线程异常中断，未自动重放；请检查步骤状态")]
    Interrupted,
    #[error("操作执行失败：{0}")]
    Execution(String),
    #[error("配置、链接或绑定记录与预览不一致；已停止覆盖，请检查绑定记录与 .zenv-backup")]
    BindingConflict,
}

#[derive(Clone)]
struct StoredPlan {
    view: OperationPlan,
    started: Instant,
    consumed: bool,
    cancel_requested: bool,
    generation: u64,
    target: Option<Installation>,
    context: String,
    config_path: PathBuf,
    config_stamp: Option<FileStamp>,
    files: Vec<(PathBuf, FileStamp)>,
    write: Option<writes::WriteSpec>,
    binding: Option<bindings::BindingSpec>,
    python: Option<crate::providers::python::PythonScanOptions>,
    python_write: Option<python::WriteSpec>,
    python_binding: Option<python_binding::BindingSpec>,
}
struct State {
    plans: VecDeque<StoredPlan>,
    sequence: u64,
    journal: Journal,
    journal_failed: bool,
    initialized: bool,
}
#[derive(Clone)]
pub struct OperationPlanService {
    state: Arc<Mutex<State>>,
}
fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}
pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(9_007_199_254_740_991) as u64
}
pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
fn step(kind: StepKind, title: &str, home: &Path) -> PlanStep {
    PlanStep {
        kind,
        title: title.into(),
        program: None,
        argv: vec![],
        cwd: home.to_string_lossy().into(),
        environment_keys: vec!["HOME".into(), "PATH".into()],
    }
}
fn ready(snapshot: &InventorySnapshot, provider: ProviderId) -> bool {
    snapshot
        .providers
        .iter()
        .any(|p| p.provider_id == provider && has_complete_coverage(p, &ToolId::from("node")))
}
fn exact_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 10
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}
impl OperationPlanService {
    pub fn bindings(&self) -> Result<Vec<BindingSummary>, PlanError> {
        let options =
            NodeScanOptions::from_environment().map_err(|_| PlanError::EvidenceUnavailable)?;
        bindings::list(&options)
    }
    pub fn new(journal_path: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                plans: VecDeque::new(),
                sequence: 0,
                journal: Journal::new(journal_path),
                journal_failed: false,
                initialized: false,
            })),
        }
    }
    /// 启动时只恢复已有 journal，新的只读会话不创建数据目录。
    pub fn recover(&self) -> Result<(), PlanError> {
        let mut state = lock(&self.state);
        if state.initialized {
            return if state.journal_failed {
                Err(PlanError::Journal)
            } else {
                Ok(())
            };
        }
        state.initialized = true;
        let result = state.journal.recover_existing();
        if result.is_err() {
            state.journal_failed = true;
        }
        result
    }
    pub fn records(&self) -> (Vec<ExecutionRecord>, bool) {
        let state = lock(&self.state);
        (state.journal.records.clone(), state.journal_failed)
    }
    pub fn preview_current(
        &self,
        query: &InventoryQueryService,
        request: PlanRequest,
    ) -> Result<OperationPlan, PlanError> {
        let options =
            NodeScanOptions::from_environment().map_err(|_| PlanError::EvidenceUnavailable)?;
        let config =
            crate::env_center::global_config_path().ok_or(PlanError::EvidenceUnavailable)?;
        self.preview(query, request, options, config)
    }
    pub fn preview(
        &self,
        query: &InventoryQueryService,
        request: PlanRequest,
        options: NodeScanOptions,
        config_path: PathBuf,
    ) -> Result<OperationPlan, PlanError> {
        let (generation, snapshot) = query.current_snapshot().map_err(|_| PlanError::Inventory)?;
        if snapshot.request_id != request.snapshot_request_id {
            return Err(PlanError::Stale);
        }
        let (configuration, config_stamp) = configuration(&config_path)?;
        let requested_version = match &request.intent {
            PlanIntent::Install { version, .. } => version.clone(),
            _ => None,
        };
        let binding_options = match &request.intent {
            PlanIntent::Bind { binding, .. } => binding.clone(),
            _ => None,
        };
        let mut binding = match &request.intent {
            PlanIntent::Unbind { binding_id } => Some(bindings::prepare_unbind(
                binding_id,
                &options,
                &config_path,
            )?),
            _ => None,
        };
        let requested_action = match &request.intent {
            PlanIntent::Verify { .. } => Action::Verify,
            PlanIntent::Install { .. } => Action::Install,
            PlanIntent::Uninstall { .. } => Action::Uninstall,
            PlanIntent::Bind { .. } => Action::Bind,
            PlanIntent::Unbind { .. } => Action::Unbind,
        };
        let (action, target, package, mut version_promise, mut capability, mut steps) =
            match request.intent {
                PlanIntent::Verify { installation_id }
                | PlanIntent::Uninstall { installation_id }
                | PlanIntent::Bind {
                    installation_id, ..
                } => {
                    let target = snapshot
                        .installations
                        .iter()
                        .find(|i| i.installation_id == installation_id)
                        .cloned()
                        .ok_or(PlanError::Stale)?;
                    let action = requested_action;
                    let reason =
                        if !ready(&snapshot, target.provider_package.provider_id) || target.stale {
                            Some("来源证据不完整或已过期，请先刷新该来源".into())
                        } else if action == Action::Uninstall {
                            Some(
                        if target.ownership != Ownership::Owned {
                            "未确认删除归属；不能将 nvm、mise 目录观测或 PATH 记录交给其他来源删除"
                        } else {
                            "卸载依赖与多版本影响尚未核定，暂不能在此执行卸载"
                        }
                        .into(),
                    )
                        } else if action == Action::Bind {
                            Some("项目绑定及配置差异预览尚未开放，当前不改写配置".into())
                        } else if target.health != Health::Healthy
                            || target.version.is_none()
                            || target.provider_package.provider_id == ProviderId::Path
                        {
                            Some("此实例尚无受信任的验证能力，未知 PATH 文件不会自动执行".into())
                        } else {
                            None
                        };
                    let capability = Capability {
                        can_preview: reason.is_none(),
                        can_execute: reason.is_none(),
                        reason,
                    };
                    let steps = if capability.can_preview {
                        vec![
                            step(
                                StepKind::Preflight,
                                "重验来源、目标文件和配置指纹",
                                &options.home,
                            ),
                            step(
                                StepKind::Refresh,
                                "只读重扫此来源的全部 Node 记录",
                                &options.home,
                            ),
                            step(
                                StepKind::Verify,
                                "确认原实例身份、版本与可执行状态",
                                &options.home,
                            ),
                        ]
                    } else {
                        vec![]
                    };
                    (
                        action,
                        Some(target.clone()),
                        target.provider_package.clone(),
                        format!(
                            "只确认原记录的精确版本 {}；不代表项目或终端生效",
                            target.version.as_deref().unwrap_or("未知")
                        ),
                        capability,
                        steps,
                    )
                }
                PlanIntent::Unbind { .. } => {
                    let spec = binding.as_ref().ok_or(PlanError::Unsupported)?;
                    (
                        Action::Unbind,
                        Some(spec.source.clone()),
                        spec.source.provider_package.clone(),
                        "只解除本绑定，不卸载原始安装".into(),
                        Capability {
                            can_preview: true,
                            can_execute: true,
                            reason: None,
                        },
                        vec![],
                    )
                }
                PlanIntent::Install {
                    provider_id,
                    package_id,
                    version,
                } => {
                    if package_id.len() > 64 {
                        return Err(PlanError::Unsupported);
                    }
                    let (package, argv, promise, name) = match provider_id {
                        ProviderId::Mise if package_id == "node" => {
                            let version = version
                                .filter(|v| exact_version(v))
                                .ok_or(PlanError::Unsupported)?;
                            (
                                "node".to_string(),
                                vec!["install".into(), format!("node@{version}")],
                                format!("请求精确版本 {version}，安装结果仍需重扫确认"),
                                "mise",
                            )
                        }
                        ProviderId::Brew => {
                            let package = native_brew_node_formula(&package_id)
                                .ok_or(PlanError::Unsupported)?;
                            if version.is_some() {
                                return Err(PlanError::Unsupported);
                            }
                            (
                            package.clone(),
                            vec!["install".into(), "--formula".into(), package],
                            "安装该 formula 执行时提供的版本；不承诺精确补丁版，可能产生依赖变化"
                                .into(),
                            "brew",
                        )
                        }
                        _ => return Err(PlanError::Unsupported),
                    };
                    let program = guards::executable(&options, name);
                    let reason = if !ready(&snapshot, provider_id) {
                        Some("来源情况未知，请先刷新；不能以失败扫描推断可安装".into())
                    } else if program.is_none() {
                        Some("未找到可定位的管理器程序，目录可观测不代表管理器可执行".into())
                    } else {
                        Some("当前支持预览安装步骤，安装执行暂未开放".into())
                    };
                    let can_preview = ready(&snapshot, provider_id) && program.is_some();
                    let mut command = step(
                        StepKind::Command,
                        "调用指定来源安装 Node；不会自动全局激活",
                        &options.home,
                    );
                    command.program = program.map(|p| p.to_string_lossy().into());
                    command.argv = argv;
                    let steps = if can_preview {
                        vec![
                            step(
                                StepKind::Preflight,
                                "重验来源、管理器和配置指纹",
                                &options.home,
                            ),
                            command,
                            step(StepKind::Refresh, "重扫来源确认实际安装结果", &options.home),
                            step(
                                StepKind::Verify,
                                "核对版本承诺；项目和终端状态另行验证",
                                &options.home,
                            ),
                        ]
                    } else {
                        vec![]
                    };
                    let package = ProviderPackage {
                        provider_id,
                        package_id: package,
                        kind: if provider_id == ProviderId::Brew {
                            PackageKind::Formula
                        } else {
                            PackageKind::Runtime
                        },
                        platform: options.platform,
                    };
                    (
                        Action::Install,
                        None,
                        package,
                        promise,
                        Capability {
                            can_preview,
                            can_execute: false,
                            reason,
                        },
                        steps,
                    )
                }
            };
        let mut write = None;
        let mut impact = vec![];
        let mut configuration_diff = vec![];
        if matches!(action, Action::Install | Action::Uninstall) {
            match writes::prepare(
                action,
                &package,
                target.as_ref(),
                requested_version.as_deref(),
                &snapshot,
                &options,
            ) {
                Ok(spec) => {
                    capability = Capability {
                        can_preview: true,
                        can_execute: true,
                        reason: None,
                    };
                    let mut command = step(
                        StepKind::Command,
                        if action == Action::Install {
                            "安装所选原生包；不自动激活 Node"
                        } else {
                            "卸载已核对的精确目标；不删除共享依赖"
                        },
                        &options.home,
                    );
                    command.program = Some(spec.program.to_string_lossy().into());
                    command.argv = spec.argv.clone();
                    steps = vec![
                        step(
                            StepKind::Preflight,
                            "重验原生管理器记录、目标及影响范围",
                            &options.home,
                        ),
                        command,
                        step(
                            StepKind::Refresh,
                            "无论命令成功、失败或取消都重扫来源",
                            &options.home,
                        ),
                        step(
                            StepKind::Verify,
                            "核对实际实例；终端与项目状态另行验证",
                            &options.home,
                        ),
                    ];
                    for step in &mut steps {
                        step.environment_keys = writes::environment_keys(package.provider_id);
                    }
                    version_promise = if action == Action::Uninstall {
                        format!(
                            "只移除已核对的实例 {}；不改写项目或全局版本请求",
                            spec.version
                        )
                    } else if package.provider_id == ProviderId::Brew {
                        format!(
                            "当前渠道元数据版本为 {}；执行前重验，不接受精确补丁版安装承诺",
                            spec.version
                        )
                    } else {
                        format!(
                            "请求并验证精确版本 {}；不会自动切换终端或项目",
                            spec.version
                        )
                    };
                    impact = spec.impact.clone();
                    write = Some(spec);
                }
                Err(error) => {
                    capability = Capability {
                        can_preview: false,
                        can_execute: false,
                        reason: Some(error.to_string()),
                    };
                    steps.clear();
                }
            }
        }
        if action == Action::Bind {
            if let Some(binding_options) = binding_options {
                match bindings::prepare_bind(
                    target.as_ref().ok_or(PlanError::Unsupported)?,
                    &binding_options,
                    &snapshot,
                    &options,
                    &config_path,
                ) {
                    Ok(spec) => binding = Some(spec),
                    Err(error) => {
                        capability = Capability {
                            can_preview: false,
                            can_execute: false,
                            reason: Some(error.to_string()),
                        };
                        steps.clear();
                    }
                }
            }
        }
        if let Some(spec) = &binding {
            capability = Capability {
                can_preview: true,
                can_execute: true,
                reason: None,
            };
            steps = vec![
                step(
                    StepKind::Preflight,
                    "重验来源、绑定资源与配置原文",
                    &options.home,
                ),
                step(
                    StepKind::Command,
                    "比较后修改绑定资源，先保存待确认收据",
                    &options.home,
                ),
                step(
                    StepKind::Refresh,
                    "只读重扫受影响的 mise 来源",
                    &options.home,
                ),
                step(
                    StepKind::Verify,
                    "核对链接、配置与受控进程；失败时单独补偿",
                    &options.home,
                ),
            ];
            if let Some(program) = &spec.program {
                steps[3].program = Some(program.to_string_lossy().into());
                steps[3].argv = vec![
                    "--no-config".into(),
                    "--no-env".into(),
                    "--no-hooks".into(),
                    "exec".into(),
                    format!("core:node@{}", spec.source.version.as_deref().unwrap_or("")),
                    "--".into(),
                    "node".into(),
                    "-p".into(),
                    "JSON.stringify({version:process.versions.node,execPath:process.execPath})"
                        .into(),
                ];
            }
            for step in &mut steps {
                step.environment_keys = writes::environment_keys(ProviderId::Mise);
                step.environment_keys.extend([
                    "MISE_AUTO_INSTALL".into(),
                    "MISE_EXEC_AUTO_INSTALL".into(),
                    "MISE_NOT_FOUND_AUTO_INSTALL".into(),
                ]);
            }
            impact = spec.impact.clone();
            configuration_diff = spec.diff.clone();
            version_promise = format!(
                "精确实例 {}；登记、配置选择与受控验证分别确认，用户终端及 IDE 未验证",
                spec.source.version.as_deref().unwrap_or("未知")
            );
        }
        if options.platform != Platform::Macos {
            capability = Capability {
                can_preview: false,
                can_execute: false,
                reason: Some("新操作计划当前仅支持 macOS Node 试点".into()),
            };
            steps.clear();
        }
        let is_install = action == Action::Install;
        let mut files = Vec::new();
        if !is_install && action != Action::Unbind && capability.can_execute {
            let path = PathBuf::from(
                target
                    .as_ref()
                    .ok_or(PlanError::Unsupported)?
                    .executable_path
                    .as_ref()
                    .ok_or(PlanError::EvidenceUnavailable)?,
            );
            files.push((path.clone(), file_stamp(&path)?));
        }
        for program in steps.iter().filter_map(|s| s.program.as_ref()) {
            let path = PathBuf::from(program);
            files.push((path.clone(), file_stamp(&path)?));
        }
        let view = OperationPlan {
            schema_version: 1,
            tool_id: InventoryTool::Node,
            plan_id: String::new(),
            created_at: 0,
            expires_at: 0,
            snapshot_generation: generation.to_string(),
            snapshot_request_id: snapshot.request_id,
            action,
            target: PlanTarget {
                provider_package: package,
                installation_id: target.as_ref().map(|t| t.installation_id.clone()),
                locator: target.as_ref().map(|t| t.locator.clone()),
            },
            capability,
            version_promise,
            configuration,
            configuration_change: if binding.is_some() {
                "仅修改下列差异与绑定记录；备份包含完整原配置，外部修改时不自动覆盖"
            } else if write.is_some() {
                "不改写 Z.Env / mise 版本请求或 shell 配置；原生管理器与依赖副作用见操作影响和恢复边界"
            } else {
                "本计划不改写配置文件"
            }.into(),
            permission: "当前用户；不提权".into(),
            rollback: if binding.is_some() {
                "失败或取消后单独补偿本次仍未被外部修改的资源；冲突时保留收据与备份，不删除原安装"
            } else if write.is_some() {
                "成功下载、缓存和外部管理器副作用可能保留；失败或取消后重扫，不自动重装或删除共享依赖"
            } else if is_install {
                "下载、依赖和包管理器副作用不保证可回滚；不会自动删除共享依赖"
            } else {
                "只读验证不改变原安装或配置；只更新清单缓存并记录步骤状态"
            }
            .into(),
            steps,
            impact,
            configuration_diff,
        };
        self.store(StoredPlan {
            view,
            started: Instant::now(),
            consumed: false,
            cancel_requested: false,
            generation,
            target,
            context: guards::context(&options, &config_path),
            config_path,
            config_stamp,
            files,
            write,
            binding,
            python: None,
            python_write: None,
            python_binding: None,
        })
    }
    fn store(&self, mut stored: StoredPlan) -> Result<OperationPlan, PlanError> {
        let mut state = lock(&self.state);
        if state.journal_failed {
            stored.view.capability.can_execute = false;
            stored.view.capability.reason = Some(PlanError::Journal.to_string());
        }
        state.sequence = state.sequence.checked_add(1).ok_or(PlanError::Consumed)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        stored.view.plan_id = format!("plan-{nonce:x}-{:x}", state.sequence);
        stored.view.created_at = now();
        stored.view.expires_at = stored.view.created_at.saturating_add(TTL_MS);
        let view = stored.view.clone();
        state.plans.push_back(stored);
        while state.plans.len() > MAX_PLANS {
            state.plans.pop_front();
        }
        Ok(view)
    }
    fn record(
        &self,
        id: &str,
        edit: impl FnOnce(&mut ExecutionRecord),
    ) -> Result<ExecutionRecord, PlanError> {
        let mut state = lock(&self.state);
        let record = state
            .journal
            .records
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or(PlanError::Journal)?;
        edit(record);
        record.updated_at = now();
        let result = record.clone();
        if state.journal.persist().is_err() {
            state.journal_failed = true;
            operations::interrupted(id);
            return Err(PlanError::Journal);
        }
        Ok(result)
    }
    /// 对话框可在线程登记任务之前请求取消；记录只作用于尚未消费的本地计划。
    pub(crate) fn cancel_pending(&self, id: &str) -> bool {
        let mut state = lock(&self.state);
        if let Some(plan) = state
            .plans
            .iter_mut()
            .find(|p| p.view.plan_id == id && !p.consumed && p.view.capability.can_execute)
        {
            plan.cancel_requested = true;
            true
        } else {
            false
        }
    }
    fn validate(
        plan: &StoredPlan,
        query: &InventoryQueryService,
        options: &NodeScanOptions,
        config_path: &Path,
        python: Option<&crate::providers::python::PythonScanOptions>,
    ) -> Result<(), PlanError> {
        if now() < plan.view.created_at
            || now() >= plan.view.expires_at
            || plan.started.elapsed() >= Duration::from_millis(TTL_MS)
        {
            return Err(PlanError::Expired);
        }
        let (generation, snapshot) = query
            .current_snapshot_for(plan.view.tool_id)
            .map_err(|_| PlanError::Inventory)?;
        if generation != plan.generation || snapshot.request_id != plan.view.snapshot_request_id {
            return Err(PlanError::Stale);
        }
        let context = match (plan.view.tool_id, python) {
            (InventoryTool::Node, None) => guards::context(options, config_path),
            (InventoryTool::Python, Some(p)) => python::context(p, config_path),
            _ => return Err(PlanError::Preconditions),
        };
        if context != plan.context || config_path != plan.config_path {
            return Err(PlanError::Preconditions);
        }
        let (config, stamp) = configuration(config_path)?;
        if config != plan.view.configuration || stamp != plan.config_stamp {
            return Err(PlanError::Preconditions);
        }
        for (path, expected) in &plan.files {
            if file_stamp(path)? != *expected {
                return Err(PlanError::Preconditions);
            }
        }
        Ok(())
    }
    pub fn execute(
        &self,
        plan_id: String,
        query: InventoryQueryService,
    ) -> Result<ExecutionRecord, PlanError> {
        let tool = lock(&self.state)
            .plans
            .iter()
            .find(|p| p.view.plan_id == plan_id)
            .map(|p| p.view.tool_id)
            .ok_or(PlanError::Consumed)?;
        if tool == InventoryTool::Python {
            return self.execute_python_with_context(&plan_id, query, || {
                let options = crate::providers::python::PythonScanOptions::from_environment()
                    .map_err(|_| PlanError::EvidenceUnavailable)?;
                let config = crate::env_center::global_config_path()
                    .ok_or(PlanError::EvidenceUnavailable)?;
                Ok((options, config))
            });
        }
        self.execute_native_with_context(&plan_id, query, || {
            let options =
                NodeScanOptions::from_environment().map_err(|_| PlanError::EvidenceUnavailable)?;
            let config =
                crate::env_center::global_config_path().ok_or(PlanError::EvidenceUnavailable)?;
            Ok((options, config))
        })
    }
    fn execute_native_with_context(
        &self,
        plan_id: &str,
        query: InventoryQueryService,
        context: impl FnMut() -> Result<(NodeScanOptions, PathBuf), PlanError>,
    ) -> Result<ExecutionRecord, PlanError> {
        let mut mutation: Option<crate::inventory::query::QueryLease> = None;
        let mut binding_transaction = bindings::Transaction::default();
        let mut refresh_sequence = 0_u32;
        self.execute_with(
            plan_id,
            &query,
            context,
            |kind, stored, options, id| match kind {
                StepKind::Preflight => {
                    if let Some(binding) = &stored.binding {
                        let (_, snapshot) =
                            query.current_snapshot().map_err(|_| PlanError::Inventory)?;
                        if stored.view.action == Action::Unbind {
                            return binding.revalidate(&snapshot, options);
                        }
                    }
                    let provider = stored.view.target.provider_package.provider_id;
                    let token = CancellationToken::default();
                    let watch = CancelWatch::start(id, token.clone());
                    let scan = scan_node_provider(provider, options, &token);
                    drop(watch);
                    if operations::cancelled(id) {
                        return Err(PlanError::Cancelled);
                    }
                    let fresh = merge_inventory("plan-preflight".into(), now(), vec![scan])
                        .map_err(|_| PlanError::Preconditions)?;
                    if let Some(binding) = &stored.binding {
                        binding.revalidate(&fresh, options)
                    } else if let Some(write) = &stored.write {
                        write.revalidate(&fresh, stored.target.as_ref(), options)
                    } else {
                        verify_target(
                            &fresh,
                            stored.target.as_ref().ok_or(PlanError::Unsupported)?,
                        )
                        .map_err(|_| PlanError::Preconditions)
                    }
                }
                StepKind::Command => {
                    if let Some(binding) = &stored.binding {
                        let (_, snapshot) =
                            query.current_snapshot().map_err(|_| PlanError::Inventory)?;
                        binding.revalidate(&snapshot, options)?;
                        let lease = query
                            .begin(format!("mutation-{id}"), Some(ProviderId::Mise))
                            .map_err(|_| PlanError::Inventory)?;
                        lease.mark_mutating().map_err(|_| PlanError::Inventory)?;
                        mutation = Some(lease);
                        return binding.apply(&mut binding_transaction, id);
                    }
                    let write = stored.write.as_ref().ok_or(PlanError::Disabled)?;
                    let (_, snapshot) =
                        query.current_snapshot().map_err(|_| PlanError::Inventory)?;
                    write.revalidate(&snapshot, stored.target.as_ref(), options)?;
                    let lease = query
                        .begin(format!("mutation-{id}"), Some(write.provider))
                        .map_err(|_| PlanError::Inventory)?;
                    lease.mark_mutating().map_err(|_| PlanError::Inventory)?;
                    mutation = Some(lease);
                    let token = CancellationToken::default();
                    let watch = CancelWatch::start(id, token.clone());
                    let result = write.run(options, &token);
                    drop(watch);
                    result
                }
                StepKind::Refresh => {
                    let provider = if stored.binding.is_some() {
                        ProviderId::Mise
                    } else {
                        stored.view.target.provider_package.provider_id
                    };
                    if stored.write.is_some() || stored.binding.is_some() {
                        crate::syscache::invalidate(if provider == ProviderId::Brew {
                            "brew"
                        } else {
                            "mise"
                        });
                    }
                    refresh_sequence += 1;
                    let lease = match mutation.take() {
                        Some(lease) => lease,
                        None => query
                            .begin(format!("verify-{id}-{refresh_sequence}"), Some(provider))
                            .map_err(|_| PlanError::Inventory)?,
                    };
                    // 写入后的核对不再继承取消，否则无法报告真实残留。
                    let result = lease
                        .execute(options.clone())
                        .map_err(|_| PlanError::ScanIncomplete)?;
                    if stored.write.is_some() && !ready(&result, provider) {
                        return Err(PlanError::ScanIncomplete);
                    }
                    Ok(())
                }
                StepKind::Verify => {
                    if let Some(binding) = &stored.binding {
                        return binding.verify(options, &mut binding_transaction, id);
                    }
                    let (_, snapshot) =
                        query.current_snapshot().map_err(|_| PlanError::Inventory)?;
                    if let Some(write) = &stored.write {
                        write.verify(&snapshot, options)
                    } else {
                        verify_target(
                            &snapshot,
                            stored.target.as_ref().ok_or(PlanError::Unsupported)?,
                        )
                    }
                }
                StepKind::Compensate => {
                    if mutation.is_none() {
                        let lease = query
                            .begin(format!("compensate-{id}"), Some(ProviderId::Mise))
                            .map_err(|_| PlanError::Inventory)?;
                        lease.mark_mutating().map_err(|_| PlanError::Inventory)?;
                        mutation = Some(lease);
                    }
                    binding_transaction.rollback()
                }
            },
        )
    }
    fn execute_with(
        &self,
        plan_id: &str,
        query: &InventoryQueryService,
        mut context: impl FnMut() -> Result<(NodeScanOptions, PathBuf), PlanError>,
        driver: impl FnMut(StepKind, &StoredPlan, &NodeScanOptions, &str) -> Result<(), PlanError>,
    ) -> Result<ExecutionRecord, PlanError> {
        self.execute_with_scope(
            plan_id,
            query,
            || {
                let (options, config) = context()?;
                Ok((options, config, None))
            },
            driver,
        )
    }
    fn execute_with_scope(
        &self,
        plan_id: &str,
        query: &InventoryQueryService,
        mut context: impl FnMut() -> Result<
            (
                NodeScanOptions,
                PathBuf,
                Option<crate::providers::python::PythonScanOptions>,
            ),
            PlanError,
        >,
        mut driver: impl FnMut(StepKind, &StoredPlan, &NodeScanOptions, &str) -> Result<(), PlanError>,
    ) -> Result<ExecutionRecord, PlanError> {
        if !valid_id(plan_id) {
            return Err(PlanError::Consumed);
        }
        let label = {
            let state = lock(&self.state);
            let plan = state
                .plans
                .iter()
                .find(|p| p.view.plan_id == plan_id && !p.consumed)
                .ok_or(PlanError::Consumed)?;
            if !plan.view.capability.can_execute {
                return Err(PlanError::Disabled);
            }
            plan.view.action.label_for(plan.view.tool_id)
        };
        // operations::run 以 AppError 返回，内部具体失败原因在闭包内单独保留；
        // 否则 Preconditions/Stale/Expired 等会被压平成 Execution，调用方和测试无法区分。
        let plan_failure: Mutex<Option<PlanError>> = Mutex::new(None);
        operations::run(Some(plan_id.into()), label, |id| {
            let result = (|| {
                let stored = {
                    let mut state = lock(&self.state);
                    if state.journal_failed {
                        return Err(PlanError::Journal);
                    }
                    let stored = state
                        .plans
                        .iter_mut()
                        .find(|p| p.view.plan_id == id && !p.consumed)
                        .ok_or(PlanError::Consumed)?;
                    stored.consumed = true;
                    stored.clone()
                };
                if stored.cancel_requested {
                    let _ = operations::cancel(id);
                    return Err(PlanError::Cancelled);
                }
                let (options, config, python) = context()?;
                Self::validate(&stored, query, &options, &config, python.as_ref())?;
                let record = ExecutionRecord {
                    schema_version: 1,
                    tool_id: stored.view.tool_id,
                    id: id.into(),
                    action: stored.view.action,
                    status: ExecutionStatus::Running,
                    created_at: now(),
                    updated_at: now(),
                    steps: stored
                        .view
                        .steps
                        .iter()
                        .map(|s| ExecutionStep {
                            kind: s.kind,
                            status: StepStatus::Planned,
                        })
                        .collect(),
                    outcome: None,
                    compensation: None,
                };
                {
                    let mut state = lock(&self.state);
                    state.initialized = true;
                    if let Err(error) = state.journal.push(record) {
                        state.journal_failed = true;
                        return Err(error);
                    }
                }
                let mut changed = false;
                let mut refreshed = false;
                let mut failure = None;
                for (index, step) in stored.view.steps.iter().enumerate() {
                    if operations::cancelled(id) {
                        failure = Some(PlanError::Cancelled);
                        break;
                    }
                    if let Err(error) =
                        self.record(id, |r| r.steps[index].status = StepStatus::Running)
                    {
                        failure = Some(error);
                        break;
                    }
                    let mut entered = false;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if step.kind == StepKind::Command {
                            let (latest_options, latest_config, python) = context()?;
                            Self::validate(
                                &stored,
                                query,
                                &latest_options,
                                &latest_config,
                                python.as_ref(),
                            )?;
                            entered = true;
                        }
                        if step.kind == StepKind::Refresh {
                            refreshed = true;
                        }
                        driver(step.kind, &stored, &options, id)
                    }))
                    .unwrap_or(Err(PlanError::Interrupted));
                    changed |= entered;
                    let status = if result.is_ok() {
                        StepStatus::Completed
                    } else {
                        StepStatus::Failed
                    };
                    if let Err(error) = self.record(id, |r| r.steps[index].status = status) {
                        failure = Some(error);
                        break;
                    }
                    if let Err(error) = result {
                        failure = Some(error);
                        break;
                    }
                    operations::log(
                        id,
                        match step.kind {
                            StepKind::Preflight => "前置条件核对完成",
                            StepKind::Command => "变更步骤完成",
                            StepKind::Refresh => "来源重扫完成",
                            StepKind::Verify => "结果验证完成",
                            StepKind::Compensate => "补偿完成",
                        },
                    );
                }
                if failure.is_some() && changed && stored.binding.is_some() {
                    let _ = self.record(id, |r| {
                        r.steps.push(ExecutionStep {
                            kind: StepKind::Compensate,
                            status: StepStatus::Running,
                        })
                    });
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        driver(StepKind::Compensate, &stored, &options, id)
                    }))
                    .unwrap_or(Err(PlanError::Interrupted));
                    let outcome = match &result {
                        Ok(()) => CompensationOutcome::Completed,
                        Err(PlanError::BindingConflict) => CompensationOutcome::Conflict,
                        Err(_) => CompensationOutcome::Failed,
                    };
                    let _ = self.record(id, |r| {
                        r.compensation = Some(outcome);
                        if let Some(step) =
                            r.steps.iter_mut().find(|s| s.kind == StepKind::Compensate)
                        {
                            step.status = if result.is_ok() {
                                StepStatus::Completed
                            } else {
                                StepStatus::Failed
                            };
                        }
                    });
                    if result.is_err() {
                        failure = Some(PlanError::BindingConflict);
                    }
                    refreshed = false;
                }
                // 只读重扫属于清理阶段；取消、命令失败、恐慌或 journal 故障均不能跳过。
                if changed && !refreshed {
                    if let Some(index) = stored
                        .view
                        .steps
                        .iter()
                        .position(|s| s.kind == StepKind::Refresh)
                    {
                        let _ = self.record(id, |r| r.steps[index].status = StepStatus::Running);
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            driver(StepKind::Refresh, &stored, &options, id)
                        }))
                        .unwrap_or(Err(PlanError::Interrupted));
                        let _ = self.record(id, |r| {
                            r.steps[index].status = if result.is_ok() {
                                StepStatus::Completed
                            } else {
                                StepStatus::Failed
                            }
                        });
                        if result.is_err()
                            && !matches!(failure, Some(PlanError::Journal | PlanError::Interrupted))
                            && !matches!(failure, Some(PlanError::BindingConflict))
                        {
                            failure = Some(PlanError::ScanIncomplete);
                        }
                    }
                }
                if let Some(error) = failure {
                    let interrupted = matches!(error, PlanError::Interrupted | PlanError::Journal)
                        || lock(&self.state).journal_failed;
                    let cancelled =
                        operations::cancelled(id) || matches!(error, PlanError::Cancelled);
                    let status = if interrupted {
                        operations::interrupted(id);
                        ExecutionStatus::Interrupted
                    } else if cancelled {
                        ExecutionStatus::Cancelled
                    } else if changed {
                        operations::partial(id);
                        ExecutionStatus::Partial
                    } else {
                        ExecutionStatus::Failed
                    };
                    let outcome = if interrupted {
                        Outcome::Interrupted
                    } else if matches!(error, PlanError::BindingConflict) {
                        Outcome::CompensationConflict
                    } else if matches!(error, PlanError::ScanIncomplete) {
                        Outcome::ScanIncomplete
                    } else if cancelled {
                        Outcome::Cancelled
                    } else if matches!(
                        error,
                        PlanError::Preconditions | PlanError::Stale | PlanError::Expired
                    ) {
                        Outcome::PreconditionChanged
                    } else {
                        Outcome::StepFailed
                    };
                    self.finish(id, status, outcome)?;
                    return Err(error);
                }
                self.finish(
                    id,
                    ExecutionStatus::Completed,
                    match stored.view.action {
                        Action::Install => Outcome::Installed,
                        Action::Uninstall => Outcome::Uninstalled,
                        Action::Bind => Outcome::Bound,
                        Action::Unbind => Outcome::Unbound,
                        _ => Outcome::Verified,
                    },
                )
            })();
            if let Err(error) = &result {
                *plan_failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error.clone());
            }
            result.map_err(|e| crate::error::AppError::Other(e.to_string()))
        })
        .map_err(|e| {
            plan_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
                .unwrap_or_else(|| PlanError::Execution(e.to_string()))
        })
    }
    fn finish(
        &self,
        id: &str,
        status: ExecutionStatus,
        outcome: Outcome,
    ) -> Result<ExecutionRecord, PlanError> {
        self.record(id, |r| {
            r.status = status;
            r.outcome = Some(outcome);
            for s in &mut r.steps {
                if s.status == StepStatus::Planned {
                    s.status = StepStatus::Skipped;
                }
            }
        })
    }
}
fn verify_target(snapshot: &InventorySnapshot, target: &Installation) -> Result<(), PlanError> {
    if !ready(snapshot, target.provider_package.provider_id) {
        return Err(PlanError::Verification);
    }
    snapshot
        .installations
        .iter()
        .find(|i| {
            i.installation_id == target.installation_id
                && !i.stale
                && i.health == Health::Healthy
                && i.ownership == target.ownership
        })
        .map(|_| ())
        .ok_or(PlanError::Verification)
}
struct CancelWatch {
    done: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl CancelWatch {
    fn start(id: &str, token: CancellationToken) -> Self {
        Self::with_action(id, move || token.cancel())
    }
    fn with_action(id: &str, action: impl FnOnce() + Send + 'static) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        let id = id.to_string();
        let worker = std::thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                if operations::cancelled(&id) {
                    action();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Self {
            done,
            worker: Some(worker),
        }
    }
}
impl Drop for CancelWatch {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod python_tests;
