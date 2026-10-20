//! 显式可丢弃目录内的真实 uv 托管 CPython 安装/重扫/卸载，不访问正式 journal。
//! 运行前设置 HOME、XDG_CONFIG_HOME、UV_PYTHON_INSTALL_DIR 与本 helper 的验收根一致。
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
        .filter(|path| path.is_absolute())
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
                .any(|component| matches!(component, std::path::Component::ParentDir))
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
    std::fs::create_dir_all(config.parent().ok_or("配置父目录无效")?)?;
    let query = InventoryQueryService::default();
    let before = query
        .begin_tool("python-uv-before".into(), None, InventoryTool::Python)?
        .execute_python(options.clone())?;
    let service = OperationPlanService::new(root.join("journal/operations-v1.json"));
    service.recover()?;
    let plan = service.preview_python(
        &query,
        PlanRequest {
            snapshot_request_id: before.request_id,
            intent: PlanIntent::Install {
                provider_id: ProviderId::Uv,
                package_id: "python".into(),
                version: Some("3.12.13".into()),
            },
        },
        options.clone(),
        config.clone(),
    )?;
    if !plan.capability.can_execute {
        return Err(format!("安装不可执行：{:?}", plan.capability.reason).into());
    }
    // execute() 自己读取 HOME/XDG 下的环境；验收显式把它们指向可丢弃根目录。
    let installed = service.execute(plan.plan_id.clone(), query.clone())?;
    if installed.outcome != Some(Outcome::Installed) {
        return Err("安装未确认".into());
    }
    let after = query
        .begin_tool(
            "python-uv-after".into(),
            Some(ProviderId::Uv),
            InventoryTool::Python,
        )?
        .execute_python(options.clone())?;
    let item = after
        .installations
        .iter()
        .find(|item| {
            item.provider_package.provider_id == ProviderId::Uv
                && item.version.as_deref() == Some("3.12.13")
                && item.health == Health::Healthy
                && Path::new(&item.locator).starts_with(&options.uv_python_dir)
        })
        .ok_or("没有健康的精确 uv 安装产物")?;
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
    let locator = PathBuf::from(&item.locator);
    let removed = service.execute(remove.plan_id.clone(), query)?;
    if removed.outcome != Some(Outcome::Uninstalled) {
        return Err("卸载未确认".into());
    }
    if locator.exists() {
        return Err("卸载后目标仍在".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"installPlan":plan,"installResult":installed,"installedSnapshot":after,"uninstallPlan":remove,"uninstallResult":removed,"journal":service.records().0,"uvPythonDir":options.uv_python_dir})
        )?
    );
    Ok(())
}
