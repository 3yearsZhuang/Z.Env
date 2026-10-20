use super::*;
use serde_json::{json, Value};

fn contract() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/operations/contract-v2.json"
    ))
    .unwrap()
}

#[test]
fn shared_v2_contract_preserves_old_plans_and_stable_linear_graphs() {
    let samples = contract();
    for name in ["node", "python"] {
        let sample = &samples[name];
        let request: PlanRequestV2 = serde_json::from_value(sample["request"].clone()).unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), sample["request"]);
        let plan: OperationPlanV2 = serde_json::from_value(sample["response"].clone()).unwrap();
        assert_eq!(serde_json::to_value(&plan).unwrap(), sample["response"]);
        let wrapped = OperationPlanV2::from_plan(plan.plan.clone());
        assert_eq!(serde_json::to_value(&wrapped).unwrap(), sample["response"]);
        assert_eq!(wrapped.step_graph.len(), wrapped.plan.steps.len());
        assert_eq!(
            wrapped.step_graph,
            OperationPlanV2::from_plan(wrapped.plan.clone()).step_graph
        );
        let inner = serde_json::to_value(&wrapped.plan).unwrap();
        assert_eq!(
            inner.get("toolId").and_then(Value::as_str),
            if name == "node" { None } else { Some("python") }
        );
    }
}

#[test]
fn v2_rejects_future_versions_unknown_subjects_and_command_injection() {
    let base = contract()["node"]["request"].clone();
    let mut invalid = Vec::new();
    for version in [json!(1), json!(3), json!("2"), json!(-1), Value::Null] {
        let mut request = base.clone();
        request["schemaVersion"] = version;
        invalid.push(request);
    }
    for key in ["argv", "program", "stepGraph", "steps"] {
        let mut request = base.clone();
        request[key] = json!([]);
        invalid.push(request);
        let mut request = base.clone();
        request["subject"][key] = json!([]);
        invalid.push(request);
        let mut request = base.clone();
        request["request"]["intent"][key] = json!([]);
        invalid.push(request);
    }
    for kind in ["shell", "recipe", "service", "snapshotRestore"] {
        let mut request = base.clone();
        request["subject"]["kind"] = json!(kind);
        invalid.push(request);
    }
    for request in invalid {
        assert!(
            serde_json::from_value::<PlanRequestV2>(request.clone()).is_err(),
            "{request}"
        );
    }
}

#[test]
fn unsupported_tools_and_programmatic_versions_stop_before_environment_or_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let service = OperationPlanService::new(directory.path().join("must-not-create/journal.json"));
    let query = InventoryQueryService::default();
    let request: PlanRequestV2 =
        serde_json::from_value(contract()["node"]["request"].clone()).unwrap();
    for name in [
        "java",
        "go",
        "rust",
        "git",
        "uv",
        "unknown",
        "",
        "Node",
        "node;touch x",
    ] {
        let mut request = request.clone();
        request.subject = OperationSubjectV2::Tool {
            tool_id: ToolId::from(name),
        };
        assert!(
            matches!(
                service.preview_v2_current(&query, request),
                Err(PlanError::Unsupported)
            ),
            "{name}"
        );
    }
    let mut request = request;
    request.schema_version = 3;
    assert!(matches!(
        service.preview_v2_current(&query, request),
        Err(PlanError::Unsupported)
    ));
    assert!(!directory.path().join("must-not-create").exists());
    assert!(super::super::lock(&service.state).plans.is_empty());
}
