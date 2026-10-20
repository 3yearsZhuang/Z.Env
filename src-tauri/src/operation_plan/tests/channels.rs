#[cfg(unix)]
use super::*;
#[cfg(unix)]
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
#[cfg(unix)]
fn script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
#[cfg(unix)]
fn node(path: &Path, version: &str) {
    script(
        path,
        &format!(
            "#!/bin/sh\nprintf '%s' '{}'\n",
            serde_json::json!({"version":version,"architecture":"arm64"})
        ),
    );
}
#[cfg(unix)]
fn setup_mise(f: &Fixture) {
    let root = f._dir.path();
    let installs = f.options.mise_data_dir.join("installs/node");
    let locator = installs.join("22.1.0");
    let rows = serde_json::to_string(
        &serde_json::json!([{"version":"22.1.0","install_path":locator,"installed":true}]),
    )
    .unwrap();
    let template = root.join("template-node");
    node(&template, "22.1.0");
    let text = format!(
        r##"#!/bin/sh
ROOT={root}
DATA={data}
if [ "$1" = '--help' ]; then printf '%s' '--no-config --no-env --no-hooks'; exit 0; fi
shift 3
case "$1" in
ls) if [ -f "$ROOT/registry.json" ]; then /bin/cat "$ROOT/registry.json"; else printf '[]'; fi ;;
install)
 printf '%s' "${{NODE_OPTIONS-unset}}|${{BASH_ENV-unset}}|$MISE_NO_CONFIG|$MISE_INSTALLS_DIR" > "$ROOT/environment"
 printf '%s\n' "$@" > "$ROOT/argv"
 mode=normal; [ ! -f "$ROOT/mode" ] || mode=$(/bin/cat "$ROOT/mode")
 [ "$mode" != permission ] || exit 13
 [ "$mode" != zero ] || exit 0
 /bin/mkdir -p "$DATA/installs/node/22.1.0/bin"
 /bin/cp "$ROOT/template-node" "$DATA/installs/node/22.1.0/bin/node"
 printf '%s' 'short = "node"
full = "core:node"
explicit_backend = true' > "$DATA/installs/node/.mise.backend.toml"
 printf '%s' {rows} > "$ROOT/registry.json"
 if [ "$mode" = wrong ]; then /bin/cp "$ROOT/wrong-node" "$DATA/installs/node/22.1.0/bin/node"; fi
 /usr/bin/touch "$ROOT/started"
 [ "$mode" != fail ] || exit 17
 if [ "$mode" = hold ]; then sleep 30 & wait; fi
 if [ "$mode" = output ]; then while :; do printf 'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX'; done; fi
 ;;
