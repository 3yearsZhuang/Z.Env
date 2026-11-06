use super::*;
use serde_json::{json, Value};

fn contract() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/operations/recipe-contract-v2.json"
    ))
    .unwrap()
}

#[test]
fn shared_recipe_requests_and_disabled_plan_round_trip_with_validated_models() {
    let samples = contract();
    for sample in samples["requests"].as_object().unwrap().values() {
        let request: RecipePlanRequestV2 = serde_json::from_value(sample.clone()).unwrap();
        request.validate().unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), *sample);
        let envelope: AnyPlanRequestV2 = serde_json::from_value(sample.clone()).unwrap();
        assert!(matches!(envelope, AnyPlanRequestV2::Recipe(_)));
        assert_eq!(serde_json::to_value(envelope).unwrap(), *sample);
    }
    for sample in samples["plans"].as_object().unwrap().values() {
        let plan: RecipeOperationPlanV2 = serde_json::from_value(sample.clone()).unwrap();
        plan.validate().unwrap();
        assert_eq!(serde_json::to_value(&plan).unwrap(), *sample);
        assert!(!plan.plan.capability.can_execute);
        assert_eq!(plan.plan.steps.len(), 2 + 11 * plan.plan.items.len());
    }
}

#[test]
fn shared_invalid_recipe_requests_and_plans_fail_deserialization() {
    let samples = contract();
    for sample in samples["invalidRequests"].as_array().unwrap() {
        let value = super::super::fixture_cases::materialize(&samples["requests"], sample);
        assert!(
            serde_json::from_value::<RecipePlanRequestV2>(value.clone()).is_err(),
            "request {}",
            sample["name"]
        );
        assert!(
            serde_json::from_value::<AnyPlanRequestV2>(value).is_err(),
            "envelope {}",
            sample["name"]
        );
    }
    for sample in samples["invalidPlans"].as_array().unwrap() {
        let value = super::super::fixture_cases::materialize(&samples["plans"], sample);
        assert!(
            serde_json::from_value::<RecipeOperationPlanV2>(value).is_err(),
            "plan {}",
            sample["name"]
        );
    }
}

#[test]
fn disabled_reason_requires_visible_text_and_preserves_a_bom_with_visible_prefix() {
    let samples = contract();
    let mut plan: RecipeOperationPlanV2 =
        serde_json::from_value(samples["plans"]["mixed"].clone()).unwrap();
    for reason in ["\u{feff}", " \u{feff} "] {
        plan.plan.capability.reason = reason.into();
        assert!(plan.validate().is_err());
    }
    let sample = &samples["plans"]["visibleReasonBom"];
    let plan: RecipeOperationPlanV2 = serde_json::from_value(sample.clone()).unwrap();
    assert_eq!(
        plan.plan.capability.reason,
        "Recipe execution disabled.\u{feff}"
    );
    plan.validate().unwrap();
    assert_eq!(serde_json::to_value(plan).unwrap(), *sample);
}

#[test]
fn old_tool_struct_api_and_wire_are_preserved_by_the_new_envelope() {
    let samples: Value = serde_json::from_str(include_str!(
        "../../../fixtures/operations/contract-v2.json"
    ))
    .unwrap();
    for name in ["node", "python"] {
        let sample = &samples[name];
        let old: PlanRequestV2 = serde_json::from_value(sample["request"].clone()).unwrap();
        let rebuilt = PlanRequestV2 {
            schema_version: old.schema_version,
            subject: old.subject,
            request: old.request,
        };
        let wrapped = AnyPlanRequestV2::Tool(rebuilt);
        assert_eq!(serde_json::to_value(&wrapped).unwrap(), sample["request"]);
        assert!(matches!(
            serde_json::from_value::<AnyPlanRequestV2>(sample["request"].clone()).unwrap(),
            AnyPlanRequestV2::Tool(_)
        ));
        let old_plan: OperationPlanV2 = serde_json::from_value(sample["response"].clone()).unwrap();
        let plan = OperationPlanV2::from_plan(old_plan.plan);
        assert_eq!(serde_json::to_value(plan).unwrap(), sample["response"]);
    }
}

#[test]
fn raw_v2_envelope_rejects_duplicate_keys_at_every_object_depth() {
    let raw_requests = [
        r#"{"schemaVersion":3,"schemaVersion":2,"subject":{"kind":"tool","toolId":"node"},"request":{"snapshotRequestId":"snapshot","intent":{"action":"verify","installationId":"instance"}}}"#,
        r#"{"schemaVersion":2,"subject":{"kind":"recipe","kind":"tool","toolId":"node"},"request":{"snapshotRequestId":"snapshot","intent":{"action":"verify","installationId":"instance"}}}"#,
        r#"{"schemaVersion":2,"subject":{"kind":"recipe"},"subject":{"kind":"tool","toolId":"node"},"request":{"snapshotRequestId":"snapshot","intent":{"action":"verify","installationId":"instance"}}}"#,
        r#"{"schemaVersion":2,"subject":{"kind":"tool","toolId":"node"},"request":{"snapshotRequestId":"discarded","snapshotRequestId":"snapshot","intent":{"action":"verify","installationId":"instance"}}}"#,
        r#"{"schemaVersion":2,"subject":{"kind":"recipe"},"request":{"projectPath":"/fixture/project","binding":{"scope":"project","strategy":"direct"},"items":[{"toolId":"node","version":"22.1.0","snapshotRequestId":"snapshot","selection":{"kind":"reuse","kind":"install"}}]}}"#,
        r#"{"schemaVersion":2,"subject":{"kind":"recipe"},"request":{"projectPath":"/fixture/project","binding":{"scope":"project","strategy":"direct"},"items":[{"toolId":"node","version":"22.1.0","snapshotRequestId":"snapshot","selection":{"kind":"reuse","installationId":"discarded","installationId":"instance"}}]}}"#,
    ];
    for raw in raw_requests {
        assert!(
            serde_json::from_str::<AnyPlanRequestV2>(raw).is_err(),
            "duplicate key must not be folded into a valid request: {raw}"
        );
    }
}

