use super::*;
use crate::domain::{
    ObservationEvidence, ProviderError, ProviderPackage, ProviderReport, ScanCoverage,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    request_id: String,
    observed_at: u64,
    scans: Vec<ProviderScan>,
    expected_snapshot: serde_json::Value,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/inventory/contract-v1.json")).unwrap()
}

fn merge(scans: Vec<ProviderScan>) -> Result<InventorySnapshot, MergeError> {
    merge_inventory("test".into(), 100, scans)
}

fn scan(provider: ProviderId) -> ProviderScan {
    fixture()
        .scans
        .into_iter()
        .find(|scan| scan.report.provider_id == provider)
        .unwrap()
}

fn first(provider: ProviderId) -> InstallationObservation {
    scan(provider).observations.remove(0)
}

#[test]
fn shared_json_matches_rust_output_and_preserves_unknowns_and_partial_failure() {
    let fixture = fixture();
    let snapshot = merge_inventory(fixture.request_id, fixture.observed_at, fixture.scans).unwrap();
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap(),
        fixture.expected_snapshot
    );
    assert_eq!(snapshot.installations.len(), 4);
    let nvm = snapshot
        .installations
        .iter()
        .find(|item| item.provider_package.provider_id == ProviderId::Nvm)
        .unwrap();
    assert_eq!(nvm.version, None);
    assert_eq!(nvm.health, Health::Broken);
    assert_eq!(nvm.ownership, Ownership::Unknown);
    let mise = &snapshot.providers[0];
    assert_eq!(mise.status, ProviderStatus::Timeout);
    assert!(!has_complete_coverage(mise, &ToolId::from("node")));
}

#[test]
fn input_order_and_repeated_evidence_do_not_change_output() {
    let mut fixture = fixture();
    let expected = merge(fixture.scans.clone()).unwrap();
    fixture.scans.reverse();
    for scan in &mut fixture.scans {
        scan.observations.reverse();
        let duplicates = scan.observations.clone();
        scan.observations.extend(duplicates);
        scan.report.coverage.tool_ids = vec![ToolId::from("nodejs"), ToolId::from("node")];
        for observation in &mut scan.observations {
            let evidence = observation.evidence.clone();
            observation.evidence.extend(evidence);
            observation.evidence.reverse();
        }
    }
    assert_eq!(merge(fixture.scans).unwrap(), expected);
}

#[test]
fn same_version_in_distinct_directories_is_not_deduplicated() {
    let snapshot = merge(vec![scan(ProviderId::Brew)]).unwrap();
    let installs = snapshot.installations;
    assert_eq!(installs.len(), 2);
    assert_eq!(installs[0].version, installs[1].version);
    assert_ne!(installs[0].installation_id, installs[1].installation_id);
    assert_ne!(installs[0].locator, installs[1].locator);
    assert!(snapshot.aliases.is_empty());
}

#[test]
fn alias_keeps_provider_package_and_ownership_of_both_observations() {
    let snapshot = merge(vec![scan(ProviderId::Path), scan(ProviderId::Brew)]).unwrap();
    assert_eq!(snapshot.installations.len(), 3);
    assert_eq!(snapshot.aliases.len(), 1);
    let alias = &snapshot.aliases[0];
    let source = snapshot
        .installations
        .iter()
        .find(|i| i.installation_id == alias.installation_id)
        .unwrap();
    let target = snapshot
        .installations
        .iter()
        .find(|i| i.installation_id == alias.target_installation_id)
        .unwrap();
    assert_eq!(source.provider_package.provider_id, ProviderId::Path);
    assert_eq!(source.ownership, Ownership::Observed);
    assert_eq!(target.provider_package.package_id, "node@22");
    assert_eq!(target.ownership, Ownership::Owned);
}

#[test]
fn conflicting_or_outdated_paths_never_prove_aliases() {
    for change in [
        "version",
        "architecture",
        "platform",
        "stale",
        "broken",
        "noPath",
        "case",
    ] {
        let mut path = scan(ProviderId::Path);
        let source = &mut path.observations[0];
        match change {
            "version" => source.version = Some("20.18.0".into()),
            "architecture" => source.architecture = Some("x86_64".into()),
            "platform" => source.provider_package.platform = Platform::Linux,
            "stale" => path.report.stale = true,
            "broken" => source.health = Health::Broken,
            "noPath" => source.canonical_executable_path = None,
            "case" => {
                source.canonical_executable_path = source
                    .canonical_executable_path
                    .as_ref()
                    .map(|p| p.to_uppercase())
            }
            _ => unreachable!(),
        }
        let snapshot = merge(vec![scan(ProviderId::Brew), path]).unwrap();
        assert!(snapshot.aliases.is_empty(), "{change}");
        assert_eq!(snapshot.installations.len(), 3);
    }
}

