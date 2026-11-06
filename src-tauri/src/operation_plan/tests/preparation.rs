//! Preparation cannot create approvals; signing must revalidate its frozen evidence.
use super::*;
use crate::providers::python::PythonScanOptions;
use std::{fs, os::unix::fs::PermissionsExt};

fn prepare(f: &Fixture) -> PreparedTool {
    OperationPlanService::prepare_node(
        &f.query,
        f.request(Action::Verify),
        f.options.clone(),
        f.config.clone(),
    )
    .unwrap()
}

fn cache_state(f: &Fixture) -> (u64, Vec<serde_json::Value>, bool, bool, usize) {
    let state = lock(&f.service.state);
    (
        state.sequence,
        state
            .plans
            .iter()
            .map(|plan| serde_json::to_value(&plan.view).unwrap())
            .collect(),
        state.initialized,
        state.journal_failed,
        state.journal.records.len(),
    )
}

fn assert_no_journal(f: &Fixture) {
    assert!(!f._dir.path().join("journal").exists());
    assert!(f.service.records().0.is_empty());
}

fn python_options(f: &Fixture) -> PythonScanOptions {
    PythonScanOptions {
        common: f.options.clone(),
        pyenv_root: f._dir.path().join("pyenv"),
        uv_python_dir: f._dir.path().join("uv/python"),
    }
}

