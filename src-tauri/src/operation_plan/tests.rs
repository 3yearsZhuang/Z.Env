use super::*;
use std::sync::atomic::AtomicUsize;

#[cfg(unix)]
struct Fixture {
    _dir: tempfile::TempDir,
    options: NodeScanOptions,
    config: PathBuf,
    query: InventoryQueryService,
    service: OperationPlanService,
}
#[cfg(unix)]
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let bin = root.join("nvm/versions/node/v22.1.0/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let node = bin.join("node");
        std::fs::write(
            &node,
            "#!/bin/sh\nprintf '%s' '{\"version\":\"22.1.0\",\"architecture\":\"arm64\"}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(root.join("mise/installs/node")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        for name in ["mise", "brew"] {
            let path = root.join("bin").join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 99\n").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let options = NodeScanOptions {
            platform: Platform::Macos,
            home: root.to_owned(),
            mise_data_dir: root.join("mise"),
            nvm_dir: root.join("nvm"),
            brew_cellars: vec![root.join("Cellar")],
            path_entries: vec![root.join("bin")],
            limits: Default::default(),
        };
        // 空但存在的 Cellar 表示该来源完整读取。
        std::fs::create_dir_all(&options.brew_cellars[0]).unwrap();
        let config = root.join("config.toml");
        std::fs::write(&config, "[env]\nSECRET = 'private-value'\n").unwrap();
        let query = InventoryQueryService::default();
        query
            .begin("fixture".into(), None)
            .unwrap()
            .execute(options.clone())
            .unwrap();
        let service = OperationPlanService::new(root.join("journal/operations-v1.json"));
        Self {
            _dir: dir,
            options,
            config,
            query,
            service,
        }
    }
    fn request(&self, action: Action) -> PlanRequest {
        let (_, snapshot) = self.query.current_snapshot().unwrap();
        let id = snapshot
            .installations
            .iter()
            .find(|i| i.provider_package.provider_id == ProviderId::Nvm)
            .unwrap()
            .installation_id
            .clone();
        PlanRequest {
            snapshot_request_id: snapshot.request_id,
            intent: match action {
                Action::Verify => PlanIntent::Verify {
                    installation_id: id,
                },
                Action::Uninstall => PlanIntent::Uninstall {
                    installation_id: id,
                },
                Action::Bind => PlanIntent::Bind {
                    installation_id: id,
                    binding: None,
                },
                Action::Unbind => unreachable!(),
                Action::Install => PlanIntent::Install {
                    provider_id: ProviderId::Mise,
                    package_id: "node".into(),
                    version: Some("22.1.0".into()),
                },
            },
        }
    }
    fn plan(&self) -> OperationPlan {
        self.service
            .preview(
                &self.query,
                self.request(Action::Verify),
                self.options.clone(),
                self.config.clone(),
            )
            .unwrap()
    }
    fn execute(
        &self,
        id: &str,
        driver: impl FnMut(StepKind, &StoredPlan, &NodeScanOptions, &str) -> Result<(), PlanError>,
    ) -> Result<ExecutionRecord, PlanError> {
        self.service.execute_with(
            id,
            &self.query,
            || Ok((self.options.clone(), self.config.clone())),
            driver,
        )
    }
}

