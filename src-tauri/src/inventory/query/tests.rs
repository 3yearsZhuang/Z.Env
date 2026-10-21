use std::sync::mpsc;
use std::thread;

use super::*;
use crate::domain::{
    Health, InstallationObservation, Ownership, PackageKind, Platform, ProviderPackage,
};

fn observation(provider: ProviderId, locator: &str, version: &str) -> InstallationObservation {
    InstallationObservation {
        provider_package: ProviderPackage {
            provider_id: provider,
            package_id: match provider {
                ProviderId::Brew => "node@22",
                _ => "node",
            }
            .to_owned(),
            kind: match provider {
                ProviderId::Brew => PackageKind::Formula,
                ProviderId::Path => PackageKind::Executable,
                _ => PackageKind::Runtime,
            },
            platform: Platform::Macos,
        },
        tool_id: ToolId::from("node"),
        locator: locator.to_owned(),
        executable_path: Some(format!("{locator}/bin/node")),
        canonical_executable_path: Some(format!("{locator}/{version}/node")),
        version: Some(version.to_owned()),
        architecture: Some("arm64".to_owned()),
        health: Health::Healthy,
        ownership: if provider == ProviderId::Path {
            Ownership::Observed
        } else {
            Ownership::Owned
        },
        evidence: Vec::new(),
    }
}

fn scan(provider: ProviderId, items: &[(&str, &str)]) -> ProviderScan {
    ProviderScan {
        report: ProviderReport {
            provider_id: provider,
            status: ProviderStatus::Ok,
            observed_at: 100,
            stale: false,
            coverage: ScanCoverage {
                tool_ids: vec![ToolId::from("node")],
                complete: true,
                truncated: false,
            },
            error: None,
        },
        observations: items
            .iter()
            .map(|(locator, version)| observation(provider, locator, version))
            .collect(),
    }
}

fn failed_scan(provider: ProviderId, items: &[(&str, &str)]) -> ProviderScan {
    let mut scan = scan(provider, items);
    scan.report.observed_at = 200;
    scan.report.status = ProviderStatus::Timeout;
    scan.report.coverage.complete = false;
    scan.report.error = Some(ProviderError {
        code: "providerTimeout".to_owned(),
        message: "来源扫描超时".to_owned(),
        retryable: true,
    });
    scan
}

fn complete_scans() -> Vec<ProviderScan> {
    PROVIDERS
        .into_iter()
        .map(|provider| scan(provider, &[("/old", "22.1.0")]))
        .collect()
}

fn execute(
    service: &InventoryQueryService,
    id: &str,
    provider: Option<ProviderId>,
    scans: Vec<ProviderScan>,
) -> InventorySnapshot {
    service
        .begin(id.to_owned(), provider)
        .unwrap()
        .execute_with(|requested, _| {
            assert_eq!(requested, provider);
            scans
        })
        .unwrap()
}

#[test]
fn request_ids_are_restricted_and_recent_reuse_is_rejected() {
    let service = InventoryQueryService::default();
    for id in ["", "space id", "中文", "path/id", "a.b", &"a".repeat(129)] {
        assert_eq!(
            service.begin(id.to_owned(), None).err(),
            Some(QueryError::InvalidRequestId)
        );
        assert_eq!(service.cancel(id), Err(QueryError::InvalidRequestId));
    }
    drop(service.begin("Valid_01-request".to_owned(), None).unwrap());
    assert_eq!(
        service.begin("Valid_01-request".to_owned(), None).err(),
        Some(QueryError::ReusedRequestId)
    );
    assert!(!service.cancel("Valid_01-request").unwrap());
}

#[test]
fn registered_and_running_queries_share_one_global_slot() {
    let service = InventoryQueryService::default();
    let first = service.begin("one".to_owned(), None).unwrap();
    assert_eq!(
        service
            .clone()
            .begin("two".to_owned(), Some(ProviderId::Nvm))
            .err(),
        Some(QueryError::Busy)
    );
    assert_eq!(
        service.begin("one".to_owned(), None).err(),
        Some(QueryError::ReusedRequestId)
    );
    drop(first);
    drop(service.begin("two".to_owned(), None).unwrap());
}