#[test]
fn unknown_version_does_not_bridge_conflicting_known_versions_at_one_path() {
    let mut brew = scan(ProviderId::Brew);
    brew.observations.truncate(1);
    let mut conflicting = brew.observations[0].clone();
    conflicting.version = Some("20.18.0".into());
    brew.observations.push(conflicting);
    let mut path = scan(ProviderId::Path);
    path.observations[0].version = None;
    let snapshot = merge(vec![brew, path]).unwrap();
    assert_eq!(snapshot.installations.len(), 3);
    assert!(snapshot.aliases.is_empty());
}

#[test]
fn path_identity_can_be_linked_without_filling_in_unknown_version() {
    let mut path = scan(ProviderId::Path);
    path.observations[0].version = None;
    path.observations[0].architecture = None;
    path.observations[0].health = Health::Unverified;
    let snapshot = merge(vec![scan(ProviderId::Brew), path]).unwrap();
    assert_eq!(snapshot.aliases.len(), 1);
    let path = snapshot.installations.last().unwrap();
    assert_eq!(path.version, None);
    assert_eq!(path.architecture, None);
    assert_eq!(path.health, Health::Unverified);
    assert_eq!(path.ownership, Ownership::Observed);
}

#[test]
fn identity_tracks_target_version_package_architecture_and_preserves_path_case() {
    let original = first(ProviderId::Brew);
    let id = installation_id(&original);
    let mut alias_name = original.clone();
    alias_name.tool_id = ToolId::from("Node.js");
    assert_eq!(id, installation_id(&alias_name));
    for change in [
        "target",
        "version",
        "package",
        "architecture",
        "case",
        "platform",
        "locator",
    ] {
        let mut other = original.clone();
        match change {
            "target" => other.canonical_executable_path = Some("/new/bin/node".into()),
            "version" => other.version = Some("22.12.0".into()),
            "package" => other.provider_package.package_id = "node".into(),
            "architecture" => other.architecture = Some("x86_64".into()),
            "case" => other.locator = other.locator.to_uppercase(),
            "platform" => other.provider_package.platform = Platform::Windows,
            "locator" => other.locator = "/another/bin/node".into(),
            _ => unreachable!(),
        }
        assert_ne!(id, installation_id(&other), "{change}");
    }
    let mut evidence_only = original.clone();
    evidence_only.evidence.clear();
    evidence_only.health = Health::Unverified;
    evidence_only.ownership = Ownership::Unknown;
    assert_eq!(id, installation_id(&evidence_only));
}

#[test]
fn length_prefixed_identity_handles_delimiters_unicode_and_null() {
    let mut left = first(ProviderId::Nvm);
    left.locator = "目录;3:abc".into();
    let mut right = left.clone();
    right.locator = "目录".into();
    right.executable_path = Some("3:abc;".into());
    assert_ne!(installation_id(&left), installation_id(&right));
    right = left.clone();
    right.version = Some(String::new());
    assert_ne!(installation_id(&left), installation_id(&right));
}

#[test]
fn duplicate_observations_preserve_all_evidence_and_take_conservative_health_and_ownership() {
    let mut source = scan(ProviderId::Brew);
    source.observations.truncate(1);
    let mut another = source.observations[0].clone();
    another.health = Health::Broken;
    another.ownership = Ownership::Observed;
    another.evidence.push(ObservationEvidence {
        kind: "probe".into(),
        path: None,
        detail: Some("不可执行".into()),
    });
    source.observations.push(another);
    let forward = merge(vec![source.clone()]).unwrap();
    source.observations.reverse();
    assert_eq!(merge(vec![source]).unwrap(), forward);
    let installation = &forward.installations[0];
    assert_eq!(forward.installations.len(), 1);
    assert_eq!(installation.health, Health::Broken);
    assert_eq!(installation.ownership, Ownership::Unknown);
    assert_eq!(installation.evidence.len(), 2);
}

#[test]
fn all_failure_statuses_retain_other_results_and_partial_observations() {
    for status in [
        ProviderStatus::Unavailable,
        ProviderStatus::Unsupported,
        ProviderStatus::PermissionDenied,
        ProviderStatus::Timeout,
        ProviderStatus::Cancelled,
        ProviderStatus::Error,
    ] {
        let mut failed = scan(ProviderId::Mise);
        failed.report.status = status;
        failed.report.coverage.complete = true;
        let mut partial = first(ProviderId::Brew);
        partial.provider_package = ProviderPackage {
            provider_id: ProviderId::Mise,
            package_id: "node".into(),
            kind: PackageKind::Runtime,
            platform: Platform::Macos,
        };
        failed.observations.push(partial);
        let snapshot = merge(vec![scan(ProviderId::Nvm), failed]).unwrap();
        assert_eq!(snapshot.providers[0].status, status);
        assert_eq!(snapshot.installations.len(), 2);
        assert!(!has_complete_coverage(
            &snapshot.providers[0],
            &ToolId::from("node")
        ));
    }
}

