//! Python 复用计划签发与任务执行器；仅实例验证可执行，安装步骤只供审阅。
use super::*;
use crate::providers::python::{scan_python_provider, PythonScanOptions};

/// Python 写渠道只允许 mise 与 uv；执行器按渠道分派，不接受任意命令。
#[derive(Clone)]
pub(super) enum WriteSpec {
    Mise(python_mise::WriteSpec),
    Uv(python_uv::WriteSpec),
}
impl WriteSpec {
    fn program(&self) -> &PathBuf {
        match self {
            Self::Mise(spec) => &spec.program,
            Self::Uv(spec) => &spec.program,
        }
    }
    fn argv(&self) -> &[String] {
        match self {
            Self::Mise(spec) => &spec.argv,
            Self::Uv(spec) => &spec.argv,
        }
    }
    fn version(&self) -> &str {
        match self {
            Self::Mise(spec) => &spec.version,
            Self::Uv(spec) => &spec.version,
        }
    }
    fn impact(&self) -> &[String] {
        match self {
            Self::Mise(spec) => &spec.impact,
            Self::Uv(spec) => &spec.impact,
        }
    }
    fn environment_keys(&self) -> Vec<String> {
        match self {
            Self::Mise(_) => python_mise::environment_keys(),
            Self::Uv(_) => python_uv::environment_keys(),
        }
    }
    fn revalidate(
        &self,
        target: Option<&Installation>,
        snapshot: &InventorySnapshot,
        options: &PythonScanOptions,
    ) -> Result<(), PlanError> {
        match self {
            Self::Mise(spec) => spec.revalidate(target, snapshot, options),
            Self::Uv(spec) => spec.revalidate(target, snapshot, options),
        }
    }
    fn run(&self, options: &PythonScanOptions, token: &CancellationToken) -> Result<(), PlanError> {
        match self {
            Self::Mise(spec) => spec.run(options, token),
            Self::Uv(spec) => spec.run(options, token),
        }
    }
    fn verify(
        &self,
        snapshot: &InventorySnapshot,
        options: &PythonScanOptions,
    ) -> Result<(), PlanError> {
        match self {
            Self::Mise(spec) => spec.verify(snapshot, options),
            Self::Uv(spec) => spec.verify(snapshot, options),
        }
    }
}