uninstall) printf '%s\n' "$@" > "$ROOT/uninstall-argv"; /bin/rm -rf "$DATA/installs/node/22.1.0"; printf '[]' > "$ROOT/registry.json" ;;
*) exit 99;;
esac
"##,
        root = quote(&root.to_string_lossy()),
        data = quote(&f.options.mise_data_dir.to_string_lossy()),
        rows = quote(&rows)
    );
    script(&root.join("bin/mise"), &text);
    node(&root.join("wrong-node"), "20.1.0");
}
#[cfg(unix)]
fn preview_install(f: &Fixture) -> OperationPlan {
    f.service
        .preview(
            &f.query,
            f.request(Action::Install),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap()
}
#[cfg(unix)]
pub(super) fn execute_native(f: &Fixture, p: &OperationPlan) -> Result<ExecutionRecord, PlanError> {
    f.service
        .execute_native_with_context(&p.plan_id, f.query.clone(), || {
            Ok((f.options.clone(), f.config.clone()))
        })
}
#[cfg(unix)]
fn request_uninstall(f: &Fixture, provider: ProviderId) -> PlanRequest {
    let (_, s) = f.query.current_snapshot().unwrap();
    PlanRequest {
        snapshot_request_id: s.request_id,
        intent: PlanIntent::Uninstall {
            installation_id: s
                .installations
                .iter()
                .find(|i| i.provider_package.provider_id == provider)
                .unwrap()
                .installation_id
                .clone(),
        },
    }
}
#[cfg(unix)]
#[test]
fn native_mise_install_and_uninstall_use_exact_core_arguments_and_preserve_configuration() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup_mise(&f);
    let before = std::fs::read(&f.config).unwrap();
    let plan = preview_install(&f);
    assert!(plan.capability.can_execute, "{:?}", plan.capability.reason);
    assert_eq!(
        plan.steps[1].argv,
        [
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "core:node@22.1.0"
        ]
    );
    let result = execute_native(&f, &plan).unwrap();
    assert_eq!(result.outcome, Some(Outcome::Installed));
    let (_, snapshot) = f.query.current_snapshot().unwrap();
    assert!(snapshot
        .installations
        .iter()
        .any(|i| i.provider_package.provider_id == ProviderId::Mise
            && i.version.as_deref() == Some("22.1.0")));
    assert!(snapshot
        .installations
        .iter()
        .any(|i| i.provider_package.provider_id == ProviderId::Nvm));
    let env = std::fs::read_to_string(f._dir.path().join("environment")).unwrap();
    assert!(env.starts_with("unset|unset|1|"));
    let uninstall = f
        .service
        .preview(
            &f.query,
            request_uninstall(&f, ProviderId::Mise),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(
        uninstall.capability.can_execute,
        "{:?}",
        uninstall.capability.reason
    );
    assert_eq!(uninstall.steps[1].argv.last().unwrap(), "core:node@22.1.0");
    assert_eq!(
        execute_native(&f, &uninstall).unwrap().outcome,
        Some(Outcome::Uninstalled)
    );
    assert!(!f
        .options
        .mise_data_dir
        .join("installs/node/22.1.0")
        .exists());
    assert!(f
        .options
        .nvm_dir
        .join("versions/node/v22.1.0/bin/node")
        .exists());
    assert_eq!(std::fs::read(f.config).unwrap(), before);
}
#[cfg(unix)]
#[test]
fn exit_zero_without_install_or_wrong_version_never_reports_installed() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["zero", "wrong", "permission", "fail", "output"] {
        let f = Fixture::new();
        setup_mise(&f);
        std::fs::write(f._dir.path().join("mode"), mode).unwrap();
        let p = preview_install(&f);
        assert!(execute_native(&f, &p).is_err(), "{mode}");
        let r = f.service.records().0.remove(0);
        assert_eq!(r.status, ExecutionStatus::Partial, "{mode}");
        assert!(
            r.steps
                .iter()
                .any(|s| s.kind == StepKind::Refresh && s.status == StepStatus::Completed),
            "{mode}"
        );
        assert_ne!(r.outcome, Some(Outcome::Installed));
        let (_, s) = f.query.current_snapshot().unwrap();
        assert!(ready(&s, ProviderId::Mise));
        if mode == "fail" || mode == "output" {
            assert!(s
                .installations
                .iter()
                .any(|i| i.provider_package.provider_id == ProviderId::Mise));
        }
    }
}
#[cfg(unix)]
#[test]
fn cancellation_reaps_command_and_still_reconciles_the_actual_partial_install() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup_mise(&f);
    std::fs::write(f._dir.path().join("mode"), "hold").unwrap();
    let p = preview_install(&f);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| execute_native(&f, &p));
        let start = Instant::now();
        while !f._dir.path().join("started").exists() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        operations::cancel(&p.plan_id).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
    });
    let r = f.service.records().0.remove(0);
    assert_eq!(r.status, ExecutionStatus::Cancelled);
    assert!(r
        .steps
        .iter()
        .any(|s| s.kind == StepKind::Refresh && s.status == StepStatus::Completed));
    let (_, s) = f.query.current_snapshot().unwrap();
    assert!(s
        .installations
        .iter()
        .any(|i| i.provider_package.provider_id == ProviderId::Mise && !i.stale));
}
#[cfg(unix)]
#[test]
fn unmanaged_mise_links_and_preexisting_installations_are_not_overwritten_or_deleted() {
    use std::os::unix::fs::symlink;
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup_mise(&f);
    let p = preview_install(&f);
    execute_native(&f, &p).unwrap();
    assert!(!preview_install(&f).capability.can_execute);
    let locator = f.options.mise_data_dir.join("installs/node/22.1.0");
    std::fs::remove_dir_all(&locator).unwrap();
    symlink(f.options.nvm_dir.join("versions/node/v22.1.0"), &locator).unwrap();
    f.query
        .begin("linked".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
    let p = f
        .service
        .preview(
            &f.query,
            request_uninstall(&f, ProviderId::Mise),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(!p.capability.can_execute);
    assert!(f
        .options
        .nvm_dir
        .join("versions/node/v22.1.0/bin/node")
        .exists());
}
#[cfg(unix)]
#[test]
fn revalidation_refuses_manager_registry_changes_before_command() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup_mise(&f);
    let p = preview_install(&f);
    std::fs::write(
        f._dir.path().join("registry.json"),
        r#"[{"version":"20.1.0","install_path":"/other/20","installed":true}]"#,
    )
    .unwrap();
    assert!(execute_native(&f, &p).is_err());
    assert!(!f._dir.path().join("started").exists());
    assert_eq!(f.service.records().0[0].status, ExecutionStatus::Failed);
}

#[cfg(unix)]
fn brew_info(installed: Vec<&str>) -> serde_json::Value {
    serde_json::json!({"formulae":[{"name":"node@22","full_name":"node@22","tap":"homebrew/core","versions":{"stable":"22.1.0","head":null},"revision":0,"dependencies":["openssl@3"],"installed":installed.iter().map(|v|serde_json::json!({"version":v})).collect::<Vec<_>>(),"linked_keg":null,"keg_only":true,"disabled":false}]})
}
#[cfg(unix)]
pub(super) fn setup_brew(f: &mut Fixture) {
    let root = f._dir.path();
    let prefix = root.join("brew-prefix");
    let cellar = prefix.join("Cellar");
    std::fs::create_dir_all(&cellar).unwrap();
    f.options.brew_cellars = vec![cellar.clone()];
    f.options.path_entries.insert(0, prefix.join("bin"));
    node(&root.join("template-node"), "22.1.0");
    std::fs::write(
        root.join("brew-info.json"),
        serde_json::to_vec(&brew_info(vec![])).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("brew-info-installed.json"),
        serde_json::to_vec(&brew_info(vec!["22.1.0"])).unwrap(),
    )
    .unwrap();
    script(
        &prefix.join("bin/brew"),
        &format!(
            r##"#!/bin/sh
ROOT={root}
PREFIX={prefix}
case "$1" in
--prefix) printf '%s' "$PREFIX" ;;
--cellar) printf '%s' "$PREFIX/Cellar" ;;
info) /bin/cat "$ROOT/brew-info.json" ;;
deps) printf 'openssl@3\n' ;;
uses) [ ! -f "$ROOT/dependents" ] || /bin/cat "$ROOT/dependents" ;;
install)
 printf '%s\n' "$@" > "$ROOT/brew-argv"
 printf '%s' "${{NODE_OPTIONS-unset}}|$HOMEBREW_NO_AUTOREMOVE|$HOMEBREW_NO_INSTALL_CLEANUP" > "$ROOT/brew-environment"
 /bin/mkdir -p "$PREFIX/Cellar/node@22/22.1.0/bin"
 /bin/cp "$ROOT/template-node" "$PREFIX/Cellar/node@22/22.1.0/bin/node"
 printf '%s' '{{"source":{{"tap":"homebrew/core","path":"/fixture/node@22.rb"}}}}' > "$PREFIX/Cellar/node@22/22.1.0/INSTALL_RECEIPT.json"
 /bin/cp "$ROOT/brew-info-installed.json" "$ROOT/brew-info.json"
 ;;
