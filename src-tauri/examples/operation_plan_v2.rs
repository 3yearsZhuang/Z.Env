//! 真实只读验证：v2 签发、旧执行入口单次消费、Node/Python 范围；只写临时 journal。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::{InventoryQueryService, InventoryTool},
    operation_plan::{OperationPlanService, PlanRequestV2},
    providers::{python::PythonScanOptions, NodeScanOptions},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = InventoryQueryService::default();
    let directory = tempfile::tempdir()?;
    let journal = directory.path().join("operations-v1.json");
    let service = OperationPlanService::new(journal.clone());
    service.recover()?;
    let mut reports = Vec::new();
    for tool in [InventoryTool::Node, InventoryTool::Python] {
        let name = if tool == InventoryTool::Node {
            "node"
        } else {
            "python"
        };
        let lease = query.begin_tool(format!("v2-native-{name}"), None, tool)?;
        let snapshot = match tool {
            InventoryTool::Node => lease.execute(NodeScanOptions::from_environment()?)?,
            InventoryTool::Python => lease.execute_python(PythonScanOptions::from_environment()?)?,
        };
        let target = snapshot
            .installations
            .iter()
            .find(|instance| {
                instance.health == Health::Healthy
                    && !instance.stale
                    && instance.provider_package.provider_id != ProviderId::Path
            })
            .ok_or("没有可验证的健康实例；本 helper 不会安装工具")?;
        let request: PlanRequestV2 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 2,
            "subject": {"kind":"tool", "toolId":name},
            "request": {"snapshotRequestId": snapshot.request_id,
                "intent": {"action":"verify", "installationId":target.installation_id}}
        }))?;
        let plan = service.preview_v2_current(&query, request)?;
        if !plan.plan.capability.can_execute {
            return Err("实例重新验证能力未开放".into());
        }
        let preview_created_journal = name == "node" && journal.exists();
        if preview_created_journal {
            return Err("预览不应创建 journal".into());
        }
        let result = service.execute(plan.plan.plan_id.clone(), query.clone())?;
        let repeated = service.execute(plan.plan.plan_id.clone(), query.clone());
        if repeated.is_ok() {
            return Err("同一签发计划被重复执行".into());
        }
        reports.push(serde_json::json!({"plan":plan,"result":result,
            "repeatedExecutionRejected":true,"repeatError":repeated.unwrap_err().to_string()}));
    }
    let records: serde_json::Value = serde_json::from_slice(&std::fs::read(journal)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scope":"readOnlyNativeServiceNotPackagedIpc", "reports":reports, "journal":records
        }))?
    );
    Ok(())
}