pub(super) fn context(options: &PythonScanOptions, config: &Path) -> String {
    format!(
        "python|{}|{:?}|{:?}",
        guards::context(&options.common, config),
        options.pyenv_root,
        options.uv_python_dir
    )
}
fn target_ready(snapshot: &InventorySnapshot, target: &Installation) -> bool {
    target.tool_id == ToolId::from("python")
        && target.provider_package.platform == Platform::Macos
        && !target.stale
        && target.health == Health::Healthy
        && target.version.is_some()
        && target.provider_package.provider_id != ProviderId::Path
        && snapshot.providers.iter().any(|p| {
            p.provider_id == target.provider_package.provider_id
                && !p.stale
                && p.coverage.tool_ids.contains(&ToolId::from("python"))
        })
}
fn verify(snapshot: &InventorySnapshot, target: &Installation) -> Result<(), PlanError> {
    snapshot
        .installations
        .iter()
        .find(|i| {
            i.installation_id == target.installation_id
                && target_ready(snapshot, i)
                && i.provider_package == target.provider_package
                && i.version == target.version
                && i.executable_path == target.executable_path
                && i.canonical_executable_path == target.canonical_executable_path
                && i.architecture == target.architecture
                && i.ownership == target.ownership
        })
        .map(|_| ())
        .ok_or(PlanError::Verification)
}
pub(super) fn install(
    provider: ProviderId,
    package: &str,
    version: Option<&str>,
    options: &PythonScanOptions,
) -> Result<(String, Vec<String>, String, Vec<String>), PlanError> {
    if package.len() > 64 {
        return Err(PlanError::Unsupported);
    }
    let impact =
        "安装可能下载、构建、写缓存和管理器元数据；本轮没有执行这些步骤，不能保证回滚".to_owned();
    if provider == ProviderId::Brew {
        let formula = native_brew_python_formula(package).ok_or(PlanError::Unsupported)?;
        if version.is_some() {
            return Err(PlanError::Unsupported);
        }
        return Ok((
            formula.clone(),
            vec![
                "install".into(),
                "--formula".into(),
                format!("homebrew/core/{formula}"),
            ],
            "仅预览 core formula；版本由执行时渠道决定，不承诺精确补丁版".into(),
            vec![
                impact,
                "Homebrew 可能改变依赖、链接和其他 keg；真实影响门禁尚未实现".into(),
            ],
        ));
    }
    if package != "python" {
        return Err(PlanError::Unsupported);
    }
    let version = version
        .filter(|v| crate::providers::python::stable(v))
        .ok_or(PlanError::Unsupported)?;
    let argv = match provider {
        ProviderId::Mise => vec!["install".into(), format!("core:python@{version}")],
        ProviderId::Pyenv => vec!["install".into(), version.into()],
        ProviderId::Uv => python_uv::argv(Action::Install, version, &options.uv_python_dir)?,
        _ => return Err(PlanError::Unsupported),
    };
    let detail = match provider {
        ProviderId::Mise => "core backend；可下载或编译，构建类型和产物仍需核验",
        ProviderId::Pyenv => "python-build 及插件可参与构建或 rehash；没有验证其副作用",
        ProviderId::Uv => "--no-bin 不写 bin；别名重指向、缓存和产物由执行计划重验",
        _ => return Err(PlanError::Unsupported),
    };
    Ok((
        "python".into(),
        argv,
        format!("请求稳定 CPython 精确版本 {version}；仅预览，尚未验证渠道产物"),
        vec![impact, detail.into()],
    ))
}
impl OperationPlanService {
    pub fn preview_python_current(
        &self,
        query: &InventoryQueryService,
        request: PlanRequest,
    ) -> Result<OperationPlan, PlanError> {
        let options =
            PythonScanOptions::from_environment().map_err(|_| PlanError::EvidenceUnavailable)?;
        let config =
            crate::env_center::global_config_path().ok_or(PlanError::EvidenceUnavailable)?;
        self.preview_python(query, request, options, config)
    }
    pub fn preview_python(
        &self,
        query: &InventoryQueryService,
        request: PlanRequest,
        options: PythonScanOptions,
        config_path: PathBuf,
    ) -> Result<OperationPlan, PlanError> {
        let (generation, snapshot) = query
            .current_snapshot_for(InventoryTool::Python)
            .map_err(|_| PlanError::Inventory)?;
        if request.snapshot_request_id != snapshot.request_id {
            return Err(PlanError::Stale);
        }
        let (configuration, config_stamp) = configuration(&config_path)?;
        let requested_action = request_action(&request.intent);
        let requested_version = match &request.intent {
            PlanIntent::Install { version, .. } => version.clone(),
            _ => None,
        };
        let mut steps = vec![];
        let mut impact = vec![];
        let (action, target, package, mut promise, mut capability) = match request.intent {
            PlanIntent::Install {
                provider_id,
                package_id,
                version,
            } => {
                let (package, argv, promise, effects) =
                    install(provider_id, &package_id, version.as_deref(), &options)?;
                let program = guards::executable(
                    &options.common,
                    match provider_id {
                        ProviderId::Mise => "mise",
                        ProviderId::Brew => "brew",
                        ProviderId::Pyenv => "pyenv",
                        ProviderId::Uv => "uv",
                        _ => return Err(PlanError::Unsupported),
                    },
                );
                let covered = snapshot.providers.iter().any(|p| {
                    p.provider_id == provider_id
                        && has_complete_coverage(p, &ToolId::from("python"))
                });
                let can_preview = covered && program.is_some();
                let reason = if !covered {
                    "来源尚未完整读取；不能依据未知状态规划安装"
                } else if program.is_none() {
                    "未找到可定位的管理器；目录观测不代表可调用渠道"
                } else {
                    "仅提供安装计划预览；渠道输出、版本能力和副作用验证尚未完成，执行未开放"
                };
                if can_preview {
                    let mut command = step(
                        StepKind::Command,
                        "安装步骤示意（本轮禁止执行）",
                        &options.common.home,
                    );
                    command.program = program.map(|p| p.to_string_lossy().into());
                    command.argv = argv;
                    steps = vec![
                        step(
                            StepKind::Preflight,
                            "待接入渠道能力与副作用门禁",
                            &options.common.home,
                        ),
                        command,
                        step(
                            StepKind::Refresh,
                            "执行能力开放后须重扫实际产物",
                            &options.common.home,
                        ),
                        step(
                            StepKind::Verify,
                            "待核对精确 CPython 构建与来源身份",
                            &options.common.home,
                        ),
                    ];
                }
                impact = effects;
                impact.push(format!(
                    "预览来源根目录：{}",
                    match provider_id {
                        ProviderId::Mise =>
                            options.common.mise_data_dir.to_string_lossy().into_owned(),
                        ProviderId::Pyenv => options.pyenv_root.to_string_lossy().into_owned(),
                        ProviderId::Uv => options.uv_python_dir.to_string_lossy().into_owned(),
                        _ => options
                            .common
                            .brew_cellars
                            .iter()
                            .map(|p| p.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("、"),
                    }
                ));
                (
                    Action::Install,
                    None,
                    ProviderPackage {
                        provider_id,
                        package_id: package,
                        kind: if provider_id == ProviderId::Brew {
                            PackageKind::Formula
                        } else {
                            PackageKind::Runtime
                        },
                        platform: options.common.platform,
                    },
                    promise,
                    Capability {
                        can_preview,
                        can_execute: false,
                        reason: Some(reason.into()),
                    },
                )
            }
            PlanIntent::Verify { installation_id }
            | PlanIntent::Uninstall { installation_id }
            | PlanIntent::Bind {
                installation_id, ..
            } => {
                let target = snapshot
                    .installations
                    .iter()
                    .find(|i| {
                        i.installation_id == installation_id && i.tool_id == ToolId::from("python")
                    })
                    .cloned()
                    .ok_or(PlanError::Stale)?;
                // Verify 复用共享执行器；Install/Uninstall 只有写渠道准备成功后才开放执行。
                let action = match requested_action {
                    Some(a) => a,
                    None => unreachable!(),
                };
                let ready = action != Action::Bind && target_ready(&snapshot, &target);
                let reason = if action == Action::Bind {
                    Some("Python 绑定、虚拟环境及项目作用域尚未接入；不能转交 Node 执行器".into())
                } else if !ready {
                    Some("此实例尚无当前健康的隔离探针证据；未知 PATH 文件、过期及异常记录不会自动执行".into())
                } else {
                    None
                };
                let enabled = ready;
                if enabled {
                    if action == Action::Uninstall {
                        steps = vec![
                            step(
                                StepKind::Preflight,
                                "核对来源根目录、配置及目标文件指纹",
                                &options.common.home,
                            ),
                            step(
                                StepKind::Command,
                                "移除原生管理器确认的精确 CPython 实例",
                                &options.common.home,
                            ),
                            step(
                                StepKind::Refresh,
                                "命令成功、失败或取消后均重扫 Python 来源",
                                &options.common.home,
                            ),
                            step(
                                StepKind::Verify,
                                "核对原生记录、精确实例与 CPython 隔离探针",
                                &options.common.home,
                            ),
                        ];
                    } else {
                        steps = vec![
                            step(
                                StepKind::Preflight,
                                "核对 Python 来源根目录、配置及目标文件指纹",
                                &options.common.home,
                            ),
                            step(
                                StepKind::Refresh,
                                "使用固定隔离探针重扫该 Python 来源",
                                &options.common.home,
                            ),
                            step(
                                StepKind::Verify,
                                "只确认所选实例身份、版本、架构与归属",
                                &options.common.home,
                            ),
                        ];
                    }
                }
                impact.push(
                    "仅作用于所选实例；同来源其他记录不完整时，不据此宣称来源完整或获得额外写权限"
                        .into(),
                );
                let promise = if action == Action::Uninstall {
                    format!(
                        "移除精确 CPython {}；不自动改写项目、虚拟环境、终端或 shell 引用",
                        target.version.as_deref().unwrap_or("未知")
                    )
                } else {
                    format!(
                        "重新确认精确 CPython {}；不证明项目、虚拟环境、终端或 IDE 生效",
                        target.version.as_deref().unwrap_or("未知")
                    )
                };
                (
                    action,
                    Some(target.clone()),
                    target.provider_package.clone(),
                    promise,
                    Capability {
                        can_preview: enabled,
                        can_execute: action == Action::Verify && enabled,
                        reason,
                    },
                )
            }
            PlanIntent::Unbind { .. } => return Err(PlanError::Unsupported),
        };
        let mut python_write = None;
        if matches!(action, Action::Install | Action::Uninstall) {
            let prepared = match package.provider_id {
                ProviderId::Mise => python_mise::prepare(
                    action,
                    &package,
                    target.as_ref(),
                    requested_version.as_deref(),
                    &snapshot,
                    &options,
                )
                .map(WriteSpec::Mise),
                ProviderId::Uv => python_uv::prepare(
                    action,
                    target.as_ref(),
                    requested_version.as_deref(),
                    &snapshot,
                    &options,
                )
                .map(WriteSpec::Uv),
                _ => Err(PlanError::Unsupported),
            };
            match prepared {
                Ok(spec) => {
                    capability = Capability {
                        can_preview: true,
                        can_execute: true,
                        reason: None,
                    };
                    let mut command = step(
                        StepKind::Command,
                        if action == Action::Install {
                            "安装精确 CPython；不自动激活"
                        } else {
                            "移除原生管理器确认的精确 CPython 实例"
                        },
                        &options.common.home,
                    );
                    command.program = Some(spec.program().to_string_lossy().into());
                    command.argv = spec.argv().to_vec();
                    steps = vec![
                        step(
                            StepKind::Preflight,
                            if package.provider_id == ProviderId::Mise {
                                "重验 core:python 收据、安装记录与受控设置"
                            } else {
                                "重验 uv 能力、安装目录与次版本别名"
                            },
                            &options.common.home,
                        ),
                        command,
                        step(
                            StepKind::Refresh,
                            "命令成功、失败或取消后均重扫 Python 来源",
                            &options.common.home,
                        ),
                        step(
                            StepKind::Verify,
                            "核对原生记录、精确实例与 CPython 隔离探针",
                            &options.common.home,
                        ),
                    ];
                    let keys = spec.environment_keys();
                    for step in &mut steps {
                        step.environment_keys = keys.clone();
                    }
                    let channel = if package.provider_id == ProviderId::Mise {
                        "精确预编译"
                    } else {
                        "uv 托管精确"
                    };
                    promise = format!(
                        "{}{channel} CPython {}；原生记录与实例结果须独立核对，不改写版本请求，实际解析需另行核对",
                        if action == Action::Install { "安装" } else { "卸载" },
                        spec.version()
                    );
                    impact = spec.impact().to_vec();
                    python_write = Some(spec);
                }
                Err(error) => {
                    capability.can_execute = false;
                    capability.reason = Some(error.to_string());
                }
            }
        }
        if options.common.platform != Platform::Macos
            || !cfg!(unix)
            || !options.common.home.is_absolute()
            || !options.pyenv_root.is_absolute()
            || !options.uv_python_dir.is_absolute()
        {
            capability = Capability {
                can_preview: false,
                can_execute: false,
                reason: Some("Python 计划仅支持 macOS 与绝对来源路径".into()),
            };
            steps.clear();
        }
        let mut files = vec![];
        if capability.can_execute && action != Action::Install {
            let item = target.as_ref().ok_or(PlanError::Unsupported)?;
            for path in [&item.executable_path, &item.canonical_executable_path]
                .into_iter()
                .flatten()
            {
                let path = PathBuf::from(path);
                files.push((path.clone(), file_stamp(&path)?));
            }
            if files.len() != 2 {
                return Err(PlanError::EvidenceUnavailable);
            }
        }
        for path in steps.iter().filter_map(|s| s.program.as_ref()) {
            let path = PathBuf::from(path);
            files.push((path.clone(), file_stamp(&path)?));
        }
        let stored_context = context(&options, &config_path);
        self.store(StoredPlan {
            view: OperationPlan {
                schema_version: 1,
                tool_id: InventoryTool::Python,
                plan_id: String::new(),
                created_at: 0,
                expires_at: 0,
                snapshot_generation: generation.to_string(),
                snapshot_request_id: snapshot.request_id,
                action,
                target: PlanTarget {
                    provider_package: package,
                    installation_id: target.as_ref().map(|i| i.installation_id.clone()),
                    locator: target.as_ref().map(|i| i.locator.clone()),
                },
                capability,
                version_promise: promise,
                configuration,
                configuration_change: "不修改 Python 版本请求、项目配置、虚拟环境或 shell；安装目录与管理器副作用见计划"
                    .into(),
                permission: "当前用户；不提权".into(),
                rollback: if python_write.is_some() {"原生下载、缓存、状态与部分安装可能保留；失败或取消后重扫，不自动重装或删除残留"} else {"重新验证只更新清单与步骤；其他渠道安装执行尚未开放"}.into(),
                steps,
                impact,
                configuration_diff: vec![],
            },
            started: Instant::now(),
            consumed: false,
            cancel_requested: false,
            generation,
            target,
            context: stored_context,
            config_path,
            config_stamp,
            files,
            write: None,
            binding: None,
            python: Some(options),
            python_write,
        })
    }
    pub(super) fn execute_python_with_context(
        &self,
        plan_id: &str,
        query: InventoryQueryService,
        mut context: impl FnMut() -> Result<(PythonScanOptions, PathBuf), PlanError>,
    ) -> Result<ExecutionRecord, PlanError> {
        let mut refresh_sequence = 0;
        let mut mutation: Option<crate::inventory::query::QueryLease> = None;
        self.execute_with_scope(
            plan_id,
            &query,
            || {
                let (options, config) = context()?;
                Ok((options.common.clone(), config, Some(options)))
            },
            |kind, stored, _, id| {
                if stored.view.tool_id != InventoryTool::Python
                    || !matches!(
                        stored.view.action,
                        Action::Verify | Action::Install | Action::Uninstall
                    )
                    || stored.write.is_some()
                    || stored.binding.is_some()
                {
                    return Err(PlanError::Disabled);
                }
                let options = stored.python.as_ref().ok_or(PlanError::Preconditions)?;
                if stored.view.action != Action::Verify && stored.python_write.is_none() {
                    return Err(PlanError::Disabled);
                }
                match kind {
                    StepKind::Preflight => {
                        let token = CancellationToken::default();
                        let watch = CancelWatch::start(id, token.clone());
                        let scan = scan_python_provider(
                            stored.view.target.provider_package.provider_id,
                            options,
                            &token,
                        );
                        drop(watch);
                        if operations::cancelled(id) {
                            return Err(PlanError::Cancelled);
                        }
                        let fresh =
                            merge_inventory("python-plan-preflight".into(), now(), vec![scan])
                                .map_err(|_| PlanError::Preconditions)?;
                        if let Some(write) = &stored.python_write {
                            write.revalidate(stored.target.as_ref(), &fresh, options)
                        } else {
                            verify(
                                &fresh,
                                stored.target.as_ref().ok_or(PlanError::Unsupported)?,
                            )
                            .map_err(|_| PlanError::Preconditions)
                        }
                    }
                    StepKind::Command => {
                        let write = stored.python_write.as_ref().ok_or(PlanError::Disabled)?;
                        let (_, snapshot) = query
                            .current_snapshot_for(InventoryTool::Python)
                            .map_err(|_| PlanError::Inventory)?;
                        write.revalidate(stored.target.as_ref(), &snapshot, options)?;
                        let lease = query
                            .begin_tool(
                                format!("python-mutation-{id}"),
                                Some(stored.view.target.provider_package.provider_id),
                                InventoryTool::Python,
                            )
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
                        refresh_sequence += 1;
                        let request = format!("python-verify-{id}-{refresh_sequence}");
                        let provider = stored.view.target.provider_package.provider_id;
                        let lease = match mutation.take() {
                            Some(lease) => lease,
                            None => query
                                .begin_tool(request.clone(), Some(provider), InventoryTool::Python)
                                .map_err(|_| PlanError::Inventory)?,
                        };
                        let result = if stored.python_write.is_some() {
                            if stored.view.target.provider_package.provider_id == ProviderId::Mise {
                                crate::syscache::invalidate("mise");
                            }
                            operations::without_cancellation(|| {
                                lease.execute_python(options.clone())
                            })
                        } else {
                            let cancel_query = query.clone();
                            let watch = CancelWatch::with_action(id, move || {
                                let _ = cancel_query.cancel(&request);
                            });
                            let result = lease.execute_python(options.clone());
                            drop(watch);
                            result
                        };
                        if stored.python_write.is_none() && operations::cancelled(id) {
                            return Err(PlanError::Cancelled);
                        }
                        result.map_err(|_| PlanError::ScanIncomplete)?;
                        Ok(())
                    }
                    StepKind::Verify => {
                        let (_, snapshot) = query
                            .current_snapshot_for(InventoryTool::Python)
                            .map_err(|_| PlanError::Inventory)?;
                        if let Some(write) = &stored.python_write {
                            write.verify(&snapshot, options)
                        } else {
                            verify(
                                &snapshot,
                                stored.target.as_ref().ok_or(PlanError::Unsupported)?,
                            )
                        }
                    }
                    _ => Err(PlanError::Disabled),
                }
            },
        )
    }
}
fn request_action(intent: &PlanIntent) -> Option<Action> {
    match intent {
        PlanIntent::Verify { .. } => Some(Action::Verify),
        PlanIntent::Uninstall { .. } => Some(Action::Uninstall),
        PlanIntent::Bind { .. } => Some(Action::Bind),
        _ => None,
    }
}
