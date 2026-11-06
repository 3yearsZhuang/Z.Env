use super::*;
use crate::domain::{
    Installation, InstallationId, Ownership, PackageKind, Platform, ProviderPackage,
    ProviderReport, ProviderStatus, ScanCoverage,
};
use crate::project_environment::{Declaration, PythonSelection, RequirementState};
use crate::providers::NodeScanOptions;
use std::path::Path;

fn options(root: &Path) -> PythonScanOptions {
    PythonScanOptions {
        common: NodeScanOptions {
            platform: Platform::Macos,
            home: root.join("home"),
            mise_data_dir: root.join("mise"),
            nvm_dir: root.join("nvm"),
            brew_cellars: vec![root.join("Cellar")],
            path_entries: vec![root.join("bin")],
            limits: Default::default(),
        },
        pyenv_root: root.join("pyenv"),
        uv_python_dir: root.join("uv/python"),
    }
}

fn requested(name: &str, version: &str) -> PresetTool {
    PresetTool {
        name: name.into(),
        version: version.into(),
    }
}

fn installation(tool: InventoryTool, provider: ProviderId, version: &str) -> Installation {
    Installation {
        installation_id: InstallationId::from(format!("{:?}-{provider:?}-{version}", tool)),
        provider_package: ProviderPackage {
            provider_id: provider,
            package_id: tool.id().0.clone(),
            kind: if provider == ProviderId::Path {
                PackageKind::Executable
            } else {
                PackageKind::Runtime
            },
            platform: Platform::Macos,
        },
        tool_id: tool.id(),
        locator: format!("/observed/{}/{version}", tool.id().0),
        executable_path: None,
        canonical_executable_path: None,
        version: Some(version.into()),
        architecture: None,
        health: Health::Healthy,
        ownership: Ownership::Observed,
        evidence: vec![],
        stale: false,
    }
}

fn snapshot(tool: InventoryTool, id: &str, items: Vec<Installation>) -> InventorySnapshot {
    InventorySnapshot {
        schema_version: 1,
        request_id: id.into(),
        observed_at: 0,
        tools: vec![],
        providers: tool
            .providers()
            .iter()
            .copied()
            .map(|provider_id| ProviderReport {
                provider_id,
                status: ProviderStatus::Ok,
                observed_at: 0,
                stale: false,
                coverage: ScanCoverage {
                    tool_ids: vec![tool.id()],
                    complete: true,
                    truncated: false,
                },
                error: None,
            })
            .collect(),
        installations: items,
        aliases: vec![],
    }
}

fn captured(
    tool: InventoryTool,
    generation: u64,
    snapshot: InventorySnapshot,
) -> CapturedInventories {
    [(tool, Some((generation, snapshot)))].into_iter().collect()
}

fn project_report(state: RequirementState, requirement: &str) -> PythonProjectEnvironment {
    PythonProjectEnvironment {
        schema_version: 1,
        request_id: "project-read".into(),
        snapshot_request_id: "python-captured".into(),
        project_directory: "/observed/project".into(),
        observed_at: 0,
        declarations: vec![Declaration {
            source: "pyproject.toml".into(),
            path: "/observed/project/pyproject.toml".into(),
            requirement: Some(requirement.into()),
            supported: state == RequirementState::Supported,
            reason: None,
        }],
        requirement_state: state,
        candidates: vec![],
        inventory_complete: true,
        selection: PythonSelection {
            kind: "none".into(),
            path: None,
            requirement: None,
            reason: None,
        },
        issues: vec![],
        files: vec![],
    }
}

/// 纯报告测试不给查询服务提交清单，也不给渠道任何真实路径。
fn synthetic(
    tools: &[PresetTool],
    captured: &CapturedInventories,
    project: Option<&PythonProjectEnvironment>,
) -> RecipeReport {
    synthetic_projects(tools, captured, None, project)
}

fn synthetic_projects(
    tools: &[PresetTool],
    captured: &CapturedInventories,
    node: Option<&ProjectEnvironment>,
    python: Option<&PythonProjectEnvironment>,
) -> RecipeReport {
    let root = std::env::temp_dir().join(format!("zenv-recipe-unused-{}", std::process::id()));
    let existed = root.exists();
    let options = options(&root);
    let session = OperationPlanService::new(root.join("operations.json"));
    let report = evaluate(
        tools,
        captured,
        ProjectReports { node, python },
        &options,
        &session,
        &InventoryQueryService::default(),
        &options.common.home.join(".config/mise/config.toml"),
    );
    assert_eq!(
        root.exists(),
        existed,
        "只读报告不应创建渠道或 journal 目录"
    );
    report
}

