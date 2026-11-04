//! 项目配方预览：把预设工具、项目声明与当前清单汇总为只读逐项报告。
//! 本模块不写入、不安装、不改变项目文件；执行仍走各渠道的独立操作计划。
use crate::{
    domain::{Health, InventorySnapshot, ProviderId},
    inventory::query::InventoryQueryService,
    operation_plan::{OperationPlanService, PlanIntent, PlanRequest},
    preset::PresetTool,
    project_environment::{PythonProjectEnvironment, RequirementState},
    providers::python::PythonScanOptions,
};
use serde::Serialize;

/// 配方中单项的处置：可直接复用、需安装、或当前不支持。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipeDisposition {
    /// 清单中已有该精确版本的健康实例，可直接使用。
    Reuse,
    /// 可通过受控计划安装。
    Install,
    /// 项目声明与清单冲突。
    Conflict,
    /// 渠道、版本语法或工具尚未接入。
    Unsupported,
    /// 证据不足，不能判断。
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeStep {
    pub tool: String,
    pub request: Option<String>,
    pub disposition: RecipeDisposition,
    pub channel: Option<String>,
    pub installation_id: Option<String>,
    pub version: Option<String>,
    pub reason: String,
    /// 只读计划预览摘要；不执行、不消费计划。
    pub plan_ready: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeReport {
    pub schema_version: u32,
    pub snapshot_request_id: String,
    pub steps: Vec<RecipeStep>,
    /// 仅当所有步骤都可复用或已具备可执行计划时为 true。
    pub executable: bool,
    pub issues: Vec<String>,
}

const SUPPORTED: [&str; 2] = ["node", "python"];

fn channel(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Mise => "mise",
        ProviderId::Brew => "homebrew",
        ProviderId::Pyenv => "pyenv",
        ProviderId::Uv => "uv",
        ProviderId::Nvm => "nvm",
        ProviderId::Path => "path",
    }
}

