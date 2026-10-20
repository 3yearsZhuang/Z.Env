use super::*;
use crate::inventory::query::InventoryQueryService;
use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant},
};
struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    global: PathBuf,
    options: NodeScanOptions,
    snapshot: InventorySnapshot,
}
fn script(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
fn node(path: &Path, version: &str) {
    script(
        path,
        &format!(
            "#!/bin/sh\nprintf '%s' '{}'\n",
            serde_json::json!({"version":version,"architecture":"arm64","execPath":path})
        ),
    );
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("projects/demo");
        std::fs::create_dir_all(&project).unwrap();
        let data = home.join("mise");
        let nvm = home.join("nvm");
        node(&data.join("installs/node/22.1.0/bin/node"), "22.1.0");
        node(&nvm.join("versions/node/v20.1.0/bin/node"), "20.1.0");
        let bin = root.path().join("bin");
        script(
            &bin.join("mise"),
            &format!(
                r##"#!/bin/sh
ROOT='{root}'
if [ "$1" = '--help' ];then printf '%s' '--no-config --no-env --no-hooks';exit 0;fi
shift 3
if [ "$1" != exec ];then /usr/bin/touch "$ROOT/unexpected-command";exit 99;fi
printf '%s\n' "$@" > "$ROOT/argv"
printf '%s' "$MISE_NO_CONFIG|$MISE_NO_ENV|$MISE_NO_HOOKS|$MISE_AUTO_INSTALL|$MISE_EXEC_AUTO_INSTALL|$MISE_NOT_FOUND_AUTO_INSTALL|${{NODE_OPTIONS-unset}}" > "$ROOT/environment"
/usr/bin/touch "$ROOT/started"
if [ -f "$ROOT/hold" ];then /bin/sleep 30 & wait;fi
if [ -f "$ROOT/wrong" ];then "$ROOT/home/nvm/versions/node/v20.1.0/bin/node";else "$ROOT/home/mise/installs/node/22.1.0/bin/node";fi
"##,
                root = root.path().display()
            ),
        );
        let options = NodeScanOptions {
            platform: Platform::Macos,
            home: home.clone(),
            mise_data_dir: data,
            nvm_dir: nvm.clone(),
            brew_cellars: vec![root.path().join("Cellar")],
            path_entries: vec![nvm.join("versions/node/v20.1.0/bin"), bin],
            limits: Default::default(),
        };
        std::fs::create_dir_all(&options.brew_cellars[0]).unwrap();
        let query = InventoryQueryService::default();
        let snapshot = query
            .begin("fixture".into(), None)
            .unwrap()
            .execute(options.clone())
            .unwrap();
        let global = home.join(".config/mise/config.toml");
        std::fs::write(project.join("mise.toml"), "[tools]\nnode='22'\n").unwrap();
        std::fs::write(
            project.join("package.json"),
            r#"{"engines":{"node":">=22 <23"}}"#,
        )
        .unwrap();
        Self {
            _root: root,
            project,
            global,
            options,
            snapshot,
        }
    }
    fn request(&self, probe: bool) -> EnvironmentRequest {
        EnvironmentRequest {
            request_id: "assessment".into(),
            snapshot_request_id: self.snapshot.request_id.clone(),
            path: self.project.to_string_lossy().into(),
            probe,
        }
    }
    fn inspect(&self, probe: bool) -> ProjectEnvironment {
        inspect(
            &self.request(probe),
            &self.snapshot,
            &self.options,
            &self.global,
            &Default::default(),
        )
        .unwrap()
    }
}
#[test]
fn node_22_requirement_and_candidate_never_hide_application_path_node_20() {
    let f = Fixture::new();
    let read = f.inspect(false);
    assert_eq!(read.requirement_state, RequirementState::Supported);
    assert_eq!(read.controlled.status, "notRequested");
    assert!(!f._root.path().join("started").exists());
    let result = f.inspect(true);
    assert_eq!(result.controlled.version.as_deref(), Some("22.1.0"));
    assert_eq!(result.controlled.requirement_match, Match::Satisfied);
    assert_eq!(result.application_path.version.as_deref(), Some("20.1.0"));
    assert_eq!(result.application_path.requirement_match, Match::Mismatch);
    assert!(result.issues.iter().any(|i| i.code == "runtimeMismatch"));
    assert_eq!(result.terminal.status, "unknown");
    assert_eq!(
        result
            .candidates
            .iter()
            .filter(|c| c.usable && c.requirement_match == Match::Satisfied)
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(f._root.path().join("environment")).unwrap(),
        "1|1|1|0|0|0|unset"
    );
    assert!(!f._root.path().join("unexpected-command").exists());
}
#[test]
fn mismatching_controlled_runtime_is_evidence_of_failure_even_when_node_22_is_installed() {
    let f = Fixture::new();
    std::fs::write(f._root.path().join("wrong"), "").unwrap();
    let report = f.inspect(true);
    assert_eq!(report.controlled.status, "verified");
    assert_eq!(report.controlled.version.as_deref(), Some("20.1.0"));
    assert_eq!(report.controlled.requirement_match, Match::Mismatch);
    assert_eq!(report.controlled.selection_match, Match::Mismatch);
}
#[test]
fn declarations_conflict_without_treating_missing_installed_intersections_as_proof() {
    let f = Fixture::new();
    std::fs::write(f.project.join(".nvmrc"), "20").unwrap();
    let report = f.inspect(true);
    assert_eq!(report.requirement_state, RequirementState::Conflict);
    assert_eq!(report.controlled.status, "unknown");
    assert!(!f._root.path().join("started").exists());
}
#[test]
fn aliases_complex_node_configuration_and_variant_inheritance_stay_unknown() {
    for mode in ["alias", "complex", "variant", "parent"] {
        let f = Fixture::new();
        match mode {
            "alias" => {
                std::fs::write(f.project.join("mise.toml"), "[tools]\nnode='lts/*'\n").unwrap()
            }
            "complex" => std::fs::write(
                f.project.join("mise.toml"),
                "[tools]\nnode={version='22', os=['macos']}\n",
            )
            .unwrap(),
            "variant" => {
                std::fs::write(f.project.join("mise.local.toml"), "[tools]\nnode='20'\n").unwrap()
            }
            "parent" => std::fs::write(
                f.project.parent().unwrap().join("mise.toml"),
                "[tools]\nnode='20'\n",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let report = f.inspect(true);
        assert_eq!(report.selection.kind, "unknown", "{mode}");
        assert_eq!(report.controlled.status, "unknown", "{mode}");
        assert!(!f._root.path().join("started").exists(), "{mode}");
    }
}
#[test]
fn unknown_path_first_entry_is_not_executed_or_skipped_for_a_healthy_later_node() {
    let mut f = Fixture::new();
    let bin = f._root.path().join("unknown");
    let marker = f._root.path().join("unsafe-called");
    script(
        &bin.join("node"),
        &format!("#!/bin/sh\n/usr/bin/touch '{}'\n", marker.display()),
    );
    f.options.path_entries.insert(0, bin.clone());
    let report = f.inspect(true);
    assert_eq!(report.application_path.status, "unverified");
    assert_eq!(
        report.application_path.executable_path,
        Some(bin.join("node").to_string_lossy().into())
    );
    assert!(!marker.exists());
}
#[test]
fn project_path_projection_can_mismatch_requirements_while_brew_or_mise_candidate_matches() {
    let f = Fixture::new();
    let bin = f.options.nvm_dir.join("versions/node/v20.1.0/bin");
    std::fs::write(
        f.project.join("mise.toml"),
        format!("[env]\n_.path=['{}']\n", bin.display()),
    )
    .unwrap();
    let report = f.inspect(true);
    assert_eq!(report.selection.kind, "path");
    assert_eq!(report.controlled.version.as_deref(), Some("20.1.0"));
    assert_eq!(report.controlled.requirement_match, Match::Mismatch);
    assert!(report
        .candidates
        .iter()
        .any(|c| c.usable && c.requirement_match == Match::Satisfied));
}
#[test]
fn stale_or_incomplete_provider_never_supplies_a_usable_candidate_or_runtime_authority() {
    let mut f = Fixture::new();
    for p in &mut f.snapshot.providers {
        p.stale = true;
        p.coverage.complete = false;
    }
    let report = f.inspect(true);
    assert!(report.candidates.iter().all(|c| !c.usable));
    assert!(!report.inventory_complete);
    assert_eq!(report.application_path.status, "unverified");
    assert_eq!(report.controlled.status, "unavailable");
    assert!(!f._root.path().join("started").exists());
}
#[test]
fn source_file_change_during_context_execution_invalidates_the_whole_project_report() {
    let f = Fixture::new();
    let node_path = f.options.nvm_dir.join("versions/node/v20.1.0/bin/node");
    script(
        &node_path,
        &format!(
            "#!/bin/sh\nprintf \"[tools]\\nnode='20'\\n\" > '{}'\nprintf '%s' '{}'\n",
            f.project.join("mise.toml").display(),
            serde_json::json!({"version":"20.1.0","architecture":"arm64","execPath":node_path})
        ),
    );
    let error = inspect(
        &f.request(true),
        &f.snapshot,
        &f.options,
        &f.global,
        &Default::default(),
    )
    .unwrap_err();
    assert!(error.contains("变化"));
    assert!(std::fs::read_to_string(f.project.join("mise.toml"))
        .unwrap()
        .contains("'20'"));
}
#[test]
fn cancellation_reaps_a_controlled_process_and_returns_no_late_success() {
    let f = Fixture::new();
    std::fs::write(f._root.path().join("hold"), "").unwrap();
    let token = CancellationToken::default();
    std::thread::scope(|scope| {
        let worker =
            scope.spawn(|| inspect(&f.request(true), &f.snapshot, &f.options, &f.global, &token));
        let start = Instant::now();
        while !f._root.path().join("started").exists() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        token.cancel();
        assert!(worker.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    });
}
#[test]
fn oversized_and_special_declarations_are_bounded_and_cannot_be_treated_as_absent() {
    for fifo in [false, true] {
        let f = Fixture::new();
        let path = f.project.join(".nvmrc");
        if fifo {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        } else {
            std::fs::write(&path, "2".repeat(128 * 1024 + 1)).unwrap();
        }
        let start = Instant::now();
        let report = f.inspect(false);
        assert_eq!(report.requirement_state, RequirementState::Unknown);
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    let f = Fixture::new();
    std::fs::write(f.project.join(".tool-versions"), "node 22\n".repeat(200)).unwrap();
    let report = f.inspect(false);
    assert_eq!(report.requirement_state, RequirementState::Unknown);
    assert_eq!(report.declarations.len(), 64);
    assert!(report.issues.iter().any(|i| i.code == "declarationBudget"));
    assert!(report
        .candidates
        .iter()
        .all(|c| c.requirement_match != Match::Satisfied));
}
#[test]
fn global_default_is_a_selection_and_not_an_additional_project_requirement() {
    let f = Fixture::new();
    std::fs::remove_file(f.project.join("mise.toml")).unwrap();
    std::fs::create_dir_all(f.global.parent().unwrap()).unwrap();
    std::fs::write(
        &f.global,
        "[tools]\nnode='20'\n[env]\nSECRET='must-not-leak'\n",
    )
    .unwrap();
    let report = f.inspect(false);
    assert_eq!(report.requirement_state, RequirementState::Supported);
    assert_eq!(report.selection.scope, "global");
    assert_eq!(report.selection.request.as_deref(), Some("20"));
    assert_eq!(report.declarations.len(), 1);
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("must-not-leak"));
}

#[test]
fn shared_project_contract_preserves_independent_contexts_and_rejects_frontend_commands() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/project-environment/contract-v1.json"
    ))
    .unwrap();
    let report: ProjectEnvironment = serde_json::from_value(fixture["report"].clone()).unwrap();
    assert_eq!(serde_json::to_value(report).unwrap(), fixture["report"]);
    let _: EnvironmentRequest = serde_json::from_value(fixture["request"].clone()).unwrap();
    let mut unsafe_request = fixture["request"].clone();
    unsafe_request["argv"] = serde_json::json!(["install"]);
    assert!(serde_json::from_value::<EnvironmentRequest>(unsafe_request).is_err());
}

#[test]
fn new_configuration_variant_during_probe_invalidates_report_and_preserves_the_new_file() {
    let f = Fixture::new();
    let variant = f.project.join("mise.local.toml");
    let path = f.options.nvm_dir.join("versions/node/v20.1.0/bin/node");
    script(
        &path,
        &format!(
            "#!/bin/sh\nprintf x > '{}'\nprintf '%s' '{}'\n",
            variant.display(),
            serde_json::json!({"version":"20.1.0","execPath":path})
        ),
    );
    let error = inspect(
        &f.request(true),
        &f.snapshot,
        &f.options,
        &f.global,
        &Default::default(),
    )
    .unwrap_err();
    assert!(error.contains("目录成员"));
    assert_eq!(std::fs::read_to_string(variant).unwrap(), "x");
}