#[test]
fn requests_reject_frontend_steps_commands_and_unknown_fields() {
    for data in [
        r#"{"snapshotRequestId":"s","intent":{"action":"verify","installationId":"i","argv":["rm"]}}"#,
        r#"{"snapshotRequestId":"s","intent":{"action":"shell","program":"sh"}}"#,
        r#"{"snapshotRequestId":"s","intent":{"action":"verify","installationId":"i"},"steps":[]}"#,
    ] {
        assert!(serde_json::from_str::<PlanRequest>(data).is_err());
    }
    let request:PlanRequest=serde_json::from_str(r#"{"snapshotRequestId":"s","intent":{"action":"install","providerId":"brew","packageId":"node@22","version":null}}"#).unwrap();
    assert!(matches!(
        request.intent,
        PlanIntent::Install { version: None, .. }
    ));
}
#[test]
fn exact_version_policy_rejects_ranges_and_shell_arguments() {
    for v in [
        "22",
        "latest",
        "22.1",
        "22.1.0;touch /tmp/x",
        "v22.1.0",
        "022.1.0",
        "22.1.0\n",
        "22.1.0-beta",
        "22.*.0",
    ] {
        assert!(!exact_version(v), "{v}");
    }
    assert!(exact_version("22.1.0"));
}
#[cfg(unix)]
#[test]
fn readonly_preview_is_bound_to_snapshot_and_does_not_create_journal() {
    let f = Fixture::new();
    f.service.recover().unwrap();
    let p = f.plan();
    assert!(p.capability.can_execute);
    assert_eq!(p.snapshot_generation, "1");
    assert_eq!(p.steps.len(), 3);
    assert_eq!(p.action, Action::Verify);
    assert!(!f._dir.path().join("journal").exists());
    let json = serde_json::to_string(&p).unwrap();
    assert!(!json.contains("private-value"));
    assert!(!json.contains("SECRET"));
    let mut r = f.request(Action::Verify);
    r.snapshot_request_id = "old".into();
    assert!(matches!(
        f.service
            .preview(&f.query, r, f.options.clone(), f.config.clone()),
        Err(PlanError::Stale)
    ));
}
#[cfg(unix)]
#[test]
fn capability_never_grants_ownership_or_precision_to_unsupported_channels() {
    let f = Fixture::new();
    for action in [Action::Uninstall, Action::Bind] {
        let p = f
            .service
            .preview(
                &f.query,
                f.request(action),
                f.options.clone(),
                f.config.clone(),
            )
            .unwrap();
        assert!(!p.capability.can_execute);
        assert!(!p.capability.can_preview);
        assert!(p.steps.is_empty());
        assert!(p.capability.reason.is_some());
    }
    let p = f
        .service
        .preview(
            &f.query,
            f.request(Action::Install),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(!p.capability.can_preview);
    assert!(!p.capability.can_execute);
    assert!(p.steps.is_empty());
    let mut request = f.request(Action::Install);
    request.intent = PlanIntent::Install {
        provider_id: ProviderId::Brew,
        package_id: "node@22".into(),
        version: Some("22.1.0".into()),
    };
    assert!(matches!(
        f.service
            .preview(&f.query, request, f.options.clone(), f.config.clone()),
        Err(PlanError::Unsupported)
    ));
    for package in ["other/tap/node@22", "node@22;touch x", "--help"] {
        let mut r = f.request(Action::Install);
        r.intent = PlanIntent::Install {
            provider_id: ProviderId::Brew,
            package_id: package.into(),
            version: None,
        };
        assert!(f
            .service
            .preview(&f.query, r, f.options.clone(), f.config.clone())
            .is_err());
    }
}
#[cfg(unix)]
#[test]
fn execute_uses_global_lock_is_single_use_and_writes_no_secrets() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    let calls = AtomicUsize::new(0);
    let record = f
        .execute(&p.plan_id, |_, _, _, _| {
            calls.fetch_add(1, Ordering::Relaxed);
            assert!(
                operations::run(Some("concurrent-plan-test".into()), "并发", |_| Ok(())).is_err()
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(record.status, ExecutionStatus::Completed);
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert!(record
        .steps
        .iter()
        .all(|s| s.status == StepStatus::Completed));
    assert!(matches!(
        f.execute(&p.plan_id, |_, _, _, _| panic!("重复执行")),
        Err(PlanError::Consumed)
    ));
    let data = std::fs::read_to_string(f._dir.path().join("journal/operations-v1.json")).unwrap();
    for secret in [
        "private-value",
        "SECRET",
        f._dir.path().to_str().unwrap(),
        "argv",
        "environment",
        "installationId",
        "22.1.0",
    ] {
        assert!(!data.contains(secret), "{secret}");
    }
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(f._dir.path().join("journal/operations-v1.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
#[cfg(unix)]
#[test]
fn expiry_configuration_target_and_source_changes_stop_before_driver() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for change in ["expiry", "config", "target", "snapshot", "context"] {
        let f = Fixture::new();
        let p = f.plan();
        match change {
            "expiry" => {
                lock(&f.service.state)
                    .plans
                    .back_mut()
                    .unwrap()
                    .view
                    .expires_at = now();
            }
            "config" => std::fs::write(&f.config, "[env]\nSECRET='changed'\n").unwrap(),
            "target" => {
                let target = lock(&f.service.state).plans.back().unwrap().files[0]
                    .0
                    .clone();
                std::fs::write(target, "replacement").unwrap();
            }
            "snapshot" => {
                f.query
                    .begin("new-snapshot".into(), None)
                    .unwrap()
                    .execute(f.options.clone())
                    .unwrap();
            }
            "context" => {
                lock(&f.service.state).plans.back_mut().unwrap().context = "changed".into()
            }
            _ => unreachable!(),
        }
        assert!(
            f.execute(&p.plan_id, |_, _, _, _| panic!("前置条件失效后不能执行"))
                .is_err(),
            "{change}"
        );
        assert!(f.service.records().0.is_empty());
        assert!(!f._dir.path().join("journal").exists());
    }
}
#[cfg(unix)]
#[test]
fn journal_failure_prevents_first_step_and_does_not_overwrite_corrupt_data() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    std::fs::write(f._dir.path().join("journal"), "not a directory").unwrap();
    assert!(f
        .execute(&p.plan_id, |_, _, _, _| panic!("journal 失败不能执行"))
        .is_err());
    assert!(f.service.records().1);
    let f = Fixture::new();
    let path = f._dir.path().join("journal/operations-v1.json");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "corrupt-private-data").unwrap();
    assert!(f.service.recover().is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "corrupt-private-data"
    );
    assert!(!f.plan().capability.can_execute);
}
#[cfg(unix)]
#[test]
fn cancellation_waits_for_step_exit_and_preserves_completed_steps() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    assert!(f
        .execute(&p.plan_id, |kind, _, _, id| {
            if kind == StepKind::Refresh {
                operations::cancel(id).unwrap();
                assert_eq!(
                    operations::list()
                        .iter()
                        .find(|j| j.id == id)
                        .unwrap()
                        .status,
                    "running"
                );
                return Err(PlanError::Cancelled);
            }
            Ok(())
        })
        .is_err());
    let r = f.service.records().0.remove(0);
    assert_eq!(r.status, ExecutionStatus::Cancelled);
    assert_eq!(r.steps[0].status, StepStatus::Completed);
    assert_eq!(r.steps[1].status, StepStatus::Failed);
    assert_eq!(r.steps[2].status, StepStatus::Skipped);
}
#[cfg(unix)]
#[test]
fn partial_failure_and_panics_have_distinct_terminal_records() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    {
        let mut state = lock(&f.service.state);
        let stored = state.plans.back_mut().unwrap();
        stored.view.steps[1].kind = StepKind::Command;
    }
    assert!(f
        .execute(&p.plan_id, |kind, _, _, _| if kind == StepKind::Verify {
            Err(PlanError::Verification)
        } else {
            Ok(())
        })
        .is_err());
    assert_eq!(f.service.records().0[0].status, ExecutionStatus::Partial);
    let f = Fixture::new();
    let p = f.plan();
    assert!(f
        .execute(&p.plan_id, |_, _, _, _| panic!("模拟异常"))
        .is_err());
    assert_eq!(
        f.service.records().0[0].status,
        ExecutionStatus::Interrupted
    );
    assert_eq!(
        operations::list()
            .iter()
            .find(|j| j.id == p.plan_id)
            .unwrap()
            .status,
        "interrupted"
    );
}
#[cfg(unix)]
#[test]
fn restart_marks_unfinished_records_interrupted_without_replay_and_locks_other_instances() {
    let f = Fixture::new();
    let p = f.plan();
    let path = f._dir.path().join("journal/operations-v1.json");
    {
        let mut state = lock(&f.service.state);
        state
            .journal
            .push(ExecutionRecord {
                schema_version: 1,
                tool_id: crate::inventory::query::InventoryTool::Node,
                id: p.plan_id.clone(),
                action: Action::Install,
                status: ExecutionStatus::Running,
                created_at: 1,
                updated_at: 1,
                steps: vec![
                    ExecutionStep {
                        kind: StepKind::Command,
                        status: StepStatus::Running,
                    },
                    ExecutionStep {
                        kind: StepKind::Verify,
                        status: StepStatus::Planned,
                    },
                ],
                outcome: None,
                compensation: None,
            })
            .unwrap();
    }
    let other = OperationPlanService::new(path.clone());
    assert!(matches!(other.recover(), Err(PlanError::JournalBusy)));
    drop(f.service);
    let recovered = OperationPlanService::new(path);
    recovered.recover().unwrap();
    let r = recovered.records().0.remove(0);
    assert_eq!(r.status, ExecutionStatus::Interrupted);
    assert_eq!(r.steps[0].status, StepStatus::Failed);
    assert_eq!(r.steps[1].status, StepStatus::Skipped);
    assert_eq!(r.outcome, Some(Outcome::Interrupted));
}
#[cfg(unix)]
#[test]
fn file_evidence_distinguishes_absence_symlink_target_and_special_files() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent");
    assert!(!configuration(&absent).unwrap().0.exists);
    let broken = dir.path().join("broken");
    symlink(&absent, &broken).unwrap();
    assert!(configuration(&broken).is_err());
    assert!(configuration(dir.path()).is_err());
    let fifo = dir.path().join("fifo");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert!(configuration(&fifo).is_err());
    let large = dir.path().join("large");
    std::fs::write(&large, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    assert!(configuration(&large).is_err());
}

#[cfg(unix)]
#[test]
fn change_during_preflight_is_checked_again_immediately_before_mutation() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    {
        lock(&f.service.state).plans.back_mut().unwrap().view.steps[1].kind = StepKind::Command;
    }
    assert!(f
        .execute(&p.plan_id, |kind, _, _, _| {
            if kind == StepKind::Preflight {
                std::fs::write(&f.config, "external update").unwrap();
                Ok(())
            } else {
                panic!("配置变化后不得进入变更步骤")
            }
        })
        .is_err());
    let records = f.service.records().0;
    assert_eq!(records[0].status, ExecutionStatus::Failed);
    assert_eq!(records[0].outcome, Some(Outcome::PreconditionChanged));
    assert_eq!(
        std::fs::read_to_string(&f.config).unwrap(),
        "external update"
    );
}
#[cfg(unix)]
#[test]
fn late_journal_failure_stops_next_step_and_marks_task_interrupted() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    assert!(f
        .execute(&p.plan_id, |kind, _, _, _| {
            assert_eq!(kind, StepKind::Preflight);
            let path = f._dir.path().join("journal/operations-v1.json");
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(path).unwrap();
            Ok(())
        })
        .is_err());
    assert!(f.service.records().1);
    assert_eq!(
        operations::list()
            .iter()
            .find(|j| j.id == p.plan_id)
            .unwrap()
            .status,
        "interrupted"
    );
}