#[test]
fn project_read_holds_the_same_slot_honors_cancel_and_never_recommits_inventory() {
    let service = InventoryQueryService::default();
    let snapshot = execute(&service, "seed", None, complete_scans());
    let before = lock_state(&service.state).caches[&InventoryTool::Node]
        .committed
        .clone();
    let lease = service.begin("project-read".into(), None).unwrap();
    let result = lease.read_snapshot(&snapshot.request_id, |current, token| {
        assert_eq!(current.request_id, "seed");
        assert_eq!(
            service.begin("overlap".into(), None).err(),
            Some(QueryError::Busy)
        );
        service.cancel("project-read").unwrap();
        assert!(token.is_cancelled());
        42
    });
    assert_eq!(result, Err(QueryError::Cancelled));
    assert_eq!(
        lock_state(&service.state).caches[&InventoryTool::Node].committed,
        before
    );
    drop(service.begin("after-project".into(), None).unwrap());
}

#[test]
fn mutation_reserves_query_slot_and_replaces_only_the_reconciled_source() {
    let service = InventoryQueryService::default();
    execute(&service, "seed", None, complete_scans());
    let lease = service
        .begin("mutation".into(), Some(ProviderId::Mise))
        .unwrap();
    lease.mark_mutating().unwrap();
    assert_eq!(
        service.begin("late-query".into(), None).err(),
        Some(QueryError::Busy)
    );
    let state = lock_state(&service.state);
    let snapshot = &state.caches[&InventoryTool::Node]
        .committed
        .as_ref()
        .unwrap()
        .1;
    assert!(snapshot
        .installations
        .iter()
        .filter(|i| i.provider_package.provider_id == ProviderId::Mise)
        .all(|i| i.stale));
    assert!(snapshot
        .installations
        .iter()
        .filter(|i| i.provider_package.provider_id != ProviderId::Mise)
        .all(|i| !i.stale));
    drop(state);
    let snapshot = lease
        .execute_with(|_, _| vec![scan(ProviderId::Mise, &[])])
        .unwrap();
    assert!(!snapshot
        .installations
        .iter()
        .any(|i| i.provider_package.provider_id == ProviderId::Mise));
    assert_eq!(snapshot.installations.len(), 3);
    drop(service.begin("after-mutation".into(), None).unwrap());
}

#[test]
fn abandoning_mutation_keeps_old_records_explicitly_stale_until_complete_rescan() {
    let service = InventoryQueryService::default();
    execute(&service, "seed", None, complete_scans());
    let lease = service
        .begin("mutation".into(), Some(ProviderId::Mise))
        .unwrap();
    lease.mark_mutating().unwrap();
    drop(lease);
    let (_, snapshot) = service.current_snapshot().unwrap();
    let report = snapshot
        .providers
        .iter()
        .find(|r| r.provider_id == ProviderId::Mise)
        .unwrap();
    assert!(report.stale && !report.coverage.complete);
    assert_eq!(report.error.as_ref().unwrap().code, "mutationPending");
    assert!(
        snapshot
            .installations
            .iter()
            .find(|i| i.provider_package.provider_id == ProviderId::Mise)
            .unwrap()
            .stale
    );
    let snapshot = execute(
        &service,
        "recovery",
        Some(ProviderId::Mise),
        vec![scan(ProviderId::Mise, &[])],
    );
    assert!(!snapshot
        .installations
        .iter()
        .any(|i| i.provider_package.provider_id == ProviderId::Mise));
}

#[test]
fn cancellation_before_registration_prevents_the_query_from_starting() {
    let service = InventoryQueryService::default();
    assert!(service.cancel("early").unwrap());
    assert!(service.cancel("early").unwrap());
    assert_eq!(
        service.begin("early".to_owned(), None).err(),
        Some(QueryError::Cancelled)
    );
    assert_eq!(
        service.begin("early".to_owned(), None).err(),
        Some(QueryError::ReusedRequestId)
    );
    let lease = service.begin("registered".to_owned(), None).unwrap();
    assert!(service.cancel("registered").unwrap());
    assert_eq!(
        lease.execute_with(|_, _| panic!("已取消租约不能启动扫描")),
        Err(QueryError::Cancelled)
    );
    drop(service.begin("after".to_owned(), None).unwrap());
}

