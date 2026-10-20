use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};
fn script(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
fn setup(f: &Fixture) {
    let root = f._dir.path();
    let node = root.join("nvm/versions/node/v22.1.0/bin/node");
    script(
        &node,
        &format!(
            "#!/bin/sh\nprintf '%s' '{}'\n",
            serde_json::json!({"version":"22.1.0","architecture":"arm64","execPath":node})
        ),
    );
    script(
        &root.join("bin/mise"),
        &format!(
            r##"#!/bin/sh
ROOT='{root}'
if [ "$1" = '--help' ]; then printf '%s' '--no-config --no-env --no-hooks'; exit 0; fi
shift 3
case "$1" in
ls)
 printf '['; comma=''
 for p in "$MISE_INSTALLS_DIR"/node/*; do
  [ -d "$p" ] || continue
  v=${{p##*/}}
  printf '%s{{"version":"%s","install_path":"%s","installed":true}}' "$comma" "$v" "$p";comma=,
 done
 printf ']';;
exec)
 printf '%s' "$MISE_AUTO_INSTALL|$MISE_EXEC_AUTO_INSTALL|$MISE_NOT_FOUND_AUTO_INSTALL" > "$ROOT/probe-environment"
 /usr/bin/touch "$ROOT/exec-started"
 mode=normal;[ ! -f "$ROOT/mode" ] || mode=$(/bin/cat "$ROOT/mode")
 if [ "$mode" = hold ];then /bin/sleep 30 & wait;fi
 if [ "$mode" = conflict ];then printf '[env]\nEXTERNAL = "keep"\n' > "$ROOT/config.toml"; exit 9;fi
 [ "$mode" != fail ] || exit 13
 if [ "$mode" = journal ];then printf 'external journal' > "$ROOT/journal/operations-v1.json";fi
 if [ "$mode" = wrong ];then printf '%s' '{{"version":"20.0.0","execPath":"/wrong/node"}}';else "$MISE_INSTALLS_DIR/node/22.1.0/bin/node";fi;;