#[test]
fn python_only_and_mixed_reports_preserve_each_snapshot_identity() {
    let python = snapshot(
        InventoryTool::Python,
        "python-captured",
        vec![installation(
            InventoryTool::Python,
            ProviderId::Mise,
            "3.12.14",
        )],
    );
    let mut captures = captured(InventoryTool::Python, 9, python);
    let report = synthetic(&[requested("python", "3.12.14")], &captures, None);
    assert_eq!(report.schema_version, 2);
    assert_eq!(
        report.snapshot_request_id.as_deref(),
        Some("python-captured")
    );
    assert_eq!(report.snapshot_bindings[0].tool, InventoryTool::Python);
    assert_eq!(report.snapshot_bindings[0].generation.as_deref(), Some("9"));
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Reuse);
    assert_eq!(report.steps[0].channel.as_deref(), Some("mise"));
    assert!(!report.steps[0].plan_ready);
    assert!(!report.executable);
    captures.insert(
        InventoryTool::Node,
        Some((
            4,
            snapshot(
                InventoryTool::Node,
                "node-captured",
                vec![installation(
                    InventoryTool::Node,
                    ProviderId::Mise,
                    "22.23.3",
                )],
            ),
        )),
    );
    let report = synthetic(
        &[requested("node", "22.23.3"), requested("python", "3.12.14")],
        &captures,
        None,
    );
    assert_eq!(report.snapshot_request_id, None);
    assert!(report
        .steps
        .iter()
        .all(|step| step.disposition == RecipeDisposition::Reuse));
    assert_eq!(report.snapshot_bindings.len(), 2);
    assert_eq!(
        report.snapshot_bindings[0].request_id.as_deref(),
        Some("node-captured")
    );
    assert_eq!(report.snapshot_bindings[0].generation.as_deref(), Some("4"));
    assert_eq!(
        report.snapshot_bindings[1].request_id.as_deref(),
        Some("python-captured")
    );
    assert_eq!(report.snapshot_bindings[1].generation.as_deref(), Some("9"));
    let json = serde_json::to_value(report).unwrap();
    assert!(!json.to_string().contains("planId"));
}

#[test]
fn missing_empty_partial_stale_or_failed_sources_keep_healthy_candidates_unknown() {
    let tool = InventoryTool::Python;
    let requested = [requested("python", "3.12.14")];
    let missing: CapturedInventories = [(tool, None)].into_iter().collect();
    let report = synthetic(&requested, &missing, None);
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Unknown);
    assert_eq!(report.snapshot_bindings[0].request_id, None);
    assert_eq!(report.snapshot_bindings[0].generation, None);
    let baseline = snapshot(
        tool,
        "python-captured",
        vec![installation(tool, ProviderId::Mise, "3.12.14")],
    );
    for variant in 0..6 {
        let mut snapshot = baseline.clone();
        match variant {
            0 => snapshot.providers.clear(),
            1 => {
                snapshot.providers.pop();
            }
            2 => snapshot.providers[0].coverage.complete = false,
            3 => snapshot.providers[0].stale = true,
            4 => snapshot.providers[0].coverage.truncated = true,
            _ => snapshot.providers[0].status = ProviderStatus::Timeout,
        }
        let report = synthetic(&requested, &captured(tool, 1, snapshot), None);
        assert_eq!(
            report.steps[0].disposition,
            RecipeDisposition::Unknown,
            "variant {variant}"
        );
        assert!(report.steps[0].installation_id.is_none());
        assert!(!report.steps[0].plan_ready);
    }
}

#[test]
fn ranges_path_broken_and_stale_installations_do_not_become_reusable() {
    for tool in [InventoryTool::Node, InventoryTool::Python] {
        let version = if tool == InventoryTool::Node {
            "22.23.3"
        } else {
            "3.12.14"
        };
        for variant in 0..3 {
            let mut item = installation(tool, ProviderId::Mise, version);
            match variant {
                0 => item.provider_package.provider_id = ProviderId::Path,
                1 => item.stale = true,
                _ => item.health = Health::Broken,
            }
            let captures = captured(tool, 1, snapshot(tool, "captured", vec![item]));
            let report = synthetic(&[requested(&tool.id().0, version)], &captures, None);
            assert_eq!(report.steps[0].disposition, RecipeDisposition::Unknown);
            assert!(report.steps[0].installation_id.is_none());
            assert!(!report.steps[0].plan_ready);
        }
        for range in ["latest", "3.12", "3.12.x", ">=3.12"] {
            let captures = captured(
                tool,
                1,
                snapshot(
                    tool,
                    "captured",
                    vec![installation(tool, ProviderId::Mise, range)],
                ),
            );
            let report = synthetic(&[requested(&tool.id().0, range)], &captures, None);
            assert_eq!(report.steps[0].disposition, RecipeDisposition::Unknown);
            assert!(report.steps[0].installation_id.is_none());
            assert!(!report.steps[0].plan_ready);
        }
    }
}

