//! 开发用只读清单采集；输出含本机路径，只保存到本地证据目录。

use std::time::{SystemTime, UNIX_EPOCH};

use mise_gui_lib::{
    inventory::merge_inventory,
    providers::{scan_node_sources, CancellationToken, NodeScanOptions},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = NodeScanOptions::from_environment()?;
    let scans = scan_node_sources(&options, &CancellationToken::default());
    let observed_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let snapshot = merge_inventory("node-inventory-example".into(), observed_at, scans)?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}
