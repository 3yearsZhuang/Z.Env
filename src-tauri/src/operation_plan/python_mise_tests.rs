use super::*;
fn setup(f: &Fixture) {
    let root = f.dir.path();
    let existing = f
        .options
        .common
        .mise_data_dir
        .join("installs/python/3.13.15");
    let locator = f
        .options
        .common
        .mise_data_dir
        .join("installs/python/3.12.12");
    let old = serde_json::json!([{"version":"3.13.15","install_path":existing,"installed":true}]);
    let both = serde_json::json!([{"version":"3.13.15","install_path":existing,"installed":true},{"version":"3.12.12","install_path":locator,"installed":true}]);
    fs::write(root.join("registry.json"), old.to_string()).unwrap();
    fs::write(root.join("old.json"), old.to_string()).unwrap();
    fs::write(root.join("both.json"), both.to_string()).unwrap();
    fs::write(
        f.options
            .common
            .mise_data_dir
            .join("installs/python/.mise.backend.toml"),
        "full='core:python'\n",
    )
    .unwrap();
    let output = serde_json::json!({"version":"3.12.12","implementation":"cpython","releaseLevel":"final","freeThreaded":false,"debug":false,"architecture":"arm64","execPath":locator.join("bin/python3")});
    script(
        &root.join("template"),
        &format!("#!/bin/sh\nprintf '%s' '{}'\n", output),
    );
    script(&root.join("wrong-template"), "#!/bin/sh\nprintf '{}'\n");
    let text = format!(
        r##"#!/bin/sh
ROOT='{root}'
if [ "$1" = '--help' ]; then printf '%s' '--no-config --no-env --no-hooks'; exit 0; fi
[ "$1|$2|$3" = '--no-config|--no-env|--no-hooks' ] || exit 95
shift 3
mode=normal; [ ! -f "$ROOT/mode" ] || mode=$(/bin/cat "$ROOT/mode")
case "$1" in
settings)
 case "$3" in
 python.compile) if [ "$mode" = compile ]; then printf true; else printf false; fi ;;
 python.default_packages_file) printf '%s' "$MISE_PYTHON_DEFAULT_PACKAGES_FILE" ;;
 *) exit 96;; esac ;;
ls)
 n=0; [ ! -f "$ROOT/ls-count" ] || n=$(/bin/cat "$ROOT/ls-count"); n=$((n+1)); printf '%s' "$n" > "$ROOT/ls-count"
 if [ "$mode" = late ] && [ "$n" -ge 3 ]; then /bin/cat "$ROOT/both.json"; else /bin/cat "$ROOT/registry.json"; fi ;;
install)
 printf '%s' "$MISE_PYTHON_COMPILE|${{PYTHONHOME-unset}}|${{PYTHONPATH-unset}}|${{PIP_CONFIG_FILE-unset}}|$MISE_NO_CONFIG|$MISE_NO_ENV|$MISE_NO_HOOKS" > "$ROOT/env"
 printf '%s\n' "$@" > "$ROOT/command"
 [ "$2" = core:python@3.12.12 ] || exit 97
 [ "$mode" != permission ] || exit 13
 [ "$mode" != zero ] || exit 0
 /bin/mkdir -p "$MISE_DATA_DIR/installs/python/3.12.12/bin"
 /bin/cp "$ROOT/template" "$MISE_DATA_DIR/installs/python/3.12.12/bin/python3"
 [ "$mode" != wrong ] || /bin/cp "$ROOT/wrong-template" "$MISE_DATA_DIR/installs/python/3.12.12/bin/python3"
 printf '%s' "full='core:python'" > "$MISE_DATA_DIR/installs/python/.mise.backend.toml"
 /bin/cp "$ROOT/both.json" "$ROOT/registry.json"
 printf '%s' 'PRIVATE_COMMAND_OUTPUT'
 if [ "$mode" = slow ]; then /usr/bin/touch "$ROOT/started"; /bin/sleep 30; fi ;;
uninstall)
 printf '%s\n' "$@" > "$ROOT/uninstall-command"
 [ "$2" = core:python@3.12.12 ] || exit 98
 /bin/rm -rf "$MISE_DATA_DIR/installs/python/3.12.12"
 /bin/cp "$ROOT/old.json" "$ROOT/registry.json" ;;