#[test]
fn only_successful_fresh_complete_coverage_is_conclusive_for_its_tool() {
    let report = scan(ProviderId::Brew).report;
    assert!(has_complete_coverage(&report, &ToolId::from("nodejs")));
    assert!(!has_complete_coverage(&report, &ToolId::from("python")));
    let mut stale = report.clone();
    stale.stale = true;
    let mut incomplete = report.clone();
    incomplete.coverage.complete = false;
    let mut truncated = report.clone();
    truncated.coverage.truncated = true;
    let mut errored = report.clone();
    errored.error = Some(ProviderError {
        code: "bad".into(),
        message: "失败".into(),
        retryable: true,
    });
    for report in [stale, incomplete, truncated, errored] {
        assert!(!has_complete_coverage(&report, &ToolId::from("node")));
    }
    let snapshot = merge(vec![ProviderScan {
        report: report.clone(),
        observations: Vec::new(),
    }])
    .unwrap();
    assert!(snapshot.installations.is_empty());
    assert_eq!(snapshot.tools[0].tool_id, ToolId::from("node"));
    assert_eq!(snapshot.providers, vec![report]);
}

#[test]
fn unknown_tools_and_missing_providers_are_not_invented_or_dropped() {
    let mut source = scan(ProviderId::Nvm);
    source.report.coverage.tool_ids = vec![ToolId::from(" CustomTool ")];
    source.observations[0].tool_id = ToolId::from("CustomTool");
    source.observations[0].provider_package.package_id = "custom-runtime".into();
    let snapshot = merge(vec![source]).unwrap();
    assert_eq!(
        snapshot.tools,
        vec![ToolDefinition {
            tool_id: ToolId::from("CustomTool"),
            display_name: "CustomTool".into(),
            aliases: vec![],
            executable_names: vec![],
        }]
    );
    assert_eq!(snapshot.providers.len(), 1);
    assert_eq!(snapshot.installations[0].health, Health::Broken);
    let empty = merge(Vec::new()).unwrap();
    assert!(empty.tools.is_empty() && empty.providers.is_empty() && empty.installations.is_empty());
}

#[test]
fn stale_scan_marks_its_installations_without_hiding_fresh_sources() {
    let mut brew = scan(ProviderId::Brew);
    brew.report.stale = true;
    let snapshot = merge(vec![brew, scan(ProviderId::Path)]).unwrap();
    assert!(snapshot
        .installations
        .iter()
        .filter(|i| i.provider_package.provider_id == ProviderId::Brew)
        .all(|i| i.stale));
    assert!(!snapshot.installations.last().unwrap().stale);
    assert!(snapshot.aliases.is_empty());
}

#[test]
fn malformed_input_is_rejected_instead_of_creating_ambiguous_identities() {
    assert_eq!(
        merge_inventory(" ".into(), 1, Vec::new()),
        Err(MergeError::EmptyRequestId)
    );
    assert_eq!(
        merge_inventory("test".into(), MAX_SAFE_TIMESTAMP + 1, Vec::new()),
        Err(MergeError::InvalidTimestamp)
    );
    assert_eq!(
        merge(vec![scan(ProviderId::Brew), scan(ProviderId::Brew)]),
        Err(MergeError::DuplicateProvider(ProviderId::Brew))
    );
    let mut bad = scan(ProviderId::Brew);
    bad.report.observed_at = MAX_SAFE_TIMESTAMP + 1;
    assert_eq!(merge(vec![bad]), Err(MergeError::InvalidTimestamp));
    let mut bad = scan(ProviderId::Brew);
    bad.observations[0].provider_package.provider_id = ProviderId::Nvm;
    assert_eq!(merge(vec![bad]), Err(MergeError::ProviderMismatch));
    let mut bad = scan(ProviderId::Brew);
    bad.observations[0].tool_id = ToolId::from("python");
    assert_eq!(merge(vec![bad]), Err(MergeError::ToolMismatch));
    let mut bad = scan(ProviderId::Path);
    bad.observations[0].ownership = Ownership::Owned;
    assert_eq!(merge(vec![bad]), Err(MergeError::PathCannotOwnInstallation));
    for invalid in ["", " ", "bad\0value"] {
        let mut bad = scan(ProviderId::Brew);
        bad.observations[0].locator = invalid.into();
        assert_eq!(
            merge(vec![bad]),
            Err(MergeError::InvalidObservation("locator"))
        );
        let mut bad = scan(ProviderId::Brew);
        bad.observations[0].version = Some(invalid.into());
        assert_eq!(
            merge(vec![bad]),
            Err(MergeError::InvalidObservation("version"))
        );
    }
}

#[test]
fn minimal_provider_report_contract_remains_explicit() {
    let report = ProviderReport {
        provider_id: ProviderId::Mise,
        status: ProviderStatus::Unavailable,
        observed_at: 0,
        stale: false,
        coverage: ScanCoverage {
            tool_ids: vec![],
            complete: false,
            truncated: false,
        },
        error: None,
    };
    let snapshot = merge(vec![ProviderScan {
        report,
        observations: vec![],
    }])
    .unwrap();
    let json = serde_json::to_value(snapshot).unwrap();
    assert_eq!(json["providers"][0]["error"], serde_json::Value::Null);
    assert_eq!(json["providers"][0]["status"], "unavailable");
}