#[test]
fn cancellation_is_scoped_and_does_not_release_running_workers_early() {
    let service = InventoryQueryService::default();
    let lease = service.begin("running".to_owned(), None).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        lease.execute_with(|_, cancellation| {
            entered_tx.send(cancellation.clone()).unwrap();
            release_rx.recv().unwrap();
            Vec::new()
        })
    });
    let token = entered_rx.recv().unwrap();
    service.cancel("different").unwrap();
    assert!(!token.is_cancelled());
    service.clone().cancel("running").unwrap();
    assert!(token.is_cancelled());
    assert_eq!(
        service.begin("replacement".to_owned(), None).err(),
        Some(QueryError::Busy)
    );
    release_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), Err(QueryError::Cancelled));
    assert!(lock_state(&service.state)
        .caches
        .values()
        .all(|c| c.scans.is_empty()));
    drop(service.begin("replacement".to_owned(), None).unwrap());
    assert_eq!(
        service.begin("different".to_owned(), None).err(),
        Some(QueryError::Cancelled)
    );
}

#[test]
fn panic_or_merge_error_releases_the_lease_without_committing_results() {
    let service = InventoryQueryService::default();
    let lease = service.begin("panic".to_owned(), None).unwrap();
    assert!(std::panic::catch_unwind(|| {
        lease.execute_with(|_, _| panic!("模拟来源意外退出"))
    })
    .is_err());
    let mut invalid = scan(ProviderId::Nvm, &[("/invalid", "22.1.0")]);
    invalid.observations[0].provider_package.provider_id = ProviderId::Brew;
    assert!(matches!(
        service
            .begin("merge-error".to_owned(), Some(ProviderId::Nvm))
            .unwrap()
            .execute_with(|_, _| vec![invalid]),
        Err(QueryError::Merge(MergeError::ProviderMismatch))
    ));
    assert!(lock_state(&service.state)
        .caches
        .values()
        .all(|c| c.scans.is_empty()));
    drop(service.begin("after-error".to_owned(), None).unwrap());
}

#[test]
fn source_scope_must_match_before_any_cache_commit() {
    let service = InventoryQueryService::default();
    for (id, provider, scans) in [
        ("missing", None, vec![scan(ProviderId::Nvm, &[])]),
        (
            "wrong",
            Some(ProviderId::Nvm),
            vec![scan(ProviderId::Mise, &[])],
        ),
        (
            "duplicate",
            Some(ProviderId::Nvm),
            vec![scan(ProviderId::Nvm, &[]), scan(ProviderId::Nvm, &[])],
        ),
    ] {
        assert!(matches!(
            service
                .begin(id.to_owned(), provider)
                .unwrap()
                .execute_with(|_, _| scans),
            Err(QueryError::InvalidScan(_))
        ));
    }
    assert!(lock_state(&service.state)
        .caches
        .values()
        .all(|c| c.scans.is_empty()));
}

#[test]
fn first_targeted_scan_reports_other_sources_as_unknown() {
    let service = InventoryQueryService::default();
    let snapshot = execute(
        &service,
        "targeted",
        Some(ProviderId::Nvm),
        vec![scan(ProviderId::Nvm, &[])],
    );
    assert_eq!(snapshot.providers.len(), 4);
    for report in &snapshot.providers {
        if report.provider_id == ProviderId::Nvm {
            assert!(super::super::has_complete_coverage(
                report,
                &ToolId::from("node")
            ));
        } else {
            assert_eq!(report.status, ProviderStatus::Error);
            assert!(!report.coverage.complete);
            assert_eq!(report.error.as_ref().unwrap().code, "notScanned");
            assert!(!super::super::has_complete_coverage(
                report,
                &ToolId::from("node")
            ));
        }
    }
    assert!(snapshot.installations.is_empty());
    assert!(snapshot.observed_at <= MAX_SAFE_TIMESTAMP);
}

