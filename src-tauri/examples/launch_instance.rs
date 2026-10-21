//! 真实只读启动示例：扫描本机 Node 实例，用原生 launcher 执行 `node --version`。
//! 不经过 shell 或 shim；只启动清单中已核验的健康实例。
use mise_gui_lib::{
    domain::Health,
    inventory::query::InventoryQueryService,
    launcher::{launch, CancellationToken, LaunchEnvironment, LaunchRequest},
    providers::NodeScanOptions,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = NodeScanOptions::from_environment()?;
    let query = InventoryQueryService::default();
    let snapshot = query
        .begin("launch-native".into(), None)?
        .execute(options.clone())?;
    let target = snapshot
        .installations
        .iter()
        .find(|item| {
            item.health == Health::Healthy
                && !item.stale
                && item.version.is_some()
                && item.executable_path.is_some()
        })
        .ok_or("没有可启动的健康 Node 实例")?;
    let result = launch(
        &snapshot,
        &LaunchRequest {
            installation_id: target.installation_id.0.clone(),
            argv: vec!["--version".into()],
            cwd: options.home.to_string_lossy().into(),
            environment: LaunchEnvironment {
                home: options.home.to_string_lossy().into(),
                path: vec![],
                vars: vec![],
            },
            timeout_ms: Some(10_000),
            max_output_bytes: Some(64 * 1024),
        },
        &CancellationToken::default(),
    )?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
