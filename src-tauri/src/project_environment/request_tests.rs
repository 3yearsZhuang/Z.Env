use super::*;

fn report(state: RequirementState, requirements: &[&str]) -> ProjectEnvironment {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/project-environment/contract-v1.json"
    ))
    .unwrap();
    let mut report: ProjectEnvironment = serde_json::from_value(fixture["report"].clone()).unwrap();
    report.requirement_state = state;
    report.declarations = requirements
        .iter()
        .enumerate()
        .map(|(index, requirement)| Declaration {
            source: format!("declaration-{index}"),
            path: format!("/fixture/project/declaration-{index}"),
            requirement: Some((*requirement).into()),
            supported: true,
            reason: None,
        })
        .collect();
    report.candidates.clear();
    report
}

#[test]
fn exact_request_matches_the_read_intersection_without_any_installed_candidate() {
    let report = report(
        RequirementState::Supported,
        &[">=20 <24", "22 || 20", ">=22 <23"],
    );
    let before = serde_json::to_value(&report).unwrap();
    assert!(report.candidates.is_empty());
    assert_eq!(match_node_request(&report, "22.1.0"), Match::Satisfied);
    assert_eq!(match_node_request(&report, "20.1.0"), Match::Mismatch);
    assert_eq!(match_node_request(&report, "24.0.0"), Match::Mismatch);
    assert_eq!(serde_json::to_value(report).unwrap(), before);
}

#[test]
fn only_a_truly_unspecified_report_allows_an_unconstrained_exact_request() {
    let unspecified = report(RequirementState::Unspecified, &[]);
    assert_eq!(match_node_request(&unspecified, "22.1.0"), Match::Satisfied);
    let contradictory = report(RequirementState::Unspecified, &["22"]);
    assert_eq!(match_node_request(&contradictory, "22.1.0"), Match::Unknown);
    let unknown = report(RequirementState::Unknown, &[]);
    assert_eq!(match_node_request(&unknown, "22.1.0"), Match::Unknown);
    let conflict = report(RequirementState::Conflict, &[">=22", "<20"]);
    assert_eq!(match_node_request(&conflict, "22.1.0"), Match::Mismatch);
    let inconsistent_supported = report(RequirementState::Supported, &[">=22", "<20"]);
    assert_eq!(
        match_node_request(&inconsistent_supported, "22.1.0"),
        Match::Mismatch
    );
}

#[test]
fn unsupported_missing_or_over_budget_declarations_never_gain_request_authority() {
    let baseline = report(RequirementState::Supported, &[">=22 <23"]);
    for variant in 0..6 {
        let mut report = baseline.clone();
        match variant {
            0 => report.declarations.clear(),
            1 => report.declarations[0].supported = false,
            2 => report.declarations[0].reason = Some("文件不可完整读取".into()),
            3 => report.declarations[0].requirement = None,
            4 => report.declarations[0].requirement = Some("lts/*".into()),
            _ => report.declarations = vec![report.declarations[0].clone(); 65],
        }
        assert_eq!(
            match_node_request(&report, "22.1.0"),
            Match::Unknown,
            "variant {variant}"
        );
    }
}

#[test]
fn request_parser_keeps_aliases_partial_versions_and_u32_overflow_unknown_even_without_declarations(
) {
    for report in [
        report(RequirementState::Unspecified, &[]),
        report(RequirementState::Supported, &["*"]),
    ] {
        for version in [
            "",
            "22",
            "22.1",
            "v22.1.0",
            "latest",
            "^22.1.0",
            "22.1.0-beta",
            "022.1.0",
            "22.1.0\n",
            "4294967296.1.0",
            "1.4294967296.0",
            "1.0.4294967296",
        ] {
            assert_eq!(
                match_node_request(&report, version),
                Match::Unknown,
                "{version}"
            );
        }
        assert_eq!(
            match_node_request(&report, "4294967295.0.0"),
            Match::Satisfied
        );
    }
}