#[test]
fn targeted_retry_preserves_every_other_source() {
    let service = InventoryQueryService::default();
    let first = execute(&service, "first", None, complete_scans());
    let refreshed = execute(
        &service,
        "retry-nvm",
        Some(ProviderId::Nvm),
        vec![scan(ProviderId::Nvm, &[("/new", "24.1.0")])],
    );
    for provider in [ProviderId::Mise, ProviderId::Brew, ProviderId::Path] {
        assert_eq!(
            first
                .providers
                .iter()
                .find(|item| item.provider_id == provider),
            refreshed
                .providers
                .iter()
                .find(|item| item.provider_id == provider)
        );
        assert_eq!(
            first
                .installations
                .iter()
                .filter(|item| item.provider_package.provider_id == provider)
                .collect::<Vec<_>>(),
            refreshed
                .installations
                .iter()
                .filter(|item| item.provider_package.provider_id == provider)
                .collect::<Vec<_>>()
        );
    }
    let nvm: Vec<_> = refreshed
        .installations
        .iter()
        .filter(|item| item.provider_package.provider_id == ProviderId::Nvm)
        .collect();
    assert_eq!(nvm.len(), 1);
    assert_eq!(nvm[0].locator, "/new");
    assert!(!nvm[0].stale);
}

#[test]
fn failed_refresh_keeps_previous_data_stale_and_latest_error_report() {
    let service = InventoryQueryService::default();
    execute(&service, "first", None, complete_scans());
    let snapshot = execute(
        &service,
        "failed",
        Some(ProviderId::Nvm),
        vec![failed_scan(ProviderId::Nvm, &[])],
    );
    let report = snapshot
        .providers
        .iter()
        .find(|report| report.provider_id == ProviderId::Nvm)
        .unwrap();
    assert_eq!(report.status, ProviderStatus::Timeout);
    assert_eq!(report.observed_at, 200);
    assert!(report.stale);
    assert!(!report.coverage.complete);
    assert_eq!(report.error.as_ref().unwrap().code, "providerTimeout");
    let old = snapshot
        .installations
        .iter()
        .find(|item| item.provider_package.provider_id == ProviderId::Nvm)
        .unwrap();
    assert_eq!(old.locator, "/old");
    assert!(old.stale);
}

#[test]
fn partial_refresh_retains_missing_locators_without_reviving_replaced_identity() {
    let service = InventoryQueryService::default();
    execute(
        &service,
        "seed",
        Some(ProviderId::Nvm),
        vec![scan(
            ProviderId::Nvm,
            &[("/same", "22.1.0"), ("/missing", "20.1.0")],
        )],
    );
    let partial = failed_scan(ProviderId::Nvm, &[("/same", "24.1.0")]);
    let expected_current = partial.observations[0].clone();
    let snapshot = execute(&service, "partial", Some(ProviderId::Nvm), vec![partial]);
    assert_eq!(snapshot.installations.len(), 2);
    let current = snapshot
        .installations
        .iter()
        .find(|item| item.locator == "/same")
        .unwrap();
    assert_eq!(current.version.as_deref(), Some("24.1.0"));
    assert_eq!(
        current.canonical_executable_path,
        expected_current.canonical_executable_path
    );
    assert!(snapshot
        .installations
        .iter()
        .all(|item| item.version.as_deref() != Some("22.1.0")));
    assert!(snapshot.installations.iter().all(|item| item.stale));
}

