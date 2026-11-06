use super::*;

fn node_project_report(state: RequirementState, requirements: &[&str]) -> ProjectEnvironment {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/project-environment/contract-v1.json"
    ))
    .unwrap();
    let mut report: ProjectEnvironment = serde_json::from_value(fixture["report"].clone()).unwrap();
    let supported = state == RequirementState::Supported;
    report.requirement_state = state;
    report.declarations = requirements
        .iter()
        .map(|requirement| Declaration {
            source: "package.json engines.node".into(),
            path: "/observed/project/package.json".into(),
            requirement: Some((*requirement).into()),
            supported,
            reason: None,
        })
        .collect();
    report.candidates.clear();
    report
}

#[test]
fn node_request_declarations_gate_reuse_before_considering_a_healthy_instance() {
    let captures = captured(
        InventoryTool::Node,
        1,
        snapshot(
            InventoryTool::Node,
            "node-captured",
            vec![installation(
                InventoryTool::Node,
                ProviderId::Mise,
                "22.1.0",
            )],
        ),
    );
    for (state, requirement, expected) in [
        (
            RequirementState::Conflict,
            ">=24 <22",
            RecipeDisposition::Conflict,
        ),
        (
            RequirementState::Unknown,
            "lts/*",
            RecipeDisposition::Unknown,
        ),
        (
            RequirementState::Supported,
            ">=24",
            RecipeDisposition::Conflict,
        ),
        (
            RequirementState::Supported,
            ">=22 <23",
            RecipeDisposition::Reuse,
        ),
    ] {
        let project = node_project_report(state, &[requirement]);
        assert!(project.candidates.is_empty());
        let report = synthetic_projects(
            &[requested("node", "22.1.0")],
            &captures,
            Some(&project),
            None,
        );
        assert_eq!(report.steps[0].disposition, expected);
        assert!(!report.executable);
        assert!(!report.steps[0].plan_ready);
        if expected != RecipeDisposition::Reuse {
            assert!(report.steps[0].installation_id.is_none());
            assert!(!report.steps[0].reason.contains("安装能力无法确认"));
        }
    }
}

#[test]
fn node_requests_are_matched_before_install_preparation_with_no_installed_candidates() {
    let captures = captured(
        InventoryTool::Node,
        1,
        snapshot(InventoryTool::Node, "node-empty", vec![]),
    );
    let mismatch = node_project_report(RequirementState::Supported, &[">=24 <25"]);
    let denied = synthetic_projects(
        &[requested("node", "22.1.0")],
        &captures,
        Some(&mismatch),
        None,
    );
    assert_eq!(denied.steps[0].disposition, RecipeDisposition::Conflict);
    assert!(!denied.steps[0].reason.contains("安装能力无法确认"));
    let matching = node_project_report(RequirementState::Supported, &[">=22 <23"]);
    let attempted = synthetic_projects(
        &[requested("node", "22.1.0")],
        &captures,
        Some(&matching),
        None,
    );
    // This synthetic query has no live cache; matching reaches preparation, which rejects it.
    assert_eq!(attempted.steps[0].disposition, RecipeDisposition::Unknown);
    assert!(attempted.steps[0].reason.contains("安装能力无法确认"));
    assert!(!attempted.executable);
    assert!(!attempted.steps[0].plan_ready);
}

#[test]
fn unspecified_node_project_cannot_authorize_a_version_beyond_the_matcher_numeric_domain() {
    let version = "4294967296.0.0";
    let captures = captured(
        InventoryTool::Node,
        1,
        snapshot(
            InventoryTool::Node,
            "node-overflow",
            vec![installation(InventoryTool::Node, ProviderId::Mise, version)],
        ),
    );
    let project = node_project_report(RequirementState::Unspecified, &[]);
    let report = synthetic_projects(
        &[requested("node", version)],
        &captures,
        Some(&project),
        None,
    );
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Unknown);
    assert!(report.steps[0].installation_id.is_none());
    assert!(!report.steps[0].plan_ready);
    assert!(!report.executable);
}

