//! 仅在显式可丢弃 HOME/data 下验收绑定；原 Node 文件由外层脚本复制。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::InventoryQueryService,
    operation_plan::{
        BindingOptions, BindingScope, BindingStrategy, OperationPlanService, Outcome, PlanIntent,
        PlanRequest,
    },
    providers::NodeScanOptions,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = NodeScanOptions::from_environment()?;
    let root = std::env::var_os("ZENV_ACCEPTANCE_ROOT")
        .map(std::path::PathBuf::from)
        .ok_or("缺少验收根目录")?;
    if !root.is_absolute()
        || !options.home.starts_with(&root)
        || !options.mise_data_dir.starts_with(&root)
        || !options.nvm_dir.starts_with(&root)
        || !root.join("ALLOW_DISPOSABLE_OPERATIONS").is_file()
    {
        return Err("只能在明确可丢弃的目录中运行".into());
    }
    let query = InventoryQueryService::default();
    let service = OperationPlanService::new(root.join("journal/operations-v1.json"));
    service.recover()?;
    let mut results = vec![];
    for (index, strategy, scope) in [
        (0, BindingStrategy::Direct, BindingScope::Global),
        (1, BindingStrategy::Direct, BindingScope::Project),
        (2, BindingStrategy::ProjectPath, BindingScope::Project),
    ] {
        let before = query
            .begin(format!("binding-before-{index}"), None)?
            .execute(options.clone())?;
        let source = before
            .installations
            .iter()
            .find(|s| {
                s.provider_package.provider_id == ProviderId::Nvm && s.health == Health::Healthy
            })
            .ok_or("缺少临时 nvm 原安装")?;
        let plan = service.preview_current(
            &query,
            PlanRequest {
                snapshot_request_id: before.request_id.clone(),
                intent: PlanIntent::Bind {
                    installation_id: source.installation_id.clone(),
                    binding: Some(BindingOptions {
                        strategy,
                        scope,
                        project_path: (scope == BindingScope::Project)
                            .then(|| root.join("project with spaces").to_string_lossy().into()),
                    }),
                },
            },
        )?;
        if !plan.capability.can_execute {
            return Err(format!("绑定不支持：{:?}", plan.capability.reason).into());
        }
        let bound = service.execute(plan.plan_id.clone(), query.clone())?;
        if bound.outcome != Some(Outcome::Bound) {
            return Err("绑定未通过验证".into());
        }
        let records = service.bindings()?;
        if records.len() != 1 || records[0].state != "bound" {
            return Err("绑定收据未确认".into());
        }
        let current = query
            .begin(format!("binding-after-{index}"), None)?
            .execute(options.clone())?;
        let unbind = service.preview_current(
            &query,
            PlanRequest {
                snapshot_request_id: current.request_id,
                intent: PlanIntent::Unbind {
                    binding_id: records[0].binding_id.clone(),
                },
            },
        )?;
        let released = service.execute(unbind.plan_id.clone(), query.clone())?;
        if released.outcome != Some(Outcome::Unbound) || !service.bindings()?.is_empty() {
            return Err("解除绑定未确认".into());
        }
        if !std::path::Path::new(&source.locator)
            .join("bin/node")
            .is_file()
        {
            return Err("原安装丢失".into());
        }
        results.push(serde_json::json!({"bindPlan":plan,"bound":bound,"unbindPlan":unbind,"unbound":released}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"cases":results,"journal":service.records().0})
        )?
    );
    Ok(())
}