fn python_fixture() -> (Fixture, PythonScanOptions, PlanRequest) {
    let f = Fixture::new();
    let options = python_options(&f);
    let executable = options
        .common
        .mise_data_dir
        .join("installs/python/3.13.15/bin/python3");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    let output = serde_json::json!({
        "version": "3.13.15",
        "implementation": "cpython",
        "releaseLevel": "final",
        "freeThreaded": false,
        "debug": false,
        "architecture": "arm64",
        "execPath": executable,
    });
    fs::write(&executable, format!("#!/bin/sh\nprintf '%s' '{output}'\n")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let snapshot = f
        .query
        .begin_tool(
            "preparation-python-baseline".into(),
            Some(ProviderId::Mise),
            InventoryTool::Python,
        )
        .unwrap()
        .execute_python(options.clone())
        .unwrap();
    let installation_id = snapshot
        .installations
        .iter()
        .find(|item| item.health == Health::Healthy)
        .unwrap()
        .installation_id
        .clone();
    let request = PlanRequest {
        snapshot_request_id: snapshot.request_id,
        intent: PlanIntent::Verify { installation_id },
    };
    (f, options, request)
}

#[test]
fn preparing_with_a_full_cache_preserves_every_existing_approval_and_journal_state() {
    let f = Fixture::new();
    for _ in 0..MAX_PLANS {
        f.plan();
    }
    let before = cache_state(&f);
    assert_eq!(before.1.len(), MAX_PLANS);
    for _ in 0..3 {
        let prepared = prepare(&f);
        assert!(prepared.draft.capability.can_execute);
        assert_eq!(cache_state(&f), before);
    }
    let mut invalid = f.request(Action::Verify);
    invalid.snapshot_request_id = "old-preparation-snapshot".into();
    assert!(matches!(
        OperationPlanService::prepare_node(&f.query, invalid, f.options.clone(), f.config.clone(),),
        Err(PlanError::Stale)
    ));
    assert_eq!(cache_state(&f), before);
    assert_no_journal(&f);
}

#[test]
fn signing_rejects_changed_frozen_inputs_without_inserting_an_approval() {
    for change in [
        "configuration",
        "removedConfiguration",
        "absentConfigurationCreated",
        "programReplaced",
        "context",
        "configurationPath",
    ] {
        let f = Fixture::new();
        if change == "absentConfigurationCreated" {
            fs::remove_file(&f.config).unwrap();
        }
        f.plan();
        let prepared = prepare(&f);
        let before = cache_state(&f);
        let mut options = f.options.clone();
        let mut config = f.config.clone();
        match change {
            "configuration" => fs::write(&f.config, "[env]\nCHANGED='yes'\n").unwrap(),
            "removedConfiguration" => fs::remove_file(&f.config).unwrap(),
            "absentConfigurationCreated" => fs::write(&f.config, "[tools]\nnode='20'\n").unwrap(),
            "programReplaced" => {
                let path = &prepared.files[0].0;
                let replacement = path.with_extension("replacement");
                fs::write(&replacement, fs::read(path).unwrap()).unwrap();
                fs::set_permissions(&replacement, fs::metadata(path).unwrap().permissions())
                    .unwrap();
                fs::rename(replacement, path).unwrap();
            }
            "context" => options.path_entries.push(f._dir.path().join("new-bin")),
            "configurationPath" => {
                config = f._dir.path().join("other-config.toml");
                fs::write(&config, fs::read(&f.config).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                f.service.store(prepared, &f.query, &options, &config, None),
                Err(PlanError::Preconditions)
            ),
            "{change}"
        );
        assert_eq!(cache_state(&f), before, "{change}");
        assert_no_journal(&f);
    }
}

#[test]
fn signing_rejects_superseded_or_active_queries_without_changing_the_cache() {
    for change in ["refreshed", "activeNode", "activePython", "mutationPending"] {
        let f = Fixture::new();
        f.plan();
        let prepared = prepare(&f);
        let before = cache_state(&f);
        let held = match change {
            "refreshed" => {
                f.query
                    .begin("preparation-node-refresh".into(), Some(ProviderId::Nvm))
                    .unwrap()
                    .execute(f.options.clone())
                    .unwrap();
                None
            }
            "activeNode" => Some(
                f.query
                    .begin("preparation-active-node".into(), None)
                    .unwrap(),
            ),
            "activePython" => Some(
                f.query
                    .begin_tool(
                        "preparation-active-python".into(),
                        None,
                        InventoryTool::Python,
                    )
                    .unwrap(),
            ),
            "mutationPending" => {
                let lease = f
                    .query
                    .begin("preparation-mutation".into(), Some(ProviderId::Nvm))
                    .unwrap();
                lease.mark_mutating().unwrap();
                drop(lease);
                None
            }
            _ => unreachable!(),
        };
        let result = f
            .service
            .store(prepared, &f.query, &f.options, &f.config, None);
        if held.is_some() {
            assert!(matches!(result, Err(PlanError::Inventory)), "{change}");
        } else {
            assert!(matches!(result, Err(PlanError::Stale)), "{change}");
        }
        assert_eq!(cache_state(&f), before, "{change}");
        assert_no_journal(&f);
        drop(held);
    }
}

#[test]
fn completed_other_tool_queries_do_not_supersede_the_prepared_tool() {
    let f = Fixture::new();
    let prepared = prepare(&f);
    let (generation, snapshot) = f.query.current_snapshot().unwrap();
    f.query
        .begin_tool(
            "preparation-unrelated-python".into(),
            Some(ProviderId::Path),
            InventoryTool::Python,
        )
        .unwrap()
        .execute_python(python_options(&f))
        .unwrap();
    let (current_generation, current) = f.query.current_snapshot().unwrap();
    assert_eq!(current_generation, generation);
    assert_eq!(current.request_id, snapshot.request_id);
    let plan = f
        .service
        .store(prepared, &f.query, &f.options, &f.config, None)
        .unwrap();
    assert_eq!(plan.snapshot_generation, generation.to_string());
    assert_eq!(plan.snapshot_request_id, snapshot.request_id);
    assert!(plan.capability.can_execute);
    assert_no_journal(&f);
}

#[test]
fn cancelled_or_dropped_queries_release_the_slot_without_superseding_committed_evidence() {
    for cancel in [false, true] {
        let f = Fixture::new();
        let prepared = prepare(&f);
        let (generation, snapshot) = f.query.current_snapshot().unwrap();
        let request = "preparation-discarded-query";
        let lease = f
            .query
            .begin(request.into(), Some(ProviderId::Nvm))
            .unwrap();
        if cancel {
            assert!(f.query.cancel(request).unwrap());
        }
        drop(lease);
        let (current_generation, current) = f.query.current_snapshot().unwrap();
        assert_eq!(current_generation, generation);
        assert_eq!(current.request_id, snapshot.request_id);
        let plan = f
            .service
            .store(prepared, &f.query, &f.options, &f.config, None)
            .unwrap();
        assert!(plan.capability.can_execute);
        assert_eq!(plan.snapshot_request_id, snapshot.request_id);
        assert_no_journal(&f);
    }
}

#[test]
fn preview_rereads_context_after_preparation_and_rejects_a_changed_or_failed_supplier() {
    for change in ["context", "configuration", "supplierFailure"] {
        let f = Fixture::new();
        let before = cache_state(&f);
        let mut calls = 0;
        let result = f
            .service
            .preview_node_using(&f.query, f.request(Action::Verify), || {
                calls += 1;
                let mut options = f.options.clone();
                if calls == 2 {
                    match change {
                        "context" => options.nvm_dir = f._dir.path().join("other-nvm"),
                        "configuration" => fs::write(&f.config, "[tools]\nnode='18'\n").unwrap(),
                        "supplierFailure" => return Err(PlanError::EvidenceUnavailable),
                        _ => unreachable!(),
                    }
                }
                Ok((options, f.config.clone()))
            });
        assert_eq!(calls, 2, "{change}");
        if change == "supplierFailure" {
            assert!(matches!(result, Err(PlanError::EvidenceUnavailable)));
        } else {
            assert!(matches!(result, Err(PlanError::Preconditions)), "{change}");
        }
        assert_eq!(cache_state(&f), before, "{change}");
        assert_no_journal(&f);
    }
}

#[test]
fn approval_clocks_start_at_signing_and_the_signed_plan_remains_single_use() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let f = Fixture::new();
    let prepared = prepare(&f);
    let signing_started = Instant::now();
    let earliest = now();
    let plan = f
        .service
        .store(prepared, &f.query, &f.options, &f.config, None)
        .unwrap();
    let latest = now();
    let signing_finished = Instant::now();
    assert!(plan.created_at >= earliest && plan.created_at <= latest);
    assert_eq!(plan.expires_at - plan.created_at, TTL_MS);
    assert!(valid_id(&plan.plan_id));
    {
        let state = lock(&f.service.state);
        let stored = state.plans.back().unwrap();
        assert!(stored.started >= signing_started && stored.started <= signing_finished);
        assert!(!stored.consumed);
        assert!(!stored.cancel_requested);
    }
    let mut calls = 0;
    let record = f
        .execute(&plan.plan_id, |_, _, _, _| {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, plan.steps.len());
    assert_eq!(record.status, ExecutionStatus::Completed);
    assert!(matches!(
        f.execute(&plan.plan_id, |_, _, _, _| panic!(
            "signed preparation cannot be replayed"
        )),
        Err(PlanError::Consumed)
    ));
}

#[test]
fn concurrent_signatures_have_unique_identities_and_preserve_both_approvals() {
    let f = Fixture::new();
    let prepared = [prepare(&f), prepare(&f)];
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = prepared
        .into_iter()
        .map(|prepared| {
            let service = f.service.clone();
            let query = f.query.clone();
            let options = f.options.clone();
            let config = f.config.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                service
                    .store(prepared, &query, &options, &config, None)
                    .unwrap()
            })
        })
        .collect();
    let plans: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_ne!(plans[0].plan_id, plans[1].plan_id);
    assert!(plans
        .iter()
        .all(|plan| plan.capability.can_execute && valid_id(&plan.plan_id)));
    let state = cache_state(&f);
    assert_eq!(state.0, 2);
    assert_eq!(state.1.len(), 2);
    assert_no_journal(&f);
}

