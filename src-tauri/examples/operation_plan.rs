//! 开发验证：临时 journal 中运行真实只读验证，不改写用户配置或用户 journal。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::InventoryQueryService,
    operation_plan::{OperationPlanService, PlanIntent, PlanRequest},
    providers::NodeScanOptions,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = InventoryQueryService::default();
    let snapshot = query
        .begin("plan-example-snapshot".into(), None)?
        .execute(NodeScanOptions::from_environment()?)?;
    let target = snapshot
        .installations
        .iter()
        .find(|i| {
            i.health == Health::Healthy
                && [ProviderId::Nvm, ProviderId::Mise].contains(&i.provider_package.provider_id)
        })
        .ok_or("没有可重新验证的受支持实例")?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("operations-v1.json");
    let service = OperationPlanService::new(path.clone());
    service.recover()?;
    let plan = service.preview_current(
        &query,
        PlanRequest {
            snapshot_request_id: snapshot.request_id,
            intent: PlanIntent::Verify {
                installation_id: target.installation_id.clone(),
            },
        },
    )?;
    if !plan.capability.can_execute {
        return Err("该实例当前不支持执行重新验证".into());
    }
    let result = service.execute(plan.plan_id.clone(), query)?;
    let journal: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"plan":plan,"result":result,"journal":journal})
        )?
    );
    Ok(())
}