#[test]
fn unique_json_decode_preserves_serde_json_numbers_and_accepts_field_reordering() {
    for raw in [
        "9007199254740993",
        "18446744073709551615",
        "-9223372036854775808",
        "1.0",
        "-0.0",
        r#"[{"exact":18446744073709551615,"nested":[null,true,"text",1.5]}]"#,
    ] {
        let expected: Value = serde_json::from_str(raw).unwrap();
        let actual: UniqueJsonValue = serde_json::from_str(raw).unwrap();
        assert_eq!(actual.0, expected, "numeric semantics changed for {raw}");
    }
    let reordered = r#"{"request":{"intent":{"installationId":"instance","action":"verify"},"snapshotRequestId":"snapshot"},"subject":{"toolId":"node","kind":"tool"},"schemaVersion":2}"#;
    assert!(matches!(
        serde_json::from_str::<AnyPlanRequestV2>(reordered).unwrap(),
        AnyPlanRequestV2::Tool(_)
    ));
}

#[test]
fn valid_recipe_is_disabled_without_calling_tool_preview_or_creating_journal() {
    let directory = tempfile::tempdir().unwrap();
    let journal = directory.path().join("must-not-create/operations.json");
    let service = OperationPlanService::new(journal.clone());
    let query = InventoryQueryService::default();
    for sample in contract()["requests"].as_object().unwrap().values() {
        let request: AnyPlanRequestV2 = serde_json::from_value(sample.clone()).unwrap();
        let mut calls = 0;
        let result = service.preview_any_v2_using(request.clone(), |_| {
            calls += 1;
            panic!("Recipe must not reach environment, inventory, or manager probes");
        });
        assert!(matches!(result, Err(PlanError::Disabled)));
        assert_eq!(calls, 0);
        assert!(matches!(
            service.preview_any_v2_current(&query, request),
            Err(PlanError::Disabled)
        ));
    }
    assert!(!journal.parent().unwrap().exists());
    let state = super::super::lock(&service.state);
    assert!(state.plans.is_empty());
    assert!(!state.initialized);
    assert!(!state.journal_failed);
}

#[test]
fn programmatic_invalid_requests_stop_before_preview_and_plan_validators_reject_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let service = OperationPlanService::new(directory.path().join("unused/journal.json"));
    let request: RecipePlanRequestV2 =
        serde_json::from_value(contract()["requests"]["mixed"].clone()).unwrap();
    let mut invalid = Vec::new();
    let mut value = request.clone();
    value.schema_version = 3;
    invalid.push(value);
    let mut value = request.clone();
    value.request.items.push(value.request.items[0].clone());
    invalid.push(value);
    let mut value = request.clone();
    value.request.items[1].tool_id = InventoryTool::Node;
    invalid.push(value);
    let mut value = request.clone();
    value.request.items[0].version = "latest".into();
    invalid.push(value);
    let mut value = request.clone();
    value.request.project_path = "/fixture/../project".into();
    invalid.push(value);
    for value in invalid {
        assert!(value.validate().is_err());
        let mut calls = 0;
        let result = service.preview_any_v2_using(AnyPlanRequestV2::Recipe(value), |_| {
            calls += 1;
            panic!("invalid Recipe must stop before I/O");
        });
        assert!(matches!(result, Err(PlanError::Unsupported)));
        assert_eq!(calls, 0);
    }
    let plan: RecipeOperationPlanV2 =
        serde_json::from_value(contract()["plans"]["mixed"].clone()).unwrap();
    let mut value = plan.clone();
    value.schema_version = 3;
    assert!(value.validate().is_err());
    let mut value = plan.clone();
    value.plan.capability.can_execute = true;
    assert!(value.validate().is_err());
    let mut value = plan.clone();
    value.plan.items[0].provider_package.provider_id = ProviderId::Brew;
    assert!(value.validate().is_err());
    let mut value = plan.clone();
    value.plan.configuration.path = "/fixture/global.toml".into();
    assert!(value.validate().is_err());
    let mut value = plan.clone();
    value.plan.steps[1].item_index = Some(1);
    assert!(value.validate().is_err());
    let mut value = plan;
    value.step_graph[1].depends_on.clear();
    assert!(value.validate().is_err());
}

#[test]
fn exact_input_budgets_are_utf8_and_match_existing_tool_version_policies() {
    let base = contract()["requests"]["nodeInstall"].clone();
    for path in [
        format!("/{}", "a".repeat(4095)),
        format!("/{}a", "中".repeat(1364)),
    ] {
        let mut value = base.clone();
        value["request"]["projectPath"] = json!(path);
        assert!(serde_json::from_value::<RecipePlanRequestV2>(value).is_ok());
    }
    let mut value = base.clone();
    value["request"]["items"][0]["snapshotRequestId"] = json!("a".repeat(128));
    assert!(serde_json::from_value::<RecipePlanRequestV2>(value).is_ok());
    for version in ["0.0.0", "9999999999.9999999999.9999999999"] {
        let mut value = base.clone();
        value["request"]["items"][0]["version"] = json!(version);
        assert!(serde_json::from_value::<RecipePlanRequestV2>(value).is_ok());
    }
    for version in ["3.8.0", "3.99.9999"] {
        let mut value = contract()["requests"]["pythonReuse"].clone();
        value["request"]["items"][0]["version"] = json!(version);
        assert!(serde_json::from_value::<RecipePlanRequestV2>(value).is_ok());
    }
}
