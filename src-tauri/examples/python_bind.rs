//! 显式可丢弃项目内的真实 Python mise 绑定：预览、写入项目配置、核验。
//! 运行前在项目目录放置 ALLOW_DISPOSABLE_BINDING；只写该项目 mise.toml 与私有 journal。
use mise_gui_lib::{
    domain::{Health, ProviderId},
    inventory::query::{InventoryQueryService, InventoryTool},
    operation_plan::{
        BindingOptions, BindingScope, BindingStrategy, OperationPlanService, PlanIntent,
        PlanRequest,
    },
    providers::python::PythonScanOptions,
};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project = std::env::var_os("ZENV_PROJECT_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("需要绝对路径 ZENV_PROJECT_PATH")?;
    if !project.is_dir() {
        return Err("项目目录不存在".into());
    }
    if !project.join("ALLOW_DISPOSABLE_BINDING").is_file() {
        return Err("缺少可丢弃绑定标记".into());
    }
    let options = PythonScanOptions::from_environment()?;
    let global = mise_gui_lib::global_config_path().ok_or("无法定位全局配置")?;
    let query = InventoryQueryService::default();
    let snapshot = query
        .begin_tool("python-bind-native".into(), None, InventoryTool::Python)?
        .execute_python(options.clone())?;
    let target = snapshot
        .installations
        .iter()
        .find(|item| {
            item.provider_package.provider_id == ProviderId::Mise
                && item.health == Health::Healthy
                && !item.stale
                && item.version.is_some()
        })
        .ok_or("没有健康的 mise Python 实例")?;
    let service = OperationPlanService::new(project.join(".zenv-binding-journal.json"));
    service.recover()?;
    let plan = service.preview_python(
        &query,
        PlanRequest {
            snapshot_request_id: snapshot.request_id.clone(),
            intent: PlanIntent::Bind {
                installation_id: target.installation_id.clone(),
                binding: Some(BindingOptions {
                    strategy: BindingStrategy::Direct,
                    scope: BindingScope::Project,
                    project_path: Some(project.to_string_lossy().into()),
                }),
            },
        },
        options.clone(),
        global,
    )?;
    if !plan.capability.can_execute {
        return Err(format!("绑定不可执行：{:?}", plan.capability.reason).into());
    }
    let record = service.execute(plan.plan_id.clone(), query.clone())?;
    let bound = std::fs::read_to_string(project.join("mise.toml"))?;
    let ledger = options.common.home.join(".zenv/python-bindings-v1.json");
    let receipts: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&ledger)?)?;
    let binding_id = receipts["records"][0]["id"]
        .as_str()
        .ok_or("绑定收据缺少 ID")?
        .to_owned();
    let current = query
        .begin_tool("python-bind-unbind".into(), None, InventoryTool::Python)?
        .execute_python(options.clone())?;
    let unbind = service.preview_python(
        &query,
        PlanRequest {
            snapshot_request_id: current.request_id,
            intent: PlanIntent::Unbind { binding_id },
        },
        options,
        mise_gui_lib::global_config_path().ok_or("无法定位全局配置")?,
    )?;
    if !unbind.capability.can_execute {
        return Err(format!("解绑不可执行：{:?}", unbind.capability.reason).into());
    }
    let unbound = service.execute(unbind.plan_id.clone(), query)?;
    let restored = std::fs::read_to_string(project.join("mise.toml"))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "plan": plan,
            "record": record,
            "boundConfiguration": bound,
            "unbindPlan": unbind,
            "unbindRecord": unbound,
            "restoredConfiguration": restored,
            "configPath": Path::new(&project).join("mise.toml"),
        }))?
    );
    Ok(())
}