#[test]
fn shared_plan_fixture_roundtrips_camel_case_nulls_and_interruption() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/operations/contract-v1.json"
    ))
    .unwrap();
    let request: PlanRequest = serde_json::from_value(fixture["request"].clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), fixture["request"]);
    let plan: OperationPlan = serde_json::from_value(fixture["plan"].clone()).unwrap();
    assert_eq!(serde_json::to_value(plan).unwrap(), fixture["plan"]);
    let record: ExecutionRecord = serde_json::from_value(fixture["record"].clone()).unwrap();
    assert_eq!(record.status, ExecutionStatus::Interrupted);
    assert_eq!(serde_json::to_value(record).unwrap(), fixture["record"]);
}

#[cfg(unix)]
#[test]
fn failed_mutating_step_is_conservatively_partial_even_without_success_exit() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    {
        lock(&f.service.state).plans.back_mut().unwrap().view.steps[1].kind = StepKind::Command;
    }
    assert!(f
        .execute(&p.plan_id, |kind, _, _, _| if kind == StepKind::Command {
            Err(PlanError::Unsupported)
        } else {
            Ok(())
        })
        .is_err());
    assert_eq!(f.service.records().0[0].status, ExecutionStatus::Partial);
}

#[cfg(unix)]
#[test]
fn absent_configuration_parent_symlink_retarget_invalidates_the_plan() {
    use std::os::unix::fs::symlink;
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    let a = f._dir.path().join("a");
    let b = f._dir.path().join("b");
    let link = f._dir.path().join("config-dir");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    symlink(&a, &link).unwrap();
    f.config = link.join("not-created.toml");
    let p = f.plan();
    assert!(!p.configuration.exists);
    std::fs::remove_file(&link).unwrap();
    symlink(&b, &link).unwrap();
    assert!(f
        .execute(&p.plan_id, |_, _, _, _| panic!("父目录重定向后不能继续"))
        .is_err());
}