#[test]
fn signing_preserves_journal_failure_downgrade_and_sequence_exhaustion() {
    let f = Fixture::new();
    let prepared = prepare(&f);
    lock(&f.service.state).journal_failed = true;
    let plan = f
        .service
        .store(prepared, &f.query, &f.options, &f.config, None)
        .unwrap();
    assert!(!plan.capability.can_execute);
    assert_eq!(plan.capability.reason, Some(PlanError::Journal.to_string()));
    assert!(matches!(
        f.execute(&plan.plan_id, |_, _, _, _| panic!(
            "journal failure cannot grant execution"
        )),
        Err(PlanError::Disabled)
    ));
    assert_no_journal(&f);

    let f = Fixture::new();
    let prepared = prepare(&f);
    lock(&f.service.state).sequence = u64::MAX;
    let before = cache_state(&f);
    assert!(matches!(
        f.service
            .store(prepared, &f.query, &f.options, &f.config, None),
        Err(PlanError::Consumed)
    ));
    assert_eq!(cache_state(&f), before);
    assert_no_journal(&f);
}

#[test]
fn python_preparation_keeps_its_own_context_and_survives_an_unrelated_node_refresh() {
    let (f, options, request) = python_fixture();
    let before = cache_state(&f);
    let prepared = OperationPlanService::prepare_python(
        &f.query,
        request.clone(),
        options.clone(),
        f.config.clone(),
    )
    .unwrap();
    assert_eq!(cache_state(&f), before);
    let mut changed = options.clone();
    changed.pyenv_root = f._dir.path().join("other-pyenv");
    assert!(matches!(
        f.service.store(
            prepared,
            &f.query,
            &changed.common,
            &f.config,
            Some(&changed)
        ),
        Err(PlanError::Preconditions)
    ));
    assert_eq!(cache_state(&f), before);
    let prepared =
        OperationPlanService::prepare_python(&f.query, request, options.clone(), f.config.clone())
            .unwrap();
    f.query
        .begin("preparation-unrelated-node".into(), Some(ProviderId::Nvm))
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
    let plan = f
        .service
        .store(
            prepared,
            &f.query,
            &options.common,
            &f.config,
            Some(&options),
        )
        .unwrap();
    assert_eq!(plan.tool_id, InventoryTool::Python);
    assert!(plan.capability.can_execute);
    assert_no_journal(&f);
}

