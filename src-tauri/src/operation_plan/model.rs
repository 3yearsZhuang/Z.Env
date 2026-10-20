//! 操作计划的传输契约；请求只描述意图，步骤由后端签发。
use crate::domain::{InstallationId, ProviderId, ProviderPackage};
use crate::inventory::query::InventoryTool;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRequest {
    pub snapshot_request_id: String,
    pub intent: PlanIntent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlanIntent {
    Verify {
        installation_id: InstallationId,
    },
    Install {
        provider_id: ProviderId,
        package_id: String,
        version: Option<String>,
    },
    Uninstall {
        installation_id: InstallationId,
    },
    Bind {
        installation_id: InstallationId,
        #[serde(default)]
        binding: Option<BindingOptions>,
    },
    Unbind {
        binding_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Verify,
    Install,
    Uninstall,
    Bind,
    Unbind,
}
impl Action {
    pub fn label(self) -> &'static str {
        self.label_for(InventoryTool::Node)
    }
    pub fn label_for(self, tool: InventoryTool) -> &'static str {
        if tool == InventoryTool::Python {
            return match self {
                Self::Verify => "Python 实例重新验证",
                Self::Install => "Python 安装",
                Self::Uninstall => "Python 卸载",
                Self::Bind => "Python 绑定",
                Self::Unbind => "Python 解除绑定",
            };
        }
        match self {
            Self::Verify => "Node.js 实例重新验证",
            Self::Install => "Node.js 安装",
            Self::Uninstall => "Node.js 卸载",
            Self::Bind => "Node.js 绑定",
            Self::Unbind => "Node.js 解除绑定",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub can_preview: bool,
    pub can_execute: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTarget {
    pub provider_package: ProviderPackage,
    pub installation_id: Option<InstallationId>,
    pub locator: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    Preflight,
    Command,
    Refresh,
    Verify,
    Compensate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingStrategy {
    Direct,
    Managed,
    ProjectPath,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingScope {
    Registration,
    Global,
    Project,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingOptions {
    pub strategy: BindingStrategy,
    pub scope: BindingScope,
    pub project_path: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingSummary {
    pub binding_id: String,
    pub provider_package: ProviderPackage,
    pub version: String,
    pub locator: String,
    pub strategy: BindingStrategy,
    pub scope: BindingScope,
    pub configuration_path: Option<String>,
    pub state: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationDiff {
    pub path: String,
    pub field: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub kind: StepKind,
    pub title: String,
    pub program: Option<String>,
    pub argv: Vec<String>,
    pub cwd: String,
    pub environment_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFingerprint {
    pub path: String,
    pub canonical_path: Option<String>,
    pub exists: bool,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPlan {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "InventoryTool::is_node")]
    pub tool_id: InventoryTool,
    pub plan_id: String,
    pub created_at: u64,
    pub expires_at: u64,
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
    #[serde(default)]
    pub impact: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configuration_diff: Vec<ConfigurationDiff>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionStatus {
    Running,
    Completed,
    Partial,
    Failed,
    Cancelled,
    Interrupted,
}
impl ExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionStep {
    pub kind: StepKind,
    pub status: StepStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Verified,
    Installed,
    Uninstalled,
    Bound,
    Unbound,
    CompensationConflict,
    ScanIncomplete,
    PreconditionChanged,
    Cancelled,
    StepFailed,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompensationOutcome {
    Completed,
    Conflict,
    Failed,
}

/// 持久记录严格白名单，不含路径、版本、配置、环境值或进程输出。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionRecord {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "InventoryTool::is_node")]
    pub tool_id: InventoryTool,
    pub id: String,
    pub action: Action,
    pub status: ExecutionStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub steps: Vec<ExecutionStep>,
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<CompensationOutcome>,
}