#[cfg(unix)]
#[test]
fn cancellation_before_worker_registration_prevents_any_execution_or_journal_write() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    assert!(f.service.cancel_pending(&p.plan_id));
    assert!(f
        .execute(&p.plan_id, |_, _, _, _| panic!("提前取消不得执行"))
        .is_err());
    assert!(!f._dir.path().join("journal").exists());
    assert_eq!(
        operations::list()
            .iter()
            .find(|j| j.id == p.plan_id)
            .unwrap()
            .status,
        "cancelled"
    );
    assert!(!f.service.cancel_pending(&p.plan_id));
    assert!(!f.service.cancel_pending("unknown"));
}

#[cfg(unix)]
#[test]
fn plan_cache_is_bounded_and_evicted_tokens_cannot_execute() {
    let f = Fixture::new();
    let first = f.plan();
    for _ in 0..MAX_PLANS {
        f.plan();
    }
    assert_eq!(lock(&f.service.state).plans.len(), MAX_PLANS);
    assert!(matches!(
        f.execute(&first.plan_id, |_, _, _, _| panic!("过期缓存身份不能执行")),
        Err(PlanError::Consumed)
    ));
}

#[cfg(unix)]
#[test]
fn external_journal_edit_is_preserved_and_stops_following_steps() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    let path = f._dir.path().join("journal/operations-v1.json");
    assert!(f
        .execute(&p.plan_id, |kind, _, _, _| {
            assert_eq!(kind, StepKind::Preflight);
            std::fs::write(&path, "external-private-data").unwrap();
            Ok(())
        })
        .is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "external-private-data"
    );
    assert!(f.service.records().1);
    assert_eq!(
        operations::list()
            .iter()
            .find(|j| j.id == p.plan_id)
            .unwrap()
            .status,
        "interrupted"
    );
}

