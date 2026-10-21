//! 显式可丢弃目录内的真实 pyenv 源码编译安装/卸载；不访问正式 journal。
//! 运行前设置 HOME、PYENV_ROOT、PATH 指向可丢弃根目录，并创建 ALLOW_DISPOSABLE_PYENV。
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
    if !root.join("ALLOW_DISPOSABLE_PYENV").is_file() {
        return Err("缺少可丢弃 pyenv 验收标记".into());
    }
    let options = PythonScanOptions::from_environment()?;
    if !options.pyenv_root.starts_with(&root) || !options.common.home.starts_with(&root) {
        return Err("验收路径必须在可丢弃根目录内".into());
    }
    let config = options.common.home.join(".config/mise/config.toml");
    std::fs::create_dir_all(config.parent().ok_or("配置父目录无效")?)?;
    let query = InventoryQueryService::default();
    let before = query
        .begin_tool("pyenv-before".into(), None, InventoryTool::Python)?
        .execute_python(options.clone())?;
    let service = OperationPlanService::new(root.join("journal/operations-v1.json"));
    service.recover()?;
    let plan = service.preview_python(
        &query,
        PlanRequest {
            snapshot_request_id: before.request_id,
            intent: PlanIntent::Install {
                provider_id: ProviderId::Pyenv,
                package_id: "python".into(),
                version: Some("3.12.13".into()),
            },
        },
        options.clone(),
        config.clone(),
    )?;
    if !plan.capability.can_execute {
        return Err(format!("pyenv 安装不可执行：{:?}", plan.capability.reason).into());
    }
    let installed = service.execute(plan.plan_id.clone(), query.clone())?;
    if installed.outcome != Some(Outcome::Installed) {
        return Err("pyenv 安装未确认".into());
    }
    let after = query
        .begin_tool(
            "pyenv-after".into(),
            Some(ProviderId::Pyenv),
            InventoryTool::Python,
        )?
        .execute_python(options.clone())?;
    let item = after
        .installations
        .iter()
        .find(|item| {
            item.provider_package.provider_id == ProviderId::Pyenv
                && item.version.as_deref() == Some("3.12.13")
                && item.health == Health::Healthy
        })
        .ok_or("没有健康的 pyenv 精确安装产物")?
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
        return Err(format!("pyenv 卸载不可执行：{:?}", remove.capability.reason).into());
    }
    let removed = service.execute(remove.plan_id.clone(), query)?;
    if removed.outcome != Some(Outcome::Uninstalled) {
        return Err("pyenv 卸载未确认".into());
    }
    if options.pyenv_root.join("versions/3.12.13").exists() {
        return Err("卸载后版本目录仍在".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "installPlan": plan,
            "installResult": installed,
            "installedSnapshot": after,
            "uninstallPlan": remove,
            "uninstallResult": removed,
            "journal": service.records().0,
        }))?
    );
    Ok(())
}
