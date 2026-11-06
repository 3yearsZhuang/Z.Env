//! 配方分别绑定工具清单，核对声明，再汇总既有计划门禁；不扫描、不写入。
use crate::{
    domain::{Health, InventorySnapshot, ProviderId},
    inventory::{
        has_complete_coverage,
        query::{validate_request_id, CapturedInventories, InventoryQueryService, InventoryTool},
    },
    operation_plan::{OperationPlanService, PlanIntent, PlanRequest},
    preset::PresetTool,
    project_environment::{
        inspect, inspect_python, match_node_request, match_python_request, EnvironmentRequest,
        Match, ProjectEnvironment, PythonProjectEnvironment, PythonProjectRequest,
    },
    providers::python::PythonScanOptions,
};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipeDisposition {
    /// 已有无需重复安装的候选，不代表项目绑定或环境生效。
    Reuse,
    /// 既有渠道允许精确安装，配方本身仍不执行。
    Install,
    Conflict,
    Unsupported,
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
    /// 安装能力摘要，不包含可执行计划 ID。
    pub plan_ready: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeSnapshotBinding {
    pub tool: InventoryTool,
    pub request_id: Option<String>,
    pub generation: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeReport {
    pub schema_version: u32,
    /// 兼容字段：仅单工具且有清单时有值；混合配方使用 snapshot_bindings。
    pub snapshot_request_id: Option<String>,
    pub snapshot_bindings: Vec<RecipeSnapshotBinding>,
    pub steps: Vec<RecipeStep>,
    /// 执行编排尚未接入，始终为 false。
    pub executable: bool,
    pub issues: Vec<String>,
}

struct ProjectReports<'a> {
    node: Option<&'a ProjectEnvironment>,
    python: Option<&'a PythonProjectEnvironment>,
}

fn inventory_tool(name: &str) -> Option<InventoryTool> {
    match name {
        "node" => Some(InventoryTool::Node),
        "python" => Some(InventoryTool::Python),
        _ => None,
    }
}
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
fn exact_version(tool: InventoryTool, request: &str) -> bool {
    if tool == InventoryTool::Python {
        return crate::providers::python::stable(request);
    }
    let parts: Vec<_> = request.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 10
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}
fn complete(snapshot: &InventorySnapshot, tool: InventoryTool) -> bool {
    tool.providers().iter().all(|expected| {
        snapshot.providers.iter().any(|provider| {
            provider.provider_id == *expected && has_complete_coverage(provider, &tool.id())
        })
    })
}

/// 命令与测试共用入口。锁内捕获、锁外读取/预览，返回前重验所有相关清单。
pub fn preview_recipe(
    tools: &[PresetTool],
    project: Option<&PythonProjectRequest>,
    options: &PythonScanOptions,
    service: &OperationPlanService,
    query: &InventoryQueryService,
    config_path: &Path,
) -> Result<RecipeReport, String> {
    if tools.len() > 64 {
        return Err("配方工具数量超出预算".into());
    }
    let journal_failed_before = service.journal_failed();
    let mut related: Vec<_> = tools
        .iter()
        .filter_map(|tool| inventory_tool(&tool.name))
        .collect();
    if project.is_some() {
        related.push(InventoryTool::Python);
    }
    let captured = query
        .capture_snapshots(&related)
        .map_err(|error| error.to_string())?;
    let python = if let Some(request) = project {
        validate_request_id(&request.request_id).map_err(|error| error.to_string())?;
        validate_request_id(&request.snapshot_request_id).map_err(|error| error.to_string())?;
        if request.path.len() > 4096 {
            return Err("项目路径超出预算".into());
        }
        let (_, snapshot) = captured
            .get(&InventoryTool::Python)
            .and_then(Option::as_ref)
            .ok_or("Python 清单不可用，请先完成读取")?;
        if request.snapshot_request_id != snapshot.request_id {
            return Err("Python 清单已变化，请重新核对项目".into());
        }
        Some(inspect_python(snapshot, request).map_err(str::to_owned)?)
    } else {
        None
    };
    let node = match (
        project,
        captured.get(&InventoryTool::Node).and_then(Option::as_ref),
    ) {
        (Some(project), Some((_, snapshot))) if tools.iter().any(|tool| tool.name == "node") => {
            Some(inspect(
                &node_project_request(project, snapshot),
                snapshot,
                &options.common,
                config_path,
                &Default::default(),
            )?)
        }
        _ => None,
    };
    let report = evaluate(
        tools,
        &captured,
        ProjectReports {
            node: node.as_ref(),
            python: python.as_ref(),
        },
        options,
        service,
        query,
        config_path,
    );
    // 门禁可能需要子进程；期间项目文件也可能被外部工具改写。
    if let (Some(request), Some(before)) = (project, python.as_ref()) {
        let (_, snapshot) = captured[&InventoryTool::Python].as_ref().unwrap();
        let after = inspect_python(snapshot, request).map_err(str::to_owned)?;
        if !same_project_evidence(before, &after) {
            return Err("项目声明已变化，请重新预览配方".into());
        }
    }
    if let (Some(project), Some(before)) = (project, node.as_ref()) {
        let (_, snapshot) = captured[&InventoryTool::Node].as_ref().unwrap();
        let after = inspect(
            &node_project_request(project, snapshot),
            snapshot,
            &options.common,
            config_path,
            &Default::default(),
        )?;
        if !same_node_project_evidence(before, &after) {
            return Err("Node 项目声明或配置目标已变化，请重新预览配方".into());
        }
    }
    query
        .validate_snapshots(&captured)
        .map_err(|error| error.to_string())?;
    if service.journal_failed()
        && (!journal_failed_before || report.steps.iter().any(|step| step.plan_ready))
    {
        return Err("操作记录状态已变化，请重新预览安装能力".into());
    }
    Ok(report)
}

fn node_project_request(
    project: &PythonProjectRequest,
    snapshot: &InventorySnapshot,
) -> EnvironmentRequest {
    EnvironmentRequest {
        request_id: project.request_id.clone(),
        snapshot_request_id: snapshot.request_id.clone(),
        path: project.path.clone(),
        probe: false,
    }
}

fn same_node_project_evidence(before: &ProjectEnvironment, after: &ProjectEnvironment) -> bool {
    before.project_directory == after.project_directory
        && before.configuration_path == after.configuration_path
        && before.requirement_state == after.requirement_state
        && before.files.len() == after.files.len()
        && before.files.iter().zip(&after.files).all(|(a, b)| {
            a.path == b.path
                && a.canonical_path == b.canonical_path
                && a.exists == b.exists
                && a.sha256 == b.sha256
        })
        && before.selection.kind == after.selection.kind
        && before.selection.scope == after.selection.scope
        && before.selection.path == after.selection.path
        && before.selection.request == after.selection.request
        && before.selection.path_entries == after.selection.path_entries
        && before.selection.reason == after.selection.reason
        && before.issues.len() == after.issues.len()
        && before
            .issues
            .iter()
            .zip(&after.issues)
            .all(|(a, b)| a.code == b.code && a.message == b.message)
}

fn same_project_evidence(
    before: &PythonProjectEnvironment,
    after: &PythonProjectEnvironment,
) -> bool {
    before.project_directory == after.project_directory
        && before.requirement_state == after.requirement_state
        && before.files.len() == after.files.len()
        && before.files.iter().zip(&after.files).all(|(a, b)| {
            a.path == b.path
                && a.canonical_path == b.canonical_path
                && a.exists == b.exists
                && a.sha256 == b.sha256
        })
}
fn evaluate(
    tools: &[PresetTool],
    captured: &CapturedInventories,
    projects: ProjectReports<'_>,
    options: &PythonScanOptions,
    service: &OperationPlanService,
    query: &InventoryQueryService,
    config_path: &Path,
) -> RecipeReport {
    let mut steps = Vec::with_capacity(tools.len());
    for tool in tools {
        let mut step = RecipeStep {
            tool: tool.name.clone(),
            request: Some(tool.version.clone()),
            disposition: RecipeDisposition::Unknown,
            channel: None,
            installation_id: None,
            version: None,
            reason: String::new(),
            plan_ready: false,
        };
        let Some(kind) = inventory_tool(&tool.name) else {
            step.disposition = RecipeDisposition::Unsupported;
            step.reason = "只读配方当前仅覆盖 Node 与 Python".into();
            steps.push(step);
            continue;
        };
        let Some((generation, snapshot)) = captured.get(&kind).and_then(Option::as_ref) else {
            step.reason = "该工具尚无清单，请先完成读取".into();
            steps.push(step);
            continue;
        };
        if !complete(snapshot, kind) {
            step.reason = "该工具来源缺失、失败、过期或覆盖不完整，保持未知".into();
            steps.push(step);
            continue;
        }
        if !exact_version(kind, &tool.version) {
            step.reason = "配方只接受精确 3 段版本；范围或别名需人工确认".into();
            steps.push(step);
            continue;
        }
        let project_match = match kind {
            InventoryTool::Node => projects
                .node
                .map(|project| match_node_request(project, &tool.version)),
            InventoryTool::Python => projects
                .python
                .map(|project| match_python_request(project, &tool.version)),
        };
        if let Some(project_match) = project_match {
            match project_match {
                Match::Satisfied => {}
                Match::Mismatch => {
                    step.disposition = RecipeDisposition::Conflict;
                    step.reason = "项目声明互相冲突或请求版本不满足项目要求".into();
                    steps.push(step);
                    continue;
                }
                Match::Unknown => {
                    step.reason = "项目包含未知声明，无法确认请求版本满足要求".into();
                    steps.push(step);
                    continue;
                }
            }
        }
        if let Some(item) = snapshot.installations.iter().find(|item| {
            item.tool_id == kind.id()
                && item.health == Health::Healthy
                && !item.stale
                && item.provider_package.provider_id != ProviderId::Path
                && item.version.as_deref() == Some(tool.version.as_str())
        }) {
            step.disposition = RecipeDisposition::Reuse;
            step.channel = Some(channel(item.provider_package.provider_id).into());
            step.installation_id = Some(item.installation_id.0.clone());
            step.version = item.version.clone();
            step.reason = "已有该精确版本的健康候选，无需重复安装；项目绑定仍需独立核对".into();
            steps.push(step);
            continue;
        }
        let request = PlanRequest {
            snapshot_request_id: snapshot.request_id.clone(),
            intent: PlanIntent::Install {
                provider_id: ProviderId::Mise,
                package_id: tool.name.clone(),
                version: Some(tool.version.clone()),
            },
        };
        let result = service.prepare_install_summary(query, kind, request, options, config_path);
        match result {
            Ok(plan)
                if plan.snapshot_request_id != snapshot.request_id
                    || plan.snapshot_generation != generation.to_string() =>
            {
                step.reason = "计划绑定的清单已变化，请重新预览".into()
            }
            Ok(plan) if plan.capability.can_execute => {
                step.disposition = RecipeDisposition::Install;
                step.channel = Some("mise".into());
                step.version = Some(tool.version.clone());
                step.reason = "既有 mise 精确安装门禁通过，可另行打开操作计划".into();
                step.plan_ready = true;
            }
            Ok(plan) => {
                step.reason = plan
                    .capability
                    .reason
                    .unwrap_or_else(|| "当前安装能力无法确认".into())
            }
            Err(error) => step.reason = format!("安装能力无法确认：{error}"),
        }
        steps.push(step);
    }
    let issues = steps
        .iter()
        .filter(|step| {
            matches!(
                step.disposition,
                RecipeDisposition::Conflict
                    | RecipeDisposition::Unsupported
                    | RecipeDisposition::Unknown
            )
        })
        .map(|step| format!("{}：{}", step.tool, step.reason))
        .collect();
    let snapshot_bindings: Vec<_> = captured
        .iter()
        .map(|(tool, snapshot)| RecipeSnapshotBinding {
            tool: *tool,
            request_id: snapshot.as_ref().map(|(_, s)| s.request_id.clone()),
            generation: snapshot.as_ref().map(|(g, _)| g.to_string()),
        })
        .collect();
    RecipeReport {
        schema_version: 2,
        snapshot_request_id: if snapshot_bindings.len() == 1 {
            snapshot_bindings[0].request_id.clone()
        } else {
            None
        },
        snapshot_bindings,
        steps,
        executable: false,
        issues,
    }
}
#[cfg(test)]
mod tests;