#[test]
fn mixed_recipe_preserves_python_evidence_when_node_request_conflicts() {
    let mut captures = captured(
        InventoryTool::Node,
        4,
        snapshot(
            InventoryTool::Node,
            "node-mixed",
            vec![installation(
                InventoryTool::Node,
                ProviderId::Mise,
                "22.1.0",
            )],
        ),
    );
    captures.extend(captured(
        InventoryTool::Python,
        9,
        snapshot(
            InventoryTool::Python,
            "python-mixed",
            vec![installation(
                InventoryTool::Python,
                ProviderId::Mise,
                "3.12.14",
            )],
        ),
    ));
    let node = node_project_report(RequirementState::Supported, &[">=24"]);
    let python = project_report(RequirementState::Supported, ">=3.12,<3.13");
    let report = synthetic_projects(
        &[requested("node", "22.1.0"), requested("python", "3.12.14")],
        &captures,
        Some(&node),
        Some(&python),
    );
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Conflict);
    assert_eq!(report.steps[1].disposition, RecipeDisposition::Reuse);
    assert_eq!(
        report.snapshot_bindings[0].request_id.as_deref(),
        Some("node-mixed")
    );
    assert_eq!(
        report.snapshot_bindings[1].request_id.as_deref(),
        Some("python-mixed")
    );
    assert!(!report.executable);
}

#[cfg(unix)]
mod production {
    use super::super::unix::Fixture;
    use super::*;
    use std::fs;

    fn prepare_project(fixture: &Fixture) -> PythonProjectRequest {
        fixture.python("python-node-project");
        let project = fixture.root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        PythonProjectRequest {
            request_id: "node-project-read".into(),
            snapshot_request_id: "python-node-project".into(),
            path: project.to_string_lossy().into(),
        }
    }

    fn preview_node(fixture: &Fixture, project: &PythonProjectRequest) -> RecipeReport {
        preview_recipe(
            &[requested("node", "22.23.3")],
            Some(project),
            &fixture.options,
            &fixture.service,
            &fixture.query,
            &fixture.options.common.home.join(".config/mise/config.toml"),
        )
        .unwrap()
    }

    #[test]
    fn production_node_preview_reads_engines_before_reuse_without_probing_or_writing() {
        let fixture = Fixture::new();
        fixture
            .query
            .begin("node-project".into(), None)
            .unwrap()
            .execute(fixture.options.common.clone())
            .unwrap();
        let project = prepare_project(&fixture);
        let package = Path::new(&project.path).join("package.json");
        for (content, expected) in [
            (
                r#"{"engines":{"node":">=22 <23"}}"#,
                RecipeDisposition::Reuse,
            ),
            (
                r#"{"engines":{"node":">=24"}}"#,
                RecipeDisposition::Conflict,
            ),
            (
                r#"{"engines":{"node":"lts/*"}}"#,
                RecipeDisposition::Unknown,
            ),
            ("invalid JSON", RecipeDisposition::Unknown),
        ] {
            fs::write(&package, content).unwrap();
            let captured = fixture
                .query
                .capture_snapshots(&[InventoryTool::Node, InventoryTool::Python])
                .unwrap();
            let report = preview_node(&fixture, &project);
            assert_eq!(report.steps[0].disposition, expected);
            assert!(!report.executable);
            assert!(!report.steps[0].plan_ready);
            fixture.query.validate_snapshots(&captured).unwrap();
            assert_eq!(fs::read_to_string(&package).unwrap(), content);
        }
        fixture.assert_readonly();
    }

    #[test]
    fn production_empty_node_inventory_still_blocks_a_mismatching_install_request() {
        let fixture = Fixture::new();
        fs::remove_dir_all(
            fixture
                .options
                .common
                .mise_data_dir
                .join("installs/node/22.23.3"),
        )
        .unwrap();
        let node = fixture
            .query
            .begin("node-project-empty".into(), None)
            .unwrap()
            .execute(fixture.options.common.clone())
            .unwrap();
        assert!(node.installations.is_empty());
        assert!(complete(&node, InventoryTool::Node));
        let project = prepare_project(&fixture);
        fs::write(
            Path::new(&project.path).join("package.json"),
            r#"{"engines":{"node":">=24 <25"}}"#,
        )
        .unwrap();
        let report = preview_node(&fixture, &project);
        assert_eq!(report.steps[0].disposition, RecipeDisposition::Conflict);
        assert!(!report.steps[0].reason.contains("安装能力无法确认"));
        assert!(!report.steps[0].plan_ready);
        assert!(!report.executable);
        fixture.assert_readonly();
    }
}