#[cfg(unix)]
mod binding;
mod channels;

#[cfg(unix)]
#[test]
fn v2_node_preview_uses_existing_once_only_execution_and_preconditions() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for change_config in [false, true] {
        let f = Fixture::new();
        let request = PlanRequestV2 {
            schema_version: 2,
            subject: OperationSubjectV2::Tool {
                tool_id: ToolId::from("node"),
            },
            request: f.request(Action::Verify),
        };
        let wrapped = f
            .service
            .preview_v2_using(request, |tool, request| {
                assert_eq!(tool, InventoryTool::Node);
                f.service
                    .preview(&f.query, request, f.options.clone(), f.config.clone())
            })
            .unwrap();
        assert_eq!(lock(&f.service.state).plans.len(), 1);
        assert_eq!(wrapped.step_graph.len(), wrapped.plan.steps.len());
        assert!(!f._dir.path().join("journal").exists());
        if change_config {
            std::fs::write(&f.config, "[tools]\nnode='20'\n").unwrap();
            assert!(matches!(
                f.execute(&wrapped.plan.plan_id, |_, _, _, _| panic!(
                    "changed configuration must stop before driver"
                )),
                Err(PlanError::Preconditions)
            ));
        } else {
            let calls = AtomicUsize::new(0);
            let record = f
                .execute(&wrapped.plan.plan_id, |_, _, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                })
                .unwrap();
            assert_eq!(record.status, ExecutionStatus::Completed);
            assert_eq!(calls.load(Ordering::Relaxed), wrapped.step_graph.len());
        }
        assert!(matches!(
            f.execute(&wrapped.plan.plan_id, |_, _, _, _| panic!(
                "v2 cannot replay a consumed v1 plan"
            )),
            Err(PlanError::Consumed)
        ));
    }
}
