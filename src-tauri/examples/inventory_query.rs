//! 开发用查询服务验证：全量扫描后单独重读 PATH，输出只保存到本地证据目录。

use mise_gui_lib::{
    domain::ProviderId, inventory::query::InventoryQueryService, providers::NodeScanOptions,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = InventoryQueryService::default();
    let options = NodeScanOptions::from_environment()?;
    let full = service
        .begin("query-example-full".into(), None)?
        .execute(options.clone())?;
    let path_refresh = service
        .begin("query-example-path".into(), Some(ProviderId::Path))?
        .execute(options)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "full": full,
            "pathRefresh": path_refresh,
        }))?
    );
    Ok(())
}