#[test]
fn declarations_block_reuse_and_probe_before_matching_a_healthy_candidate() {
    let tool = InventoryTool::Python;
    let captures = captured(
        tool,
        1,
        snapshot(
            tool,
            "python-captured",
            vec![installation(tool, ProviderId::Mise, "3.12.14")],
        ),
    );
    for (state, requirement, expected) in [
        (
            RequirementState::Conflict,
            ">=3.13,<3.12",
            RecipeDisposition::Conflict,
        ),
        (
            RequirementState::Unknown,
            "^3.12",
            RecipeDisposition::Unknown,
        ),
        (
            RequirementState::Supported,
            ">=3.13",
            RecipeDisposition::Conflict,
        ),
    ] {
        let project = project_report(state, requirement);
        let report = synthetic(&[requested("python", "3.12.14")], &captures, Some(&project));
        assert_eq!(report.steps[0].disposition, expected);
        assert!(report.steps[0].installation_id.is_none());
        assert!(!report.steps[0].plan_ready);
        assert!(
            !report.steps[0].reason.contains("安装能力无法确认"),
            "声明失败不能继续探测渠道"
        );
    }
    let matching = project_report(RequirementState::Supported, ">=3.12,<3.13");
    let report = synthetic(
        &[requested("python", "3.12.14")],
        &captures,
        Some(&matching),
    );
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Reuse);
}

#[test]
fn empty_and_unsupported_recipes_never_claim_execution() {
    let empty = synthetic(&[], &CapturedInventories::new(), None);
    assert!(empty.steps.is_empty());
    assert!(empty.snapshot_bindings.is_empty());
    assert_eq!(empty.snapshot_request_id, None);
    assert!(!empty.executable);
    let unsupported = synthetic(
        &[requested("java", "21.0.1")],
        &CapturedInventories::new(),
        None,
    );
    assert_eq!(
        unsupported.steps[0].disposition,
        RecipeDisposition::Unsupported
    );
    assert!(!unsupported.executable);
}

#[cfg(unix)]
mod install;

mod node;

#[cfg(unix)]
mod unix {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    pub(super) struct Fixture {
        pub(super) root: tempfile::TempDir,
        pub(super) options: PythonScanOptions,
        pub(super) query: InventoryQueryService,
        pub(super) service: OperationPlanService,
    }

    fn script(path: &Path, value: serde_json::Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("#!/bin/sh\nprintf '%s' '{}'\n", value)).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    impl Fixture {
        pub(super) fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let options = options(root.path());
            for path in [
                options.common.home.clone(),
                options.common.mise_data_dir.join("installs/node"),
                options.common.mise_data_dir.join("installs/nodejs"),
                options.common.mise_data_dir.join("installs/python"),
                options.common.nvm_dir.join("versions/node"),
                options.common.brew_cellars[0].clone(),
                options.common.path_entries[0].clone(),
                options.pyenv_root.join("versions"),
                options.uv_python_dir.clone(),
            ] {
                fs::create_dir_all(path).unwrap();
            }
            let python = options
                .common
                .mise_data_dir
                .join("installs/python/3.12.14/bin/python3");
            script(
                &python,
                serde_json::json!({
                    "version":"3.12.14", "implementation":"cpython", "releaseLevel":"final",
                    "freeThreaded":false, "debug":false, "architecture":"arm64", "execPath":python,
                }),
            );
            script(
                &options
                    .common
                    .mise_data_dir
                    .join("installs/node/22.23.3/bin/node"),
                serde_json::json!({"version":"22.23.3","architecture":"arm64"}),
            );
            let service = OperationPlanService::new(root.path().join("journal/operations.json"));
            Self {
                root,
                options,
                query: InventoryQueryService::default(),
                service,
            }
        }

        pub(super) fn python(&self, id: &str) -> InventorySnapshot {
            self.query
                .begin_tool(id.into(), None, InventoryTool::Python)
                .unwrap()
                .execute_python(self.options.clone())
                .unwrap()
        }

        fn preview(&self, project: Option<&PythonProjectRequest>) -> Result<RecipeReport, String> {
            preview_recipe(
                &[requested("python", "3.12.14")],
                project,
                &self.options,
                &self.service,
                &self.query,
                &self.options.common.home.join(".config/mise/config.toml"),
            )
        }