*) exit 99;;
esac
"##,
        root = root.display()
    );
    script(&root.join("bin/mise"), &text);
}
fn install(f: &Fixture) -> OperationPlan {
    let (_, s) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    f.preview(PlanRequest {
        snapshot_request_id: s.request_id,
        intent: PlanIntent::Install {
            provider_id: ProviderId::Mise,
            package_id: "python".into(),
            version: Some("3.12.12".into()),
        },
    })
    .unwrap()
}
#[test]
fn mise_python_installs_and_removes_exact_native_target_without_changing_node_or_configuration() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let node = serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap();
    let config = fs::read(&f.config).unwrap();
    let p = install(&f);
    assert!(p.capability.can_execute, "{:?}", p.capability);
    assert!(p.steps[1]
        .environment_keys
        .contains(&"MISE_PYTHON_DEFAULT_PACKAGES_FILE".into()));
    assert_eq!(f.execute(&p).unwrap().outcome, Some(Outcome::Installed));
    assert_eq!(
        fs::read_to_string(f.dir.path().join("env")).unwrap(),
        "0|unset|unset|unset|1|1|1"
    );
    let (_, snapshot) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    let item = snapshot
        .installations
        .iter()
        .find(|i| i.version.as_deref() == Some("3.12.12"))
        .unwrap();
    let p = f
        .preview(PlanRequest {
            snapshot_request_id: snapshot.request_id,
            intent: PlanIntent::Uninstall {
                installation_id: item.installation_id.clone(),
            },
        })
        .unwrap();
    assert!(p.capability.can_execute, "{:?}", p.capability);
    assert_eq!(f.execute(&p).unwrap().outcome, Some(Outcome::Uninstalled));
    assert!(!f
        .options
        .common
        .mise_data_dir
        .join("installs/python/3.12.12")
        .exists());
    assert!(f.executable.exists());
    assert_eq!(
        node,
        serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap()
    );
    assert_eq!(config, fs::read(&f.config).unwrap());
    let journal = fs::read_to_string(f.dir.path().join("journal/operations-v1.json")).unwrap();
    for value in [
        "PRIVATE_COMMAND_OUTPUT",
        "3.12.12",
        "argv",
        f.dir.path().to_str().unwrap(),
    ] {
        assert!(!journal.contains(value));
    }
}
#[test]
fn mise_failure_zero_exit_and_wrong_product_never_report_installation_success() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["permission", "zero", "wrong"] {
        let f = Fixture::new();
        setup(&f);
        let p = install(&f);
        assert!(p.capability.can_execute);
        fs::write(f.dir.path().join("mode"), mode).unwrap();
        assert!(f.execute(&p).is_err(), "{mode}");
        let record = f.service.records().0.remove(0);
        assert_ne!(record.status, ExecutionStatus::Completed);
        assert_ne!(record.outcome, Some(Outcome::Installed));
        assert!(record
            .steps
            .iter()
            .any(|s| s.kind == StepKind::Refresh && s.status == StepStatus::Completed));
    }
}
#[test]
fn cancelled_mise_reaps_process_reconciles_partial_install_and_returns_query_slot() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup(&f);
    let p = install(&f);
    fs::write(f.dir.path().join("mode"), "slow").unwrap();
    let start = Instant::now();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| f.execute(&p));
        while !f.dir.path().join("started").exists() && start.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(f.dir.path().join("started").exists());
        assert!(f.query.begin("must-wait".into(), None).is_err());
        operations::cancel(&p.plan_id).unwrap();
        assert!(worker.join().unwrap().is_err());
    });
    assert!(start.elapsed() < Duration::from_secs(4));
    let record = f.service.records().0.remove(0);
    assert_eq!(record.status, ExecutionStatus::Cancelled);
    assert!(record
        .steps
        .iter()
        .any(|s| s.kind == StepKind::Refresh && s.status == StepStatus::Completed));
    let (_, snapshot) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    assert!(snapshot
        .installations
        .iter()
        .any(|i| i.version.as_deref() == Some("3.12.12")
            && i.health == Health::Healthy
            && !i.stale));
    assert!(f.query.begin("after-cancel".into(), None).is_ok());
}
#[test]
fn changed_settings_receipt_packages_and_last_moment_registry_block_before_write() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["compile", "receipt", "packages", "late"] {
        let f = Fixture::new();
        setup(&f);
        let p = install(&f);
        assert!(p.capability.can_execute);
        match mode {
            "compile" | "late" => fs::write(f.dir.path().join("mode"), mode).unwrap(),
            "receipt" => fs::write(
                f.options
                    .common
                    .mise_data_dir
                    .join("installs/python/.mise.backend.toml"),
                "full='asdf:python'",
            )
            .unwrap(),
            "packages" => {
                let path = f
                    .options
                    .common
                    .mise_data_dir
                    .join("zenv-command-config/no-python-packages");
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "unexpected-package").unwrap();
            }
            _ => unreachable!(),
        };
        assert!(f.execute(&p).is_err(), "{mode}");
        assert!(!f.dir.path().join("command").exists(), "{mode}");
    }
}
#[test]
fn missing_python_source_requires_physical_absence_and_empty_native_records() {
    let f = Fixture::new();
    setup(&f);
    fs::remove_dir_all(f.options.common.mise_data_dir.join("installs/python")).unwrap();
    fs::write(f.dir.path().join("registry.json"), "[]").unwrap();
    f.query
        .begin_tool(
            "fresh-python".into(),
            Some(ProviderId::Mise),
            InventoryTool::Python,
        )
        .unwrap()
        .execute_python(f.options.clone())
        .unwrap();
    // An old cache is deliberately retained by an unavailable source; refresh an independent service to model a new machine.
    let query = InventoryQueryService::default();
    let s = query
        .begin_tool("fresh".into(), None, InventoryTool::Python)
        .unwrap()
        .execute_python(f.options.clone())
        .unwrap();
    let request = PlanRequest {
        snapshot_request_id: s.request_id,
        intent: PlanIntent::Install {
            provider_id: ProviderId::Mise,
            package_id: "python".into(),
            version: Some("3.12.12".into()),
        },
    };
    let p = f
        .service
        .preview_python(&query, request.clone(), f.options.clone(), f.config.clone())
        .unwrap();
    assert!(p.capability.can_execute, "{:?}", p.capability);
    fs::copy(
        f.dir.path().join("old.json"),
        f.dir.path().join("registry.json"),
    )
    .unwrap();
    let p = f
        .service
        .preview_python(&query, request, f.options.clone(), f.config.clone())
        .unwrap();
    assert!(!p.capability.can_execute);
}
#[test]
fn external_links_and_missing_core_receipt_never_gain_uninstall_permission() {
    use std::os::unix::fs::symlink;
    for mode in ["receipt", "link", "root"] {
        let f = Fixture::new();
        setup(&f);
        if mode == "receipt" {
            fs::remove_file(
                f.options
                    .common
                    .mise_data_dir
                    .join("installs/python/.mise.backend.toml"),
            )
            .unwrap();
        } else if mode == "link" {
            let locator = f.executable.parent().unwrap().parent().unwrap();
            let external = f.dir.path().join("external");
            fs::rename(locator, &external).unwrap();
            symlink(&external, locator).unwrap();
        } else {
            let root = f.options.common.mise_data_dir.join("installs/python");
            let external = f.dir.path().join("external-root");
            fs::rename(&root, &external).unwrap();
            symlink(&external, &root).unwrap();
        }
        let p = f.preview(f.request(Action::Uninstall)).unwrap();
        assert!(!p.capability.can_execute, "{mode}");
        if mode == "receipt" {
            assert!(!install(&f).capability.can_execute);
            assert!(f.executable.exists());
        }
        assert!(!f.dir.path().join("uninstall-command").exists());
    }
}
#[test]
fn shared_python_mise_contract_has_fixed_core_commands_and_explicit_environment() {
    let values: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/operations/python-mise-v1.json"
    ))
    .unwrap();
    for key in ["installPlan", "uninstallPlan"] {
        let p: OperationPlan = serde_json::from_value(values[key].clone()).unwrap();
        assert_eq!(p.tool_id, InventoryTool::Python);
        assert_eq!(p.target.provider_package.provider_id, ProviderId::Mise);
        assert_eq!(p.target.provider_package.package_id, "python");
        assert!(p.capability.can_execute);
        assert_eq!(
            &p.steps[1].argv[..3],
            &["--no-config", "--no-env", "--no-hooks"]
        );
        assert_eq!(p.steps[1].argv[4], "core:python@3.12.12");
        assert!(p.steps[1]
            .environment_keys
            .contains(&"MISE_PYTHON_COMPILE".into()));
        assert_eq!(serde_json::to_value(&p).unwrap(), values[key]);
    }
}