#[test]
fn incomplete_ok_report_retains_missing_data_and_complete_empty_removes_it() {
    for truncated in [false, true] {
        let service = InventoryQueryService::default();
        execute(
            &service,
            "seed",
            Some(ProviderId::Nvm),
            vec![scan(ProviderId::Nvm, &[("/old", "22.1.0")])],
        );
        let mut partial = scan(ProviderId::Nvm, &[]);
        partial.report.coverage.complete = truncated;
        partial.report.coverage.truncated = truncated;
        let partial = execute(&service, "partial", Some(ProviderId::Nvm), vec![partial]);
        assert_eq!(partial.installations.len(), 1);
        assert!(partial.installations[0].stale);
        let empty = execute(
            &service,
            "empty",
            Some(ProviderId::Nvm),
            vec![scan(ProviderId::Nvm, &[])],
        );
        assert!(empty.installations.is_empty());
        let report = empty
            .providers
            .iter()
            .find(|report| report.provider_id == ProviderId::Nvm)
            .unwrap();
        assert!(super::super::has_complete_coverage(
            report,
            &ToolId::from("node")
        ));
    }
}

#[test]
fn cancelled_refresh_never_overwrites_cache_and_next_request_gets_a_new_token() {
    let service = InventoryQueryService::default();
    execute(&service, "seed", None, complete_scans());
    let before = lock_state(&service.state).caches[&InventoryTool::Node]
        .scans
        .clone();
    let lease = service
        .begin("cancelled".to_owned(), Some(ProviderId::Nvm))
        .unwrap();
    let old_token = lease.cancellation.clone();
    let result = lease.execute_with(|_, _| {
        service.cancel("cancelled").unwrap();
        vec![scan(ProviderId::Nvm, &[("/must-not-commit", "24.1.0")])]
    });
    assert_eq!(result, Err(QueryError::Cancelled));
    assert_eq!(
        lock_state(&service.state).caches[&InventoryTool::Node].scans,
        before
    );
    let lease = service
        .begin("fresh-token".to_owned(), Some(ProviderId::Nvm))
        .unwrap();
    assert!(old_token.is_cancelled());
    assert!(!lease.cancellation.is_cancelled());
    let snapshot = lease
        .execute_with(|_, _| vec![scan(ProviderId::Nvm, &[("/latest", "24.1.0")])])
        .unwrap();
    assert!(snapshot
        .installations
        .iter()
        .any(|item| item.locator == "/latest"));
    assert!(!snapshot
        .installations
        .iter()
        .any(|item| item.locator == "/must-not-commit"));
}

#[test]
fn obsolete_lease_cannot_commit_or_release_a_newer_generation() {
    let service = InventoryQueryService::default();
    let obsolete = service.begin("obsolete".to_owned(), None).unwrap();
    {
        // 模拟将来增加恢复机制时旧线程迟到，仍须由身份与代数共同守住提交边界。
        lock_state(&service.state).active = None;
    }
    let latest = service.begin("latest".to_owned(), None).unwrap();
    assert_eq!(
        obsolete.execute_with(|_, _| complete_scans()),
        Err(QueryError::Superseded)
    );
    let state = lock_state(&service.state);
    assert_eq!(state.active.as_ref().unwrap().request_id, "latest");
    assert!(state.caches[&InventoryTool::Node].scans.is_empty());
    drop(state);
    latest.execute_with(|_, _| complete_scans()).unwrap();
}

#[test]
fn request_history_and_pre_registration_cancellations_are_bounded() {
    let service = InventoryQueryService::default();
    for index in 0..(REQUEST_HISTORY_LIMIT + 10) {
        drop(service.begin(format!("done-{index}"), None).unwrap());
    }
    for index in 0..(PENDING_CANCELLATION_LIMIT + 10) {
        service.cancel(&format!("pending-{index}")).unwrap();
    }
    let state = lock_state(&service.state);
    assert_eq!(state.request_history.len(), REQUEST_HISTORY_LIMIT);
    assert_eq!(
        state.pending_cancellations.len(),
        PENDING_CANCELLATION_LIMIT
    );
    assert!(!state.request_history.contains(&"done-0".to_owned()));
    assert!(!state
        .pending_cancellations
        .contains(&"pending-0".to_owned()));
}