#[test]
fn node_project_bind_rejects_changed_declarations_before_signing_or_execution() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for after_signing in [false, true] {
        let f = Fixture::new();
        let project = f._dir.path().join("project");
        fs::create_dir(&project).unwrap();
        let config = project.join(".mise.toml");
        let original = "[tools]\npython='3.13'\n[env]\nKEEP='original'\n";
        fs::write(&config, original).unwrap();
        let declaration = project.join("package.json");
        fs::write(&declaration, r#"{"engines":{"node":"22.1.0"}}"#).unwrap();
        let mut request = f.request(Action::Bind);
        if let PlanIntent::Bind { binding, .. } = &mut request.intent {
            *binding = Some(BindingOptions {
                scope: BindingScope::Project,
                strategy: BindingStrategy::ProjectPath,
                project_path: Some(project.to_str().unwrap().into()),
            });
        }
        let prepared = OperationPlanService::prepare_node(
            &f.query,
            request,
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
        assert!(
            prepared.draft.capability.can_execute,
            "{:?}",
            prepared.draft.capability.reason
        );
        assert!(prepared.binding.is_some());
        assert!(prepared.project_target.is_some());
        assert_eq!(prepared.draft.action, Action::Bind);
        assert_eq!(prepared.draft.configuration_diff[0].field, "env._.path");
        let before = cache_state(&f);
        let plan = if after_signing {
            Some(
                f.service
                    .store(prepared, &f.query, &f.options, &f.config, None)
                    .unwrap(),
            )
        } else {
            fs::write(&declaration, r#"{"engines":{"node":"20.0.0"}}"#).unwrap();
            assert!(matches!(
                f.service
                    .store(prepared, &f.query, &f.options, &f.config, None),
                Err(PlanError::Preconditions)
            ));
            assert_eq!(cache_state(&f), before);
            None
        };
        if let Some(plan) = plan {
            assert!(plan.capability.can_execute);
            fs::write(&declaration, r#"{"engines":{"node":"20.0.0"}}"#).unwrap();
            assert!(matches!(
                f.execute(&plan.plan_id, |_, _, _, _| {
                    panic!("changed project declarations must stop before the binding driver")
                }),
                Err(PlanError::Preconditions)
            ));
        }
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert!(!project.join("mise.toml").exists());
        assert!(!f.options.home.join(".zenv/bindings-v1.json").exists());
        assert_no_journal(&f);
    }
}
