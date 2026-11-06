//! 未签发的工具准备：没有批准身份、时钟、消费状态或计划缓存副作用。
use super::*;

pub(super) struct PlanDraft {
    pub tool_id: InventoryTool,
    pub snapshot_generation: String,
    pub snapshot_request_id: String,
    pub action: Action,
    pub target: PlanTarget,
    pub capability: Capability,
    pub version_promise: String,
    pub configuration: ConfigFingerprint,
    pub configuration_change: String,
    pub permission: String,
    pub rollback: String,
    pub steps: Vec<PlanStep>,
    pub impact: Vec<String>,
    pub configuration_diff: Vec<ConfigurationDiff>,
}

pub(super) struct PreparedTool {
    pub draft: PlanDraft,
    pub generation: u64,
    pub target: Option<Installation>,
    pub context: String,
    pub config_path: PathBuf,
    pub config_stamp: Option<FileStamp>,
    pub files: Vec<(PathBuf, FileStamp)>,
    pub write: Option<writes::WriteSpec>,
    pub binding: Option<bindings::BindingSpec>,
    pub python: Option<crate::providers::python::PythonScanOptions>,
    pub python_write: Option<python::WriteSpec>,
    pub python_binding: Option<python_binding::BindingSpec>,
    pub project_target: Option<project_target::FrozenProjectTarget>,
}

/// 只读摘要明确不具备可提交给 operation_execute 的身份。
pub(crate) struct PreparationSummary {
    pub snapshot_generation: String,
    pub snapshot_request_id: String,
    pub capability: Capability,
}

impl PreparedTool {
    pub(super) fn freeze_project(mut self, raw: Option<&str>) -> Result<Self, PlanError> {
        if self.draft.capability.can_execute && self.draft.action == Action::Bind {
            if let Some(raw) = raw {
                let selected = self
                    .python_binding
                    .as_ref()
                    .map(|spec| spec.config_path.clone())
                    .or_else(|| {
                        self.binding
                            .as_ref()?
                            .diff
                            .first()
                            .map(|diff| PathBuf::from(&diff.path))
                    })
                    .ok_or(PlanError::Preconditions)?;
                self.project_target = Some(project_target::FrozenProjectTarget::capture(
                    raw, &selected,
                )?);
            }
        }
        Ok(self)
    }

    /// 对冻结输入做比较；不以最新文件、配置目标或清单重新生成批准。
    pub(super) fn revalidate(
        &self,
        query: &InventoryQueryService,
        options: &NodeScanOptions,
        config_path: &Path,
        python: Option<&crate::providers::python::PythonScanOptions>,
    ) -> Result<(), PlanError> {
        let (generation, snapshot) = query
            .current_snapshot_for(self.draft.tool_id)
            .map_err(|_| PlanError::Inventory)?;
        if generation != self.generation || snapshot.request_id != self.draft.snapshot_request_id {
            return Err(PlanError::Stale);
        }
        let context = match (self.draft.tool_id, python) {
            (InventoryTool::Node, None) => guards::context(options, config_path),
            (InventoryTool::Python, Some(options)) => python::context(options, config_path),
            _ => return Err(PlanError::Preconditions),
        };
        if context != self.context || config_path != self.config_path {
            return Err(PlanError::Preconditions);
        }
        let (configuration, stamp) = configuration(config_path)?;
        if configuration != self.draft.configuration || stamp != self.config_stamp {
            return Err(PlanError::Preconditions);
        }
        for (path, expected) in &self.files {
            if file_stamp(path)? != *expected {
                return Err(PlanError::Preconditions);
            }
        }
        if let Some(project) = &self.project_target {
            project.revalidate()?;
        }
        if let Some(binding) = &self.binding {
            binding.revalidate_resources()?;
        }
        if let Some(binding) = &self.python_binding {
            binding.revalidate(self.target.as_ref(), &snapshot)?;
        }
        Ok(())
    }

    pub(super) fn into_stored(
        self,
        plan_id: String,
        created_at: u64,
        started: Instant,
    ) -> StoredPlan {
        StoredPlan {
            view: OperationPlan {
                schema_version: 1,
                tool_id: self.draft.tool_id,
                plan_id,
                created_at,
                expires_at: created_at.saturating_add(TTL_MS),
                snapshot_generation: self.draft.snapshot_generation,
                snapshot_request_id: self.draft.snapshot_request_id,
                action: self.draft.action,
                target: self.draft.target,
                capability: self.draft.capability,
                version_promise: self.draft.version_promise,
                configuration: self.draft.configuration,
                configuration_change: self.draft.configuration_change,
                permission: self.draft.permission,
                rollback: self.draft.rollback,
                steps: self.draft.steps,
                impact: self.draft.impact,
                configuration_diff: self.draft.configuration_diff,
            },
            started,
            consumed: false,
            cancel_requested: false,
            generation: self.generation,
            target: self.target,
            context: self.context,
            config_path: self.config_path,
            config_stamp: self.config_stamp,
            files: self.files,
            write: self.write,
            binding: self.binding,
            python: self.python,
            python_write: self.python_write,
            python_binding: self.python_binding,
            project_target: self.project_target,
        }
    }
}

pub(super) fn project_request(request: &PlanRequest) -> Option<&str> {
    match &request.intent {
        PlanIntent::Bind {
            binding: Some(binding),
            ..
        } if binding.scope == BindingScope::Project => binding.project_path.as_deref(),
        _ => None,
    }
}