#[test]
fn changing_incomplete_batches_keep_every_fresh_record_with_a_bounded_cache() {
    let service = InventoryQueryService::default();
    let mise = scan(ProviderId::Mise, &[("/mise", "22.1.0")]);
    execute(
        &service,
        "mise-seed",
        Some(ProviderId::Mise),
        vec![mise.clone()],
    );
    for batch in 0..6 {
        let mut fresh = scan(ProviderId::Nvm, &[]);
        fresh.report.coverage.complete = false;
        fresh.observations = (0..CACHED_OBSERVATION_LIMIT / 2)
            .map(|index| {
                observation(
                    ProviderId::Nvm,
                    &format!("/batch-{batch}/{index:03}"),
                    "24.1.0",
                )
            })
            .collect();
        let expected = fresh.observations.clone();
        let snapshot = execute(
            &service,
            &format!("incomplete-{batch}"),
            Some(ProviderId::Nvm),
            vec![fresh],
        );
        let state = lock_state(&service.state);
        let cached = &state.caches[&InventoryTool::Node].scans[&ProviderId::Nvm];
        assert!(cached.observations.len() <= CACHED_OBSERVATION_LIMIT);
        assert!(expected
            .iter()
            .all(|observation| cached.observations.contains(observation)));
        assert_eq!(
            state.caches[&InventoryTool::Node].scans[&ProviderId::Mise],
            mise
        );
        if batch >= 2 {
            assert!(cached.report.stale);
            assert!(!cached.report.coverage.complete);
            assert!(cached.report.coverage.truncated);
            assert!(!super::super::has_complete_coverage(
                &cached.report,
                &ToolId::from("node")
            ));
        }
        assert!(snapshot.installations.len() <= CACHED_OBSERVATION_LIMIT + 1);
    }
    let empty = execute(
        &service,
        "complete-empty",
        Some(ProviderId::Nvm),
        vec![scan(ProviderId::Nvm, &[])],
    );
    assert_eq!(empty.installations.len(), 1);
    assert_eq!(
        empty.installations[0].provider_package.provider_id,
        ProviderId::Mise
    );
    assert!(
        lock_state(&service.state).caches[&InventoryTool::Node].scans[&ProviderId::Nvm]
            .observations
            .is_empty()
    );
}

#[test]
fn capped_history_selection_is_deterministic_and_marks_dropped_history_unknown() {
    let mut previous = scan(ProviderId::Nvm, &[]);
    previous.observations = (0..CACHED_OBSERVATION_LIMIT)
        .map(|index| observation(ProviderId::Nvm, &format!("/old/{index:03}"), "22.1.0"))
        .collect();
    let mut fresh = failed_scan(ProviderId::Nvm, &[]);
    fresh.observations = (0..CACHED_OBSERVATION_LIMIT / 2)
        .map(|index| observation(ProviderId::Nvm, &format!("/fresh/{index:03}"), "24.1.0"))
        .collect();
    let expected = refresh_source(Some(&previous), fresh.clone());
    previous.observations.reverse();
    let reordered = refresh_source(Some(&previous), fresh.clone());
    assert_eq!(expected, reordered);
    assert_eq!(expected.observations.len(), CACHED_OBSERVATION_LIMIT);
    assert!(fresh
        .observations
        .iter()
        .all(|observation| expected.observations.contains(observation)));
    assert!(expected.report.stale);
    assert!(expected.report.coverage.truncated);
    assert!(!expected.report.coverage.complete);
    assert_eq!(expected.report.status, fresh.report.status);
    assert_eq!(expected.report.error, fresh.report.error);
    assert_eq!(expected.report.observed_at, fresh.report.observed_at);
}

#[test]
fn oversized_fresh_source_is_rejected_without_losing_cached_results() {
    let service = InventoryQueryService::default();
    execute(&service, "seed", None, complete_scans());
    let previous = lock_state(&service.state).caches[&InventoryTool::Node]
        .scans
        .clone();
    let mut oversized = scan(ProviderId::Nvm, &[]);
    oversized.observations = (0..=CACHED_OBSERVATION_LIMIT)
        .map(|index| observation(ProviderId::Nvm, &format!("/large/{index:03}"), "24.1.0"))
        .collect();
    assert!(matches!(
        service
            .begin("oversized".to_owned(), Some(ProviderId::Nvm))
            .unwrap()
            .execute_with(|_, _| vec![oversized]),
        Err(QueryError::InvalidScan(_))
    ));
    assert_eq!(
        lock_state(&service.state).caches[&InventoryTool::Node].scans,
        previous
    );
    drop(service.begin("after-oversized".to_owned(), None).unwrap());
}