*) exit 99;;
esac
"##,
            root = root.display()
        ),
    );
    f.query
        .begin("binding-fixture".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
}
fn request(
    f: &Fixture,
    strategy: BindingStrategy,
    scope: BindingScope,
    project: Option<String>,
) -> PlanRequest {
    let mut request = f.request(Action::Bind);
    if let PlanIntent::Bind { binding, .. } = &mut request.intent {
        *binding = Some(BindingOptions {
            strategy,
            scope,
            project_path: project,
        });
    }
    request
}
fn preview(
    f: &Fixture,
    strategy: BindingStrategy,
    scope: BindingScope,
    project: Option<String>,
) -> OperationPlan {
    f.service
        .preview(
            &f.query,
            request(f, strategy, scope, project),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap()
}
fn execute(f: &Fixture, p: &OperationPlan) -> Result<ExecutionRecord, PlanError> {
    f.service
        .execute_native_with_context(&p.plan_id, f.query.clone(), || {
            Ok((f.options.clone(), f.config.clone()))
        })
}
fn unbind(f: &Fixture) -> OperationPlan {
    let id = bindings::list(&f.options).unwrap()[0].binding_id.clone();
    let (_, snapshot) = f.query.current_snapshot().unwrap();
    f.service
        .preview(
            &f.query,
            PlanRequest {
                snapshot_request_id: snapshot.request_id,
                intent: PlanIntent::Unbind { binding_id: id },
            },
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap()
}
#[test]
fn direct_global_bind_and_unbind_preserve_original_source_and_configuration() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let before = std::fs::read(&f.config).unwrap();
    let source = f.options.nvm_dir.join("versions/node/v22.1.0/bin/node");
    let source_before = std::fs::read(&source).unwrap();
    let p = preview(&f, BindingStrategy::Direct, BindingScope::Global, None);
    assert!(p.capability.can_execute, "{:?}", p.capability.reason);
    assert_eq!(p.configuration_diff[0].field, "tools.node");
    assert_eq!(std::fs::read(&f.config).unwrap(), before);
    assert!(!f.options.home.join(".zenv/bindings-v1.json").exists());
    assert_eq!(execute(&f, &p).unwrap().outcome, Some(Outcome::Bound));
    assert_eq!(
        std::fs::read_to_string(f._dir.path().join("probe-environment")).unwrap(),
        "0|0|0"
    );
    assert_eq!(bindings::list(&f.options).unwrap()[0].state, "bound");
    let registration = f.options.mise_data_dir.join("installs/node/22.1.0");
    assert!(registration.is_symlink());
    let p = unbind(&f);
    assert_eq!(execute(&f, &p).unwrap().outcome, Some(Outcome::Unbound));
    assert!(!registration.exists());
    assert_eq!(std::fs::read(&f.config).unwrap(), before);
    assert_eq!(std::fs::read(&source).unwrap(), source_before);
    assert!(bindings::list(&f.options).unwrap().is_empty());
    let journal =
        std::fs::read_to_string(f._dir.path().join("journal/operations-v1.json")).unwrap();
    assert!(!journal.contains("private-value"));
    assert!(!journal.contains(&f.options.home.to_string_lossy().to_string()));
    let mode = std::fs::metadata(f.options.home.join(".zenv/bindings-v1.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
#[test]
fn verification_failure_or_wrong_runtime_restores_links_and_exact_config_bytes() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["fail", "wrong"] {
        let f = Fixture::new();
        setup(&f);
        let before = std::fs::read(&f.config).unwrap();
        let p = preview(&f, BindingStrategy::Direct, BindingScope::Global, None);
        std::fs::write(f._dir.path().join("mode"), mode).unwrap();
        assert!(execute(&f, &p).is_err());
        let r = f.service.records().0.remove(0);
        assert_eq!(r.compensation, Some(CompensationOutcome::Completed));
        assert_ne!(r.outcome, Some(Outcome::Bound));
        assert!(r
            .steps
            .iter()
            .any(|s| s.kind == StepKind::Compensate && s.status == StepStatus::Completed));
        assert_eq!(std::fs::read(&f.config).unwrap(), before);
        assert!(!f
            .options
            .mise_data_dir
            .join("installs/node/22.1.0")
            .exists());
        assert!(bindings::list(&f.options).unwrap().is_empty());
    }
}
#[test]
fn cancellation_during_controlled_probe_reaps_process_then_compensates() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    std::fs::write(f._dir.path().join("mode"), "hold").unwrap();
    let before = std::fs::read(&f.config).unwrap();
    let p = preview(&f, BindingStrategy::Direct, BindingScope::Global, None);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| execute(&f, &p));
        let start = Instant::now();
        while !f._dir.path().join("exec-started").exists() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        operations::cancel(&p.plan_id).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
    });
    let r = f.service.records().0.remove(0);
    assert_eq!(r.status, ExecutionStatus::Cancelled);
    assert_eq!(r.compensation, Some(CompensationOutcome::Completed));
    assert_eq!(std::fs::read(&f.config).unwrap(), before);
    assert!(bindings::list(&f.options).unwrap().is_empty());
}
#[test]
fn external_edit_stops_config_compensation_and_retains_recovery_receipt() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    std::fs::write(f._dir.path().join("mode"), "conflict").unwrap();
    let p = preview(&f, BindingStrategy::Direct, BindingScope::Global, None);
    assert!(execute(&f, &p).is_err());
    let r = f.service.records().0.remove(0);
    assert_eq!(r.compensation, Some(CompensationOutcome::Conflict));
    assert_eq!(r.outcome, Some(Outcome::CompensationConflict));
    assert!(std::fs::read_to_string(&f.config)
        .unwrap()
        .contains("EXTERNAL"));
    assert!(!f
        .options
        .mise_data_dir
        .join("installs/node/22.1.0")
        .exists());
    assert_eq!(bindings::list(&f.options).unwrap()[0].state, "pending");
    let id = bindings::list(&f.options).unwrap()[0].binding_id.clone();
    let (_, s) = f.query.current_snapshot().unwrap();
    assert!(f
        .service
        .preview(
            &f.query,
            PlanRequest {
                snapshot_request_id: s.request_id,
                intent: PlanIntent::Unbind { binding_id: id }
            },
            f.options.clone(),
            f.config.clone()
        )
        .is_err());
}
#[test]
fn existing_native_install_or_unknown_link_is_never_overwritten() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for link in [false, true] {
        let f = Fixture::new();
        setup(&f);
        let path = f.options.mise_data_dir.join("installs/node/22.1.0");
        if link {
            symlink(f._dir.path().join("missing"), &path).unwrap();
        } else {
            std::fs::create_dir(&path).unwrap();
        }
        let p = preview(
            &f,
            BindingStrategy::Direct,
            BindingScope::Registration,
            None,
        );
        assert!(!p.capability.can_execute);
        assert!(std::fs::symlink_metadata(path).is_ok());
    }
}
#[test]
fn project_path_binding_preserves_other_tools_and_rejects_conflicting_node_requirement() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let project = f._dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let file = project.join(".mise.toml");
    let original = "[tools]\npython='3.13'\n[env] # keep\nA='secret'\n";
    std::fs::write(&file, original).unwrap();
    let p = preview(
        &f,
        BindingStrategy::ProjectPath,
        BindingScope::Project,
        Some(project.to_string_lossy().into()),
    );
    assert!(p.capability.can_execute, "{:?}", p.capability.reason);
    assert_eq!(p.configuration_diff[0].field, "env._.path");
    assert_eq!(execute(&f, &p).unwrap().outcome, Some(Outcome::Bound));
    assert!(!f
        .options
        .mise_data_dir
        .join("installs/node/22.1.0")
        .exists());
    assert!(!project.join("mise.toml").exists());
    assert_eq!(
        execute(&f, &unbind(&f)).unwrap().outcome,
        Some(Outcome::Unbound)
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    std::fs::write(&file, "[tools]\nnode='20'\n").unwrap();
    let p = preview(
        &f,
        BindingStrategy::ProjectPath,
        BindingScope::Project,
        Some(project.to_string_lossy().into()),
    );
    assert!(!p.capability.can_execute);
    assert!(p.capability.reason.unwrap().contains("Node 工具声明"));
}
#[test]
fn source_removal_does_not_prevent_explicit_unbind_of_recorded_resources() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let p = preview(
        &f,
        BindingStrategy::Direct,
        BindingScope::Registration,
        None,
    );
    execute(&f, &p).unwrap();
    std::fs::remove_dir_all(f.options.nvm_dir.join("versions/node/v22.1.0")).unwrap();
    f.query
        .begin("removed-source".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
    let p = unbind(&f);
    assert!(p.capability.can_execute);
    assert_eq!(execute(&f, &p).unwrap().outcome, Some(Outcome::Unbound));
    assert!(bindings::list(&f.options).unwrap().is_empty());
}
#[test]
fn config_change_after_preview_and_symlink_retarget_stop_before_any_binding_write() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let project = f._dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let file = project.join("mise.toml");
    std::fs::write(&file, "[env]\nA='old'\n").unwrap();
    let p = preview(
        &f,
        BindingStrategy::Direct,
        BindingScope::Project,
        Some(project.to_string_lossy().into()),
    );
    std::fs::remove_file(&file).unwrap();
    let external = f._dir.path().join("external.toml");
    std::fs::write(&external, "[env]\nA='external'\n").unwrap();
    symlink(&external, &file).unwrap();
    assert!(execute(&f, &p).is_err());
    assert!(!f.options.home.join(".zenv/bindings-v1.json").exists());
    assert!(std::fs::read_to_string(external)
        .unwrap()
        .contains("external"));
}

