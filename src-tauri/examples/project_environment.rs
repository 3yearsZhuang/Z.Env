//! 可丢弃目录中的真实 Node 上下文验收；不写入用户配置或执行项目钩子。
use mise_gui_lib::{
    inventory::query::InventoryQueryService,
    project_environment::{inspect, EnvironmentRequest, Match, RequirementState},
    providers::NodeScanOptions,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root =
        std::path::PathBuf::from(std::env::var_os("ZENV_ACCEPTANCE_ROOT").ok_or("缺少验收根目录")?);
    let mut options = NodeScanOptions::from_environment()?;
    if !root.is_absolute()
        || !options.home.starts_with(&root)
        || !options.mise_data_dir.starts_with(&root)
        || !options.nvm_dir.starts_with(&root)
        || !root.join("ALLOW_DISPOSABLE_OPERATIONS").is_file()
    {
        return Err("只能在明确可丢弃的目录中运行".into());
    }
    options.brew_cellars = vec![root.join("Cellar")];
    let query = InventoryQueryService::default();
    let snapshot = query
        .begin("native-project-inventory".into(), None)?
        .execute(options.clone())?;
    let mut reports = Vec::new();
    for (name, probe) in [
        ("mise project", false),
        ("mise project", true),
        ("path project", true),
        ("conflict project", true),
        ("alias project", true),
    ] {
        let request = EnvironmentRequest {
            request_id: format!("native-{}-{probe}", name.replace(' ', "-")),
            snapshot_request_id: snapshot.request_id.clone(),
            path: root.join(name).to_string_lossy().into(),
            probe,
        };
        let report = inspect(
            &request,
            &snapshot,
            &options,
            &options.home.join(".config/mise/config.toml"),
            &Default::default(),
        )?;
        if !probe {
            assert_eq!(report.controlled.status, "notRequested");
        } else {
            assert_eq!(report.application_path.status, "verified");
            assert_eq!(report.application_path.version.as_deref(), Some("20.19.5"));
            assert_eq!(report.terminal.status, "unknown");
            match name {
                "mise project" => {
                    assert_eq!(report.controlled.status, "verified");
                    assert_eq!(report.controlled.version.as_deref(), Some("22.23.3"));
                    assert_eq!(report.controlled.requirement_match, Match::Satisfied);
                    assert_eq!(report.application_path.requirement_match, Match::Mismatch);
                }
                "path project" => {
                    assert_eq!(report.controlled.status, "verified");
                    assert_eq!(report.controlled.version.as_deref(), Some("20.19.5"));
                    assert_eq!(report.controlled.requirement_match, Match::Mismatch);
                }
                "conflict project" => {
                    assert_eq!(report.requirement_state, RequirementState::Conflict);
                    assert_eq!(report.controlled.status, "unknown");
                }
                "alias project" => {
                    assert_eq!(report.requirement_state, RequirementState::Unknown);
                    assert_eq!(report.controlled.status, "unknown");
                }
                _ => unreachable!(),
            }
        }
        reports.push(report);
    }
    assert!(!root.join("HOOK_MUST_NOT_RUN").exists());
    assert!(!root.join("ENV_MUST_NOT_RUN").exists());
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"snapshot":snapshot,"reports":reports}))?
    );
    Ok(())
}