#[test]
fn malformed_observation_rolls_back_all_sources_before_the_next_targeted_refresh() {
    let service = InventoryQueryService::default();
    execute(&service, "seed", None, complete_scans());
    let previous = lock_state(&service.state).caches[&InventoryTool::Node]
        .scans
        .clone();
    let mut fresh: Vec<_> = PROVIDERS
        .into_iter()
        .map(|provider| scan(provider, &[("/new-full-refresh", "24.1.0")]))
        .collect();
    // 来源报告范围合法仍不足以提交，末来源的观测不得冒充其它来源。
    fresh.last_mut().unwrap().observations[0]
        .provider_package
        .provider_id = ProviderId::Nvm;
    assert_eq!(
        service
            .begin("malformed-full".to_owned(), None)
            .unwrap()
            .execute_with(|_, _| fresh),
        Err(QueryError::Merge(MergeError::ProviderMismatch))
    );
    assert_eq!(
        lock_state(&service.state).caches[&InventoryTool::Node].scans,
        previous
    );
    execute(
        &service,
        "valid-targeted",
        Some(ProviderId::Nvm),
        vec![scan(ProviderId::Nvm, &[("/valid-targeted", "24.1.0")])],
    );
    let state = lock_state(&service.state);
    for provider in [ProviderId::Mise, ProviderId::Brew, ProviderId::Path] {
        assert_eq!(
            state.caches[&InventoryTool::Node].scans[&provider],
            previous[&provider]
        );
    }
    assert_eq!(
        state.caches[&InventoryTool::Node].scans[&ProviderId::Nvm].observations[0].locator,
        "/valid-targeted"
    );
}

