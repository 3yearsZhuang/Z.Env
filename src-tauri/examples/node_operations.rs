//! 开发验收：由外层脚本提供可丢弃 HOME/MISE_DATA_DIR，绝不使用正式用户目录。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::InventoryQueryService,
    operation_plan::{OperationPlanService, Outcome, PlanIntent, PlanRequest},
    providers::NodeScanOptions,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = NodeScanOptions::from_environment()?;
    let acceptance_root = std::env::var_os("ZENV_ACCEPTANCE_ROOT")
        .map(std::path::PathBuf::from)
        .ok_or("仅在显式可丢弃验收目录中运行")?;
    if !options.home.starts_with(&acceptance_root)
        || !options.mise_data_dir.starts_with(&acceptance_root)
        || !acceptance_root
            .join("ALLOW_DISPOSABLE_OPERATIONS")
            .is_file()
    {
        return Err("验收范围不匹配".into());
    }
    let query = InventoryQueryService::default();
    let initial = query
        .begin("native-install-before".into(), None)?
        .execute(options.clone())?;
    let service = OperationPlanService::new(acceptance_root.join("journal/operations-v1.json"));
    service.recover()?;
    let install = service.preview_current(
        &query,
        PlanRequest {
            snapshot_request_id: initial.request_id,
            intent: PlanIntent::Install {
                provider_id: ProviderId::Mise,
                package_id: "node".into(),
                version: Some("22.15.0".into()),
            },
        },
    )?;
    if !install.capability.can_execute {
        return Err(format!("安装未开放：{:?}", install.capability.reason).into());
    }
    let installed = service.execute(install.plan_id.clone(), query.clone())?;
    if installed.outcome != Some(Outcome::Installed) {
        return Err("安装未确认".into());
    }
    let after = query
        .begin("native-install-after".into(), Some(ProviderId::Mise))?
        .execute(options.clone())?;
    let target = after
        .installations
        .iter()
        .find(|i| {
            i.provider_package.provider_id == ProviderId::Mise
                && i.version.as_deref() == Some("22.15.0")
                && i.health == Health::Healthy
                && std::path::Path::new(&i.locator)
                    == options.mise_data_dir.join("installs/node/22.15.0")
        })
        .ok_or("未发现安装实例")?;
    let uninstall = service.preview_current(
        &query,
        PlanRequest {
            snapshot_request_id: after.request_id.clone(),
            intent: PlanIntent::Uninstall {
                installation_id: target.installation_id.clone(),
            },
        },
    )?;
    if !uninstall.capability.can_execute {
        return Err(format!("卸载未开放：{:?}", uninstall.capability.reason).into());
    }
    let removed = service.execute(uninstall.plan_id.clone(), query)?;
    if removed.outcome != Some(Outcome::Uninstalled) {
        return Err("卸载未确认".into());
    }
    if options.mise_data_dir.join("installs/node/22.15.0").exists() {
        return Err("卸载后目录仍在".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"installPlan":install,"installResult":installed,"installedSnapshot":after,"uninstallPlan":uninstall,"uninstallResult":removed,"journal":service.records().0})
        )?
    );
    Ok(())
}
