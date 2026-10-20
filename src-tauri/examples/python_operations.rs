//! 显式可丢弃目录内的真实 core:python 安装/重扫/卸载，不访问正式 journal。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::{InventoryQueryService, InventoryTool},
    operation_plan::{OperationPlanService, Outcome, PlanIntent, PlanRequest},
    providers::python::PythonScanOptions,
};
use std::path::{Path, PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var_os("ZENV_ACCEPTANCE_ROOT")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or("需要显式可丢弃验收根目录")?;
    let canonical = root.canonicalize()?;
    let options = PythonScanOptions::from_environment()?;
    for path in [
        &options.common.home,
        &options.common.mise_data_dir,
        &options.pyenv_root,
        &options.uv_python_dir,
    ] {
        if !path.is_absolute()
            || !path.starts_with(&root)
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("验收范围不匹配".into());
        }
        let mut parent = path.as_path();
        while !parent.exists() {
            parent = parent.parent().ok_or("验收父目录无效")?;
        }
        if !parent.canonicalize()?.starts_with(&canonical) {
            return Err("验收路径发生重定向".into());
        }
    }
    if !root.join("ALLOW_DISPOSABLE_OPERATIONS").is_file() {
        return Err("缺少可丢弃验收标记".into());
    }
    let config = options.common.home.join(".config/mise/config.toml");
    let query = InventoryQueryService::default();
    let before = query
        .begin_tool("python-native-before".into(), None, InventoryTool::Python)?
        .execute_python(options.clone())?;
    let service = OperationPlanService::new(root.join("journal/operations-v1.json"));
    service.recover()?;
    let plan = service.preview_python(
        &query,
        PlanRequest {
            snapshot_request_id: before.request_id,
            intent: PlanIntent::Install {
                provider_id: ProviderId::Mise,
                package_id: "python".into(),
                version: Some("3.12.12".into()),
            },
        },
        options.clone(),
        config.clone(),
    )?;
    if !plan.capability.can_execute {
        return Err(format!("安装不可执行：{:?}", plan.capability.reason).into());
    }
    // execute() 自己捕获环境和全局配置路径；验收显式设置 XDG_CONFIG_HOME 与 HOME 对应。
    let installed = service.execute(plan.plan_id.clone(), query.clone())?;
    if installed.outcome != Some(Outcome::Installed) {
        return Err("安装未确认".into());
    }
    let after = query
        .begin_tool(
            "python-native-after".into(),
            Some(ProviderId::Mise),
            InventoryTool::Python,
        )?
        .execute_python(options.clone())?;
    let item = after
        .installations
        .iter()
        .find(|i| {
            i.provider_package.provider_id == ProviderId::Mise
                && i.version.as_deref() == Some("3.12.12")
                && i.health == Health::Healthy
                && Path::new(&i.locator)
                    == options.common.mise_data_dir.join("installs/python/3.12.12")
        })
        .ok_or("没有健康的精确安装产物")?;
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
        return Err(format!("卸载不可执行：{:?}", remove.capability.reason).into());
    }
    let removed = service.execute(remove.plan_id.clone(), query)?;
    if removed.outcome != Some(Outcome::Uninstalled) {
        return Err("卸载未确认".into());
    }
    if options
        .common
        .mise_data_dir
        .join("installs/python/3.12.12")
        .exists()
    {
        return Err("卸载后目标仍在".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"installPlan":plan,"installResult":installed,"installedSnapshot":after,"uninstallPlan":remove,"uninstallResult":removed,"journal":service.records().0})
        )?
    );
    Ok(())
}
