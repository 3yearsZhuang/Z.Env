//! 只读真实主机验收：仅临时 journal；不调用 Python 安装渠道或修改版本配置。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::{InventoryQueryService, InventoryTool},
    operation_plan::{OperationPlanService, PlanIntent, PlanRequest},
    providers::{python::PythonScanOptions, NodeScanOptions},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = InventoryQueryService::default();
    let dir = tempfile::tempdir()?;
    let journal_path = dir.path().join("operations-v1.json");
    let service = OperationPlanService::new(journal_path.clone());
    service.recover()?;
    let node = query
        .begin("python-plan-node-baseline".into(), None)?
        .execute(NodeScanOptions::from_environment()?)?;
    let node_target = node.installations.iter().find(|i| {
        i.health == Health::Healthy && i.provider_package.provider_id != ProviderId::Path
    });
    let node_plan = node_target
        .map(|i| {
            service.preview_current(
                &query,
                PlanRequest {
                    snapshot_request_id: node.request_id.clone(),
                    intent: PlanIntent::Verify {
                        installation_id: i.installation_id.clone(),
                    },
                },
            )
        })
        .transpose()?;
    let python = query
        .begin_tool("python-plan-native".into(), None, InventoryTool::Python)?
        .execute_python(PythonScanOptions::from_environment()?)?;
    let target = python
        .installations
        .iter()
        .find(|i| {
            i.health == Health::Healthy
                && !i.stale
                && i.provider_package.provider_id != ProviderId::Path
        })
        .ok_or("没有可验证的 Python 实例")?;
    let plan = service.preview_python_current(
        &query,
        PlanRequest {
            snapshot_request_id: python.request_id.clone(),
            intent: PlanIntent::Verify {
                installation_id: target.installation_id.clone(),
            },
        },
    )?;
    if !plan.capability.can_execute {
        return Err("当前 Python 实例不可重新验证".into());
    }
    let no_journal_during_preview = !journal_path.exists();
    let result = service.execute(plan.plan_id.clone(), query.clone())?;
    let node_result = node_plan
        .filter(|p| p.capability.can_execute)
        .map(|p| service.execute(p.plan_id, query.clone()))
        .transpose()?;
    let journal: serde_json::Value = serde_json::from_slice(&std::fs::read(journal_path)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"plan":plan,"result":result,"nodeBaselinePlanStillValid":node_result.is_some(),"nodeResult":node_result,"noJournalDuringPreview":no_journal_during_preview,"journal":journal})
        )?
    );
    Ok(())
}