        pub(super) fn assert_readonly(&self) {
            assert!(!self.root.path().join("journal").exists());
            assert!(!self
                .options
                .common
                .home
                .join(".config/mise/config.toml")
                .exists());
            assert!(!self.options.common.home.join(".zenv").exists());
        }
    }

    #[test]
    fn production_entry_reuses_python_only_without_scanning_node_or_writing() {
        let fixture = Fixture::new();
        let snapshot = fixture.python("python-only");
        assert!(complete(&snapshot, InventoryTool::Python));
        assert!(snapshot
            .installations
            .iter()
            .any(|item| item.health == Health::Healthy && item.ownership == Ownership::Observed));
        let before = fixture
            .query
            .capture_snapshots(&[InventoryTool::Node, InventoryTool::Python])
            .unwrap();
        assert!(before[&InventoryTool::Node].is_none());
        let report = fixture.preview(None).unwrap();
        assert_eq!(report.steps[0].disposition, RecipeDisposition::Reuse);
        assert_eq!(report.snapshot_request_id.as_deref(), Some("python-only"));
        fixture.query.validate_snapshots(&before).unwrap();
        fixture.assert_readonly();
    }

    #[test]
    fn production_entry_rejects_wrong_project_snapshot_ids_and_path_budget() {
        let fixture = Fixture::new();
        fixture.python("python-only");
        let project = fixture.root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let baseline = PythonProjectRequest {
            request_id: "project-read".into(),
            snapshot_request_id: "python-only".into(),
            path: project.to_string_lossy().into(),
        };
        let mut wrong = baseline.clone();
        wrong.snapshot_request_id = "different-snapshot".into();
        assert!(fixture
            .preview(Some(&wrong))
            .unwrap_err()
            .contains("清单已变化"));
        for id in ["".to_owned(), "invalid id".into(), "x".repeat(129)] {
            let mut invalid = baseline.clone();
            invalid.request_id = id.clone();
            assert!(fixture.preview(Some(&invalid)).is_err());
            invalid = baseline.clone();
            invalid.snapshot_request_id = id;
            assert!(fixture.preview(Some(&invalid)).is_err());
        }
        let mut oversized = baseline;
        oversized.path = "x".repeat(4097);
        assert!(fixture
            .preview(Some(&oversized))
            .unwrap_err()
            .contains("路径超出预算"));
        fixture.assert_readonly();
    }

    #[test]
    fn production_entry_reads_project_declarations_before_reusing_a_healthy_python() {
        let fixture = Fixture::new();
        fixture.python("python-only");
        let project = fixture.root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let request = PythonProjectRequest {
            request_id: "project-read".into(),
            snapshot_request_id: "python-only".into(),
            path: project.to_string_lossy().into(),
        };
        for (python_version, requires_python, expected) in [
            ("3.12.14", ">=3.13", RecipeDisposition::Conflict),
            ("3.12.14", "^3.12", RecipeDisposition::Unknown),
            ("3.13.1", ">=3.13", RecipeDisposition::Conflict),
            ("3.12.14", ">=3.12,<3.13", RecipeDisposition::Reuse),
        ] {
            let declaration = format!("[project]\nrequires-python = '{requires_python}'\n");
            fs::write(project.join(".python-version"), python_version).unwrap();
            fs::write(project.join("pyproject.toml"), &declaration).unwrap();
            let report = fixture.preview(Some(&request)).unwrap();
            assert_eq!(report.steps[0].disposition, expected);
            assert!(!report.steps[0].plan_ready);
            assert_eq!(
                fs::read_to_string(project.join(".python-version")).unwrap(),
                python_version
            );
            assert_eq!(
                fs::read_to_string(project.join("pyproject.toml")).unwrap(),
                declaration
            );
        }
        fixture.assert_readonly();
    }

    #[test]
    fn production_entry_keeps_distinct_real_node_and_python_snapshot_bindings() {
        let fixture = Fixture::new();
        let node = fixture
            .query
            .begin("node-real".into(), None)
            .unwrap()
            .execute(fixture.options.common.clone())
            .unwrap();
        let python = fixture.python("python-real");
        assert!(complete(&node, InventoryTool::Node));
        assert!(complete(&python, InventoryTool::Python));
        let before = fixture
            .query
            .capture_snapshots(&[InventoryTool::Node, InventoryTool::Python])
            .unwrap();
        let report = preview_recipe(
            &[requested("node", "22.23.3"), requested("python", "3.12.14")],
            None,
            &fixture.options,
            &fixture.service,
            &fixture.query,
            &fixture.options.common.home.join(".config/mise/config.toml"),
        )
        .unwrap();
        assert!(report
            .steps
            .iter()
            .all(|step| step.disposition == RecipeDisposition::Reuse));
        assert_eq!(report.snapshot_request_id, None);
        assert_eq!(
            report.snapshot_bindings[0].request_id.as_deref(),
            Some(node.request_id.as_str())
        );
        assert_eq!(
            report.snapshot_bindings[1].request_id.as_deref(),
            Some(python.request_id.as_str())
        );
        assert_ne!(
            report.snapshot_bindings[0].generation,
            report.snapshot_bindings[1].generation
        );
        fixture.query.validate_snapshots(&before).unwrap();
        fixture.assert_readonly();
    }
}