#[test]
fn managed_brew_binding_uses_farm_and_unbind_never_uninstalls_the_original_keg() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    channels::setup_brew(&mut f);
    setup(&f);
    // 用真实文件布局及动态注册记录，验证双层链接的目标身份。
    let install = channels::preview_brew(&f);
    channels::execute_native(&f, &install).unwrap();
    let node = f.options.brew_cellars[0].join("node@22/22.1.0/bin/node");
    script(
        &node,
        &format!(
            "#!/bin/sh\nprintf '%s' '{}'\n",
            serde_json::json!({"version":"22.1.0","architecture":"arm64","execPath":node})
        ),
    );
    f.query
        .begin("brew-binding".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
    let (_, snapshot) = f.query.current_snapshot().unwrap();
    let target = snapshot
        .installations
        .iter()
        .find(|i| i.provider_package.provider_id == ProviderId::Brew)
        .unwrap();
    let p = f
        .service
        .preview(
            &f.query,
            PlanRequest {
                snapshot_request_id: snapshot.request_id.clone(),
                intent: PlanIntent::Bind {
                    installation_id: target.installation_id.clone(),
                    binding: Some(BindingOptions {
                        strategy: BindingStrategy::Managed,
                        scope: BindingScope::Registration,
                        project_path: None,
                    }),
                },
            },
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(p.capability.can_execute, "{:?}", p.capability.reason);
    assert!(p.impact.iter().any(|s| s.contains("创建链接")));
    assert_eq!(execute(&f, &p).unwrap().outcome, Some(Outcome::Bound));
    let farm = f.options.home.join(".zenv/managed/node/22.1.0");
    assert!(farm.is_symlink());
    assert_eq!(
        execute(&f, &unbind(&f)).unwrap().outcome,
        Some(Outcome::Unbound)
    );
    assert!(!farm.exists());
    assert!(node.exists());
    assert!(!f._dir.path().join("brew-uninstall-argv").exists());
}

#[test]
fn pending_receipt_after_abrupt_exit_can_be_explicitly_released_without_replaying_bind() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let before = std::fs::read(&f.config).unwrap();
    let p = preview(&f, BindingStrategy::Direct, BindingScope::Global, None);
    let stored = lock(&f.service.state)
        .plans
        .iter()
        .find(|s| s.view.plan_id == p.plan_id)
        .unwrap()
        .clone();
    let mut transaction = bindings::Transaction::default();
    stored
        .binding
        .as_ref()
        .unwrap()
        .apply(&mut transaction, "simulated-exit")
        .unwrap();
    drop(transaction);
    assert_eq!(bindings::list(&f.options).unwrap()[0].state, "pending");
    f.query
        .begin("after-exit".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
    assert_eq!(
        execute(&f, &unbind(&f)).unwrap().outcome,
        Some(Outcome::Unbound)
    );
    assert_eq!(std::fs::read(&f.config).unwrap(), before);
    assert!(bindings::list(&f.options).unwrap().is_empty());
}

#[test]
fn late_journal_failure_still_compensates_bound_resources_and_preserves_external_journal() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let before = std::fs::read(&f.config).unwrap();
    let p = preview(&f, BindingStrategy::Direct, BindingScope::Global, None);
    std::fs::write(f._dir.path().join("mode"), "journal").unwrap();
    assert!(execute(&f, &p).is_err());
    let r = f.service.records().0.remove(0);
    assert_eq!(r.status, ExecutionStatus::Interrupted);
    assert_eq!(r.compensation, Some(CompensationOutcome::Completed));
    assert_eq!(std::fs::read(&f.config).unwrap(), before);
    assert!(bindings::list(&f.options).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(f._dir.path().join("journal/operations-v1.json")).unwrap(),
        "external journal"
    );
}

