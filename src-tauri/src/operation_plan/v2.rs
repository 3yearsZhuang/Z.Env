//! v2 仅包裹已签发的工具计划；步骤图不接受前端命令，也不创建第二套执行记录。
use super::{
    InventoryQueryService, InventoryTool, OperationPlan, OperationPlanService, PlanError,
    PlanRequest, ToolId,
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OperationSubjectV2 {
    Tool { tool_id: ToolId },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRequestV2 {
    #[serde(deserialize_with = "schema_version")]
    pub schema_version: u32,
    pub subject: OperationSubjectV2,
    pub request: PlanRequest,
}

fn schema_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version != 2 {
        return Err(serde::de::Error::custom(
            "operation plan schemaVersion must be 2",
        ));
    }
    Ok(version)
}

impl PlanRequestV2 {
    /// 在任何环境读取、管理器探测或快照查询之前验证版本和当前开放的工具范围。
    pub fn validate(&self) -> Result<InventoryTool, PlanError> {
        if self.schema_version != 2 {
            return Err(PlanError::Unsupported);
        }
        match &self.subject {
            OperationSubjectV2::Tool { tool_id } => match tool_id.0.as_str() {
                "node" => Ok(InventoryTool::Node),
                "python" => Ok(InventoryTool::Python),
                _ => Err(PlanError::Unsupported),
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepGraphEntryV2 {
    pub step_id: String,
    pub step_index: usize,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationPlanV2 {
    #[serde(deserialize_with = "schema_version")]
    pub schema_version: u32,
    pub subject: OperationSubjectV2,
    pub plan: OperationPlan,
    pub step_graph: Vec<StepGraphEntryV2>,
}

impl OperationPlanV2 {
    pub fn from_plan(plan: OperationPlan) -> Self {
        let tool_id = ToolId::from(match plan.tool_id {
            InventoryTool::Node => "node",
            InventoryTool::Python => "python",
        });
        let step_graph = (0..plan.steps.len())
            .map(|index| StepGraphEntryV2 {
                step_id: format!("{}-step-{index}", plan.plan_id),
                step_index: index,
                depends_on: if index == 0 {
                    vec![]
                } else {
                    vec![format!("{}-step-{}", plan.plan_id, index - 1)]
                },
            })
            .collect();
        Self {
            schema_version: 2,
            subject: OperationSubjectV2::Tool { tool_id },
            plan,
            step_graph,
        }
    }
}

impl OperationPlanService {
    pub fn preview_v2_current(
        &self,
        query: &InventoryQueryService,
        request: PlanRequestV2,
    ) -> Result<OperationPlanV2, PlanError> {
        self.preview_v2_using(request, |tool, request| match tool {
            InventoryTool::Node => self.preview_current(query, request),
            InventoryTool::Python => self.preview_python_current(query, request),
        })
    }

    pub(super) fn preview_v2_using(
        &self,
        request: PlanRequestV2,
        preview: impl FnOnce(InventoryTool, PlanRequest) -> Result<OperationPlan, PlanError>,
    ) -> Result<OperationPlanV2, PlanError> {
        let tool = request.validate()?;
        let plan = preview(tool, request.request)?;
        Ok(OperationPlanV2::from_plan(plan))
    }
}

#[cfg(test)]
#[path = "v2_tests.rs"]
mod tests;