uninstall) printf '%s\n' "$@" > "$ROOT/brew-uninstall-argv"; /bin/rm -rf "$PREFIX/Cellar/node@22/22.1.0"; printf '%s' {empty} > "$ROOT/brew-info.json" ;;
*) exit 98;;
esac
"##,
            root = quote(&root.to_string_lossy()),
            prefix = quote(&prefix.to_string_lossy()),
            empty = quote(&brew_info(vec![]).to_string())
        ),
    );
    f.query
        .begin("brew-fixture".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
}
#[cfg(unix)]
pub(super) fn preview_brew(f: &Fixture) -> OperationPlan {
    let (_, s) = f.query.current_snapshot().unwrap();
    f.service
        .preview(
            &f.query,
            PlanRequest {
                snapshot_request_id: s.request_id,
                intent: PlanIntent::Install {
                    provider_id: ProviderId::Brew,
                    package_id: "node@22".into(),
                    version: None,
                },
            },
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap()
}
#[cfg(unix)]
#[test]
fn brew_channel_installs_without_linking_or_cleanup_and_uninstalls_only_the_selected_keg() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    setup_brew(&mut f);
    let p = preview_brew(&f);
    assert!(p.capability.can_execute, "{:?}", p.capability.reason);
    assert_eq!(
        p.steps[1].argv,
        [
            "install",
            "--formula",
            "--skip-link",
            "--skip-post-install",
            "node@22"
        ]
    );
    assert!(p.impact.iter().any(|s| s.contains("openssl@3")));
    assert_eq!(
        execute_native(&f, &p).unwrap().outcome,
        Some(Outcome::Installed)
    );
    assert_eq!(
        std::fs::read_to_string(f._dir.path().join("brew-environment")).unwrap(),
        "unset|1|1"
    );
    let p = f
        .service
        .preview(
            &f.query,
            request_uninstall(&f, ProviderId::Brew),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(p.capability.can_execute, "{:?}", p.capability.reason);
    assert_eq!(p.steps[1].argv, ["uninstall", "--formula", "node@22"]);
    // 渠道最新版不能覆盖用户选中实例的卸载版本承诺。
    let mut newer = brew_info(vec!["22.1.0"]);
    newer["formulae"][0]["versions"]["stable"] = "22.2.0".into();
    std::fs::write(f._dir.path().join("brew-info.json"), newer.to_string()).unwrap();
    let p = f
        .service
        .preview(
            &f.query,
            request_uninstall(&f, ProviderId::Brew),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(p.version_promise.contains("22.1.0"));
    assert!(!p.version_promise.contains("22.2.0"));
    assert_eq!(
        execute_native(&f, &p).unwrap().outcome,
        Some(Outcome::Uninstalled)
    );
    assert!(f
        .options
        .nvm_dir
        .join("versions/node/v22.1.0/bin/node")
        .exists());
}
#[cfg(unix)]
#[test]
fn brew_dependents_multiple_kegs_third_party_metadata_and_environment_policy_block_execution() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["dependents", "multiple", "third-party", "environment"] {
        let mut f = Fixture::new();
        setup_brew(&mut f);
        let p = preview_brew(&f);
        execute_native(&f, &p).unwrap();
        match mode {
            "dependents" => std::fs::write(f._dir.path().join("dependents"), "my-app\n").unwrap(),
            "multiple" => std::fs::write(
                f._dir.path().join("brew-info.json"),
                brew_info(vec!["22.1.0", "22.0.0"]).to_string(),
            )
            .unwrap(),
            "third-party" => {
                let mut info = brew_info(vec!["22.1.0"]);
                info["formulae"][0]["tap"] = "other/tap".into();
                std::fs::write(f._dir.path().join("brew-info.json"), info.to_string()).unwrap();
            }
            "environment" => {
                let file = f._dir.path().join("brew-prefix/etc/homebrew/brew.env");
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, "HOMEBREW_NO_AUTOREMOVE=0").unwrap();
            }
            _ => unreachable!(),
        }
        let p = f
            .service
            .preview(
                &f.query,
                request_uninstall(&f, ProviderId::Brew),
                f.options.clone(),
                f.config.clone(),
            )
            .unwrap();
        assert!(!p.capability.can_execute, "{mode}");
        assert!(f.options.brew_cellars[0]
            .join("node@22/22.1.0/bin/node")
            .exists());
        assert!(!f._dir.path().join("brew-uninstall-argv").exists());
    }
}
#[cfg(unix)]
#[test]
fn changed_brew_channel_metadata_invalidates_the_approved_plan() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    setup_brew(&mut f);
    let p = preview_brew(&f);
    let mut info = brew_info(vec![]);
    info["formulae"][0]["versions"]["stable"] = "22.2.0".into();
    std::fs::write(f._dir.path().join("brew-info.json"), info.to_string()).unwrap();
    assert!(execute_native(&f, &p).is_err());
    assert!(!f._dir.path().join("brew-argv").exists());
}
#[cfg(unix)]
#[test]
fn changed_brew_receipt_ownership_blocks_uninstall_even_when_manager_metadata_is_unchanged() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    setup_brew(&mut f);
    execute_native(&f, &preview_brew(&f)).unwrap();
    let plan = f
        .service
        .preview(
            &f.query,
            request_uninstall(&f, ProviderId::Brew),
            f.options.clone(),
            f.config.clone(),
        )
        .unwrap();
    assert!(plan.capability.can_execute);
    std::fs::write(
        f.options.brew_cellars[0].join("node@22/22.1.0/INSTALL_RECEIPT.json"),
        r#"{"source":{"tap":"other/tap"}}"#,
    )
    .unwrap();
    assert!(execute_native(&f, &plan).is_err());
    assert!(!f._dir.path().join("brew-uninstall-argv").exists());
    assert!(f.options.brew_cellars[0]
        .join("node@22/22.1.0/bin/node")
        .exists());
    assert_eq!(f.service.records().0[0].status, ExecutionStatus::Failed);
}
#[cfg(unix)]
#[test]
fn postscan_failure_leaves_installation_unconfirmed_instead_of_reporting_success() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    setup_mise(&f);
    f.options.limits.max_probes = 0;
    f.query
        .begin("budget-initial".into(), None)
        .unwrap()
        .execute(f.options.clone())
        .unwrap();
    let p = preview_install(&f);
    assert!(p.capability.can_execute);
    assert!(execute_native(&f, &p).is_err());
    let r = f.service.records().0.remove(0);
    assert_eq!(r.status, ExecutionStatus::Partial);
    assert_eq!(r.outcome, Some(Outcome::ScanIncomplete));
    let (_, s) = f.query.current_snapshot().unwrap();
    assert!(!ready(&s, ProviderId::Mise));
    assert_ne!(r.outcome, Some(Outcome::Installed));
}
#[cfg(unix)]
#[test]
fn mutating_command_panic_or_journal_failure_still_runs_readonly_reconciliation() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["panic", "journal"] {
        let f = Fixture::new();
        let p = f.plan();
        lock(&f.service.state).plans.back_mut().unwrap().view.steps[1].kind = StepKind::Command;
        // 加入明确的执行后重扫步骤，再用可控驱动注入终态故障。
        lock(&f.service.state)
            .plans
            .back_mut()
            .unwrap()
            .view
            .steps
            .insert(2, step(StepKind::Refresh, "重扫", &f.options.home));
        let calls = AtomicUsize::new(0);
        assert!(f
            .execute(&p.plan_id, |kind, _, _, _| {
                match kind {
                    StepKind::Command => {
                        if mode == "panic" {
                            panic!("模拟写命令恐慌");
                        }
                        std::fs::write(
                            f._dir.path().join("journal/operations-v1.json"),
                            "external-edit",
                        )
                        .unwrap();
                    }
                    StepKind::Refresh => {
                        calls.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
                Ok(())
            })
            .is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            operations::list()
                .iter()
                .find(|j| j.id == p.plan_id)
                .unwrap()
                .status,
            "interrupted"
        );
    }
}
#[cfg(unix)]
#[test]
fn write_runner_timeout_is_bounded_and_does_not_inherit_parent_injection() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    setup_mise(&f);
    std::fs::write(f._dir.path().join("mode"), "hold").unwrap();
    preview_install(&f);
    let spec = lock(&f.service.state)
        .plans
        .back()
        .unwrap()
        .write
        .clone()
        .unwrap();
    let started = Instant::now();
    assert!(spec
        .run_limited(
            &f.options,
            &CancellationToken::default(),
            &crate::process::probe::ProbeLimits {
                timeout: Duration::from_millis(100),
                max_output_bytes: 32768
            }
        )
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
}