#[test]
fn modified_receipt_cannot_redirect_unbind_to_an_unrelated_resource() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let p = preview(&f, BindingStrategy::Direct, BindingScope::Global, None);
    execute(&f, &p).unwrap();
    let ledger = f.options.home.join(".zenv/bindings-v1.json");
    let mut doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&ledger).unwrap()).unwrap();
    doc["records"][0]["resources"][0]["path"] = f.config.to_string_lossy().to_string().into();
    std::fs::write(&ledger, doc.to_string()).unwrap();
    let id = bindings::list(&f.options).unwrap()[0].binding_id.clone();
    let (_, s) = f.query.current_snapshot().unwrap();
    assert!(f
        .service
        .preview(
            &f.query,
            PlanRequest {
                snapshot_request_id: s.request_id,
                intent: PlanIntent::Unbind { binding_id: id }
            },
            f.options.clone(),
            f.config.clone()
        )
        .is_err());
    assert!(f
        .options
        .mise_data_dir
        .join("installs/node/22.1.0")
        .is_symlink());
}

#[test]
fn native_mise_selection_reuses_and_preserves_the_independent_install_on_unbind() {
    let _lock = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let node = f
        .options
        .mise_data_dir
        .join("installs/node/22.1.0/bin/node");
    script(
        &node,
        &format!(
            "#!/bin/sh\nprintf '%s' '{}'\n",
            serde_json::json!({"version":"22.1.0","architecture":"arm64","execPath":node})
        ),
    );
    let before = std::fs::read(&node).unwrap();
    f.query
        .begin("native-mise".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
    let (_, snapshot) = f.query.current_snapshot().unwrap();
    let target = snapshot
        .installations
        .iter()
        .find(|i| i.provider_package.provider_id == ProviderId::Mise)
        .unwrap();
    let p = f
        .service
        .preview(
            &f.query,
            PlanRequest {
                snapshot_request_id: snapshot.request_id.clone(),
                intent: PlanIntent::Bind {
                    installation_id: target.installation_id.clone(),
                    binding: Some(BindingOptions {
                        strategy: BindingStrategy::Direct,
                        scope: BindingScope::Global,
                        project_path: None,
                    }),
                },
            },
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(p.capability.can_execute, "{:?}", p.capability.reason);
    assert_eq!(execute(&f, &p).unwrap().outcome, Some(Outcome::Bound));
    assert_eq!(
        execute(&f, &unbind(&f)).unwrap().outcome,
        Some(Outcome::Unbound)
    );
    assert_eq!(std::fs::read(&node).unwrap(), before);
    assert!(!node.parent().unwrap().parent().unwrap().is_symlink());
}
