//! 只读 Python 项目核对：扫描本机 Python 来源，解析项目声明并输出候选匹配。
//! 不写配置、不执行解释器、不安装依赖；路径由 ZENV_PROJECT_PATH 显式给出。
use mise_gui_lib::{
    inventory::query::{InventoryQueryService, InventoryTool},
    project_environment::{inspect_python, PythonProjectRequest},
    providers::python::PythonScanOptions,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("ZENV_PROJECT_PATH")?;
    if !std::path::Path::new(&path).is_absolute() {
        return Err("ZENV_PROJECT_PATH 必须是绝对路径".into());
    }
    let query = InventoryQueryService::default();
    let snapshot = query
        .begin_tool("python-project-native".into(), None, InventoryTool::Python)?
        .execute_python(PythonScanOptions::from_environment()?)?;
    let report = inspect_python(
        &snapshot,
        &PythonProjectRequest {
            request_id: "python-project-native".into(),
            snapshot_request_id: snapshot.request_id.clone(),
            path,
        },
    )
    .map_err(|error| format!("项目核对失败：{error}"))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
