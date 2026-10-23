//! 可丢弃 Homebrew 前缀内的真实 python formula 安装/卸载；不访问正式 journal。
//! 运行前设置 HOME、HOMEBREW_CELLAR、PATH 指向 /private/var/tmp/hb 验收环境。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::{InventoryQueryService, InventoryTool},
    operation_plan::{OperationPlanService, Outcome, PlanIntent, PlanRequest},
    providers::python::PythonScanOptions,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var_os("ZENV_ACCEPTANCE_ROOT")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("需要绝对路径 ZENV_ACCEPTANCE_ROOT")?;
    if !root.join("ALLOW_DISPOSABLE_BREW").is_file() {
        return Err("缺少可丢弃 Homebrew 验收标记".into());
    }
    let prefix = std::env::var_os("ZENV_BREW_ACCEPTANCE_PREFIX")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("需要绝对路径 ZENV_BREW_ACCEPTANCE_PREFIX")?;
    let options = PythonScanOptions::from_environment()?;
    if !options.common.home.starts_with(&root)
        || !options
            .common
            .brew_cellars
            .iter()
            .any(|cellar| cellar.starts_with(&prefix))
    {
        return Err("验收 HOME/Cellar 不在可丢弃范围内".into());
    }
    let config = options.common.home.join(".config/mise/config.toml");
    std::fs::create_dir_all(config.parent().ok_or("配置父目录无效")?)?;
    let query = InventoryQueryService::default();
    let before = query
        .begin_tool("brew-before".into(), None, InventoryTool::Python)?
        .execute_python(options.clone())?;
    let service = OperationPlanService::new(root.join("journal/operations-v1.json"));
    service.recover()?;
    let plan = service.preview_python(
        &query,
        PlanRequest {
            snapshot_request_id: before.request_id,
            intent: PlanIntent::Install {
                provider_id: ProviderId::Brew,
                package_id: "python@3.13".into(),
                version: None,
            },
        },
        options.clone(),
        config.clone(),
    )?;
    if !plan.capability.can_execute {
        return Err(format!("Homebrew 安装不可执行：{:?}", plan.capability.reason).into());
    }
    let installed = service.execute(plan.plan_id.clone(), query.clone())?;
    if installed.outcome != Some(Outcome::Installed) {
        return Err("Homebrew 安装未确认".into());
    }
    let after = query
        .begin_tool(
            "brew-after".into(),
            Some(ProviderId::Brew),
            InventoryTool::Python,
        )?
        .execute_python(options.clone())?;
    let item = after
        .installations
        .iter()
        .find(|item| {
            item.provider_package.provider_id == ProviderId::Brew
                && item.provider_package.package_id == "python@3.13"
                && item.health == Health::Healthy
        })
        .ok_or("没有健康的 Homebrew Python 产物")?
        .clone();
    let remove = service.preview_python(
        &query,
        PlanRequest {
            snapshot_request_id: after.request_id.clone(),
            intent: PlanIntent::Uninstall {
                installation_id: item.installation_id.clone(),
            },
        },
        options.clone(),
        config,
    )?;
    if !remove.capability.can_execute {
        return Err(format!("Homebrew 卸载不可执行：{:?}", remove.capability.reason).into());
    }
    let removed = service.execute(remove.plan_id.clone(), query)?;
    if removed.outcome != Some(Outcome::Uninstalled) {
        return Err("Homebrew 卸载未确认".into());
    }
    if options
        .common
        .brew_cellars
        .iter()
        .any(|cellar| cellar.join("python@3.13").exists())
    {
        return Err("卸载后 formula 目录仍在".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "installPlan": plan,
            "installResult": installed,
            "installedSnapshot": after,
            "uninstallPlan": remove,
            "uninstallResult": removed,
        }))?
    );
    Ok(())
}