fn python_scans() -> Vec<ProviderScan> {
    crate::providers::python::PROVIDERS
        .into_iter()
        .map(|provider| {
            let mut s = scan(provider, &[("/python", "3.13.8")]);
            s.report.coverage.tool_ids = vec![ToolId::from("python")];
            for o in &mut s.observations {
                o.tool_id = ToolId::from("python");
                o.provider_package.package_id = "python".into();
            }
            s
        })
        .collect()
}
#[test]
fn tool_caches_are_separate_but_share_the_global_read_and_mutation_slot() {
    let service = InventoryQueryService::default();
    let node = execute(&service, "node-before", None, complete_scans());
    let lease = service
        .begin_tool("python-read".into(), None, InventoryTool::Python)
        .unwrap();
    assert!(matches!(
        service.begin("blocked-node".into(), None),
        Err(QueryError::Busy)
    ));
    let python = lease.execute_with(|_, _| python_scans()).unwrap();
    assert_eq!(python.tools[0].tool_id, ToolId::from("python"));
    assert_eq!(service.current_snapshot().unwrap().1, node);
    service
        .begin_tool("python-project".into(), None, InventoryTool::Python)
        .unwrap()
        .read_snapshot("python-read", |snapshot, _| assert_eq!(snapshot, &python))
        .unwrap();
    assert!(matches!(
        service
            .begin("wrong-node-snapshot".into(), None)
            .unwrap()
            .read_snapshot("python-read", |_, _| ()),
        Err(QueryError::NotReady)
    ));
    let state = lock_state(&service.state);
    assert_eq!(state.caches.len(), 2);
    assert_eq!(
        state.caches[&InventoryTool::Node]
            .committed
            .as_ref()
            .unwrap()
            .1
            .request_id,
        "node-before"
    );
}
#[test]
fn python_rejects_foreign_coverage_and_scanner_and_does_not_overwrite_either_cache() {
    let service = InventoryQueryService::default();
    execute(&service, "node-before", None, complete_scans());
    let before = service.current_snapshot().unwrap();
    assert!(service
        .begin_tool(
            "foreign-provider".into(),
            Some(ProviderId::Nvm),
            InventoryTool::Python
        )
        .is_err());
    let bad = service
        .begin_tool("mixed-tools".into(), None, InventoryTool::Python)
        .unwrap()
        .execute_with(|_, _| {
            let mut scans = python_scans();
            scans[0].observations[0].tool_id = ToolId::from("node");
            scans
        });
    assert!(matches!(bad, Err(QueryError::InvalidScan(_))));
    assert_eq!(service.current_snapshot().unwrap(), before);
    assert!(service
        .begin_tool("wrong-scanner".into(), None, InventoryTool::Python)
        .unwrap()
        .execute(NodeScanOptions::from_environment().unwrap())
        .is_err());
    assert_eq!(service.current_snapshot().unwrap(), before);
    assert!(serde_json::from_str::<InventoryTool>("\"ruby\"").is_err());
}
#[test]
fn cancelled_python_read_keeps_old_python_and_node_snapshots() {
    let service = InventoryQueryService::default();
    execute(&service, "node-before", None, complete_scans());
    service
        .begin_tool("python-before".into(), None, InventoryTool::Python)
        .unwrap()
        .execute_with(|_, _| python_scans())
        .unwrap();
    let before = service.current_snapshot().unwrap();
    let lease = service
        .begin_tool("cancel-python".into(), None, InventoryTool::Python)
        .unwrap();
    assert!(service.cancel("cancel-python").unwrap());
    assert!(matches!(
        lease.execute_with(|_, _| panic!("cancelled scanner must not execute")),
        Err(QueryError::Cancelled)
    ));
    assert_eq!(service.current_snapshot().unwrap(), before);
    assert_eq!(
        lock_state(&service.state).caches[&InventoryTool::Python]
            .committed
            .as_ref()
            .unwrap()
            .1
            .request_id,
        "python-before"
    );
}
#[test]
fn python_mise_mutation_invalidates_only_python_source_and_preserves_tool_scope() {
    let service = InventoryQueryService::default();
    execute(&service, "node-seed", None, complete_scans());
    let node = service.current_snapshot().unwrap();
    let lease = service
        .begin_tool(
            "python-mutation".into(),
            Some(ProviderId::Mise),
            InventoryTool::Python,
        )
        .unwrap();
    lease.mark_mutating().unwrap();
    {
        let state = lock_state(&service.state);
        assert_eq!(
            state.caches[&InventoryTool::Node]
                .committed
                .as_ref()
                .unwrap(),
            &node
        );
        let python = &state.caches[&InventoryTool::Python];
        assert_eq!(
            python.scans[&ProviderId::Mise].report.coverage.tool_ids,
            vec![ToolId::from("python")]
        );
        assert!(python.scans[&ProviderId::Mise].report.stale);
    }
    assert!(matches!(
        service.begin("cannot-overlap".into(), None),
        Err(QueryError::Busy)
    ));
    drop(lease);
    let lease = service
        .begin_tool(
            "allowed-python-uv-write".into(),
            Some(ProviderId::Uv),
            InventoryTool::Python,
        )
        .unwrap();
    lease.mark_mutating().unwrap();
    drop(lease);
    let lease = service
        .begin_tool(
            "allowed-python-brew-write".into(),
            Some(ProviderId::Brew),
            InventoryTool::Python,
        )
        .unwrap();
    lease.mark_mutating().unwrap();
    drop(lease);
    let lease = service
        .begin_tool(
            "allowed-python-pyenv-write".into(),
            Some(ProviderId::Pyenv),
            InventoryTool::Python,
        )
        .unwrap();
    lease.mark_mutating().unwrap();
    drop(lease);
    let lease = service
        .begin_tool(
            "forbidden-python-path-write".into(),
            Some(ProviderId::Path),
            InventoryTool::Python,
        )
        .unwrap();
    assert!(matches!(
        lease.mark_mutating(),
        Err(QueryError::InvalidScan(_))
    ));
    drop(lease);
    assert_eq!(service.current_snapshot().unwrap(), node);
}