fn exact_version(tool: &str, request: &str) -> bool {
    if tool == "python" {
        return crate::providers::python::stable(request);
    }
    if tool != "node" {
        return false;
    }
    let parts: Vec<_> = request.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn python_requirement(report: &PythonProjectEnvironment) -> Option<&str> {
    report
        .declarations
        .iter()
        .find_map(|declaration| declaration.requirement.as_deref())
}

/// 生成只读配方预览；不消费计划、不写入任何文件。
pub fn preview(
    tools: &[PresetTool],
    snapshot: &InventorySnapshot,
    python: Option<&PythonProjectEnvironment>,
    options: &PythonScanOptions,
    service: &OperationPlanService,
    query: &InventoryQueryService,
) -> RecipeReport {
    let mut steps = Vec::with_capacity(tools.len());
    let mut issues = Vec::new();
    for tool in tools {
        let mut step = RecipeStep {
            tool: tool.name.clone(),
            request: Some(tool.version.clone()),
            disposition: RecipeDisposition::Unsupported,
            channel: None,
            installation_id: None,
            version: None,
            reason: String::new(),
            plan_ready: false,
        };
        if !SUPPORTED.contains(&tool.name.as_str()) {
            step.reason = "只读配方当前仅覆盖 Node 与 Python".into();
            issues.push(format!("{}：配方暂不支持", tool.name));
            steps.push(step);
            continue;
        }
        if let Some(item) = snapshot.installations.iter().find(|item| {
            item.tool_id.0 == tool.name
                && item.health == Health::Healthy
                && !item.stale
                && item.version.as_deref() == Some(tool.version.as_str())
        }) {
            step.disposition = RecipeDisposition::Reuse;
            step.channel = Some(channel(item.provider_package.provider_id).into());
            step.installation_id = Some(item.installation_id.0.clone());
            step.version = item.version.clone();
            step.reason = "清单中已有该精确版本的健康实例".into();
            steps.push(step);
            continue;
        }
        if tool.name == "python" {
            if let Some(report) = python {
                if report.requirement_state == RequirementState::Conflict {
                    step.disposition = RecipeDisposition::Conflict;
                    step.reason = python_requirement(report)
                        .map(|requirement| format!("项目声明 {requirement} 与清单候选冲突"))
                        .unwrap_or_else(|| "项目声明与清单候选冲突".into());
                    issues.push(format!("{}：项目声明冲突", tool.name));
                    steps.push(step);
                    continue;
                }
            }
        }
        if !exact_version(&tool.name, &tool.version) {
            step.disposition = RecipeDisposition::Unknown;
            step.reason = "配方只接受精确 3 段版本；范围或别名需人工确认".into();
            issues.push(format!("{}：{} 无法只读判断", tool.name, tool.version));
            steps.push(step);
            continue;
        }
        if tool.name != "python" {
            step.reason = "Node 的配方安装计划尚未接入统一计划服务".into();
            issues.push(format!("{}：计划渠道未接入", tool.name));
            steps.push(step);
            continue;
        }
        let request = PlanRequest {
            snapshot_request_id: snapshot.request_id.clone(),
            intent: PlanIntent::Install {
                provider_id: ProviderId::Mise,
                package_id: "python".into(),
                version: Some(tool.version.clone()),
            },
        };
        let config = options.common.home.join(".config/mise/config.toml");
        match service.preview_python(query, request, options.clone(), config) {
            Ok(plan) if plan.capability.can_execute => {
                step.disposition = RecipeDisposition::Install;
                step.channel = Some("mise".into());
                step.reason = "可通过 mise 精确预编译计划安装".into();
                step.plan_ready = true;
            }
            Ok(plan) => {
                step.reason = plan
                    .capability
                    .reason
                    .unwrap_or_else(|| "当前渠道不可执行".into());
                issues.push(format!("{}：计划不可执行", tool.name));
            }
            Err(error) => {
                step.disposition = RecipeDisposition::Unknown;
                step.reason = format!("计划预览失败：{error:?}");
                issues.push(format!("{}：无法生成计划", tool.name));
            }
        }
        steps.push(step);
    }
    let executable = steps.iter().all(|step| match step.disposition {
        RecipeDisposition::Reuse => true,
        RecipeDisposition::Install => step.plan_ready,
        _ => false,
    });
    RecipeReport {
        schema_version: 1,
        snapshot_request_id: snapshot.request_id.clone(),
        steps,
        executable,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Installation, InstallationId, Ownership, PackageKind, Platform, ProviderId,
        ProviderPackage, ToolId,
    };

    use crate::project_environment::{Issue, PythonProjectEnvironment, PythonSelection};
    use crate::providers::NodeScanOptions;

    fn options() -> PythonScanOptions {
        PythonScanOptions {
            common: NodeScanOptions {
                platform: Platform::Macos,
                home: std::path::PathBuf::from("/tmp/zenv-recipe-home"),
                mise_data_dir: std::path::PathBuf::from("/tmp/zenv-recipe-mise"),
                nvm_dir: std::path::PathBuf::from("/tmp/zenv-recipe-nvm"),
                brew_cellars: vec![],
                path_entries: vec![],
                limits: Default::default(),
            },
            pyenv_root: std::path::PathBuf::from("/tmp/zenv-recipe-pyenv"),
            uv_python_dir: std::path::PathBuf::from("/tmp/zenv-recipe-uv"),
        }
    }

    fn snapshot(items: Vec<Installation>) -> InventorySnapshot {
        InventorySnapshot {
            schema_version: 1,
            request_id: "snapshot".into(),
            observed_at: 0,
            tools: vec![],
            providers: vec![],
            installations: items,
            aliases: vec![],
        }
    }

    fn installation(
        provider: ProviderId,
        tool: &str,
        version: &str,
        health: Health,
        stale: bool,
    ) -> Installation {
        Installation {
            installation_id: InstallationId::from(format!("{tool}-{provider:?}-{version}")),
            provider_package: ProviderPackage {
                provider_id: provider,
                package_id: tool.into(),
                kind: PackageKind::Runtime,
                platform: Platform::Macos,
            },
            tool_id: ToolId::from(tool),
            locator: format!("/tmp/{tool}/{version}"),
            executable_path: None,
            canonical_executable_path: None,
            version: Some(version.into()),
            architecture: None,
            health,
            ownership: Ownership::Observed,
            evidence: vec![],
            stale,
        }
    }

    fn python_report(state: RequirementState) -> PythonProjectEnvironment {
        PythonProjectEnvironment {
            schema_version: 1,
            request_id: "python".into(),
            snapshot_request_id: "snapshot".into(),
            project_directory: "/tmp/project".into(),
            observed_at: 0,
            declarations: vec![],
            requirement_state: state,
            candidates: vec![],
            inventory_complete: true,
            selection: PythonSelection {
                kind: "none".into(),
                path: None,
                requirement: None,
                reason: None,
            },
            issues: vec![Issue {
                code: "test".into(),
                message: "test".into(),
            }],
            files: vec![],
        }
    }

    #[test]
    fn recipe_reuses_exact_healthy_installation_without_planning() {
        let service = OperationPlanService::new("/tmp/zenv-recipe-journal-unused.json".into());
        let query = InventoryQueryService::default();
        let report = preview(
            &[PresetTool {
                name: "node".into(),
                version: "22.23.3".into(),
            }],
            &snapshot(vec![installation(
                ProviderId::Mise,
                "node",
                "22.23.3",
                Health::Healthy,
                false,
            )]),
            None,
            &options(),
            &service,
            &query,
        );
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].disposition, RecipeDisposition::Reuse);
        assert_eq!(report.steps[0].channel.as_deref(), Some("mise"));
        assert!(!report.steps[0].plan_ready);
        assert!(report.executable);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn recipe_reports_conflict_unsupported_and_unknown_without_writes() {
        let service = OperationPlanService::new("/tmp/zenv-recipe-journal-unused2.json".into());
        let query = InventoryQueryService::default();
        let report = preview(
            &[
                PresetTool {
                    name: "python".into(),
                    version: "3.13.15".into(),
                },
                PresetTool {
                    name: "java".into(),
                    version: "21".into(),
                },
                PresetTool {
                    name: "node".into(),
                    version: "22".into(),
                },
            ],
            &snapshot(vec![installation(
                ProviderId::Mise,
                "python",
                "3.13.15",
                Health::Broken,
                false,
            )]),
            Some(&python_report(RequirementState::Conflict)),
            &options(),
            &service,
            &query,
        );
        assert_eq!(report.steps[0].disposition, RecipeDisposition::Conflict);
        assert_eq!(report.steps[1].disposition, RecipeDisposition::Unsupported);
        assert_eq!(report.steps[2].disposition, RecipeDisposition::Unknown);
        assert!(!report.executable);
        assert_eq!(report.issues.len(), 3);
        assert!(!std::path::Path::new("/tmp/zenv-recipe-mise").exists());
    }

    #[test]
    fn recipe_ignores_stale_or_broken_installations() {
        let service = OperationPlanService::new("/tmp/zenv-recipe-journal-unused3.json".into());
        let query = InventoryQueryService::default();
        let report = preview(
            &[PresetTool {
                name: "node".into(),
                version: "22.23.3".into(),
            }],
            &snapshot(vec![
                installation(ProviderId::Mise, "node", "22.23.3", Health::Healthy, true),
                installation(ProviderId::Nvm, "node", "22.23.3", Health::Broken, false),
            ]),
            None,
            &options(),
            &service,
            &query,
        );
        assert_ne!(report.steps[0].disposition, RecipeDisposition::Reuse);
        assert!(!report.executable);
    }
}
