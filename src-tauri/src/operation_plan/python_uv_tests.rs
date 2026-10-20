#![cfg(target_os = "macos")]
use super::*;
use std::os::unix::fs::symlink;

/// 伪 uv 只实现本应用实际发送的参数：能力探测、精确安装与受支持卸载。
/// 所有写调用记录到 write-calls；安装/卸载受 mode 文件控制，便于故障场景。
fn fake_uv(f: &Fixture) -> (String, String) {
    let root = f.dir.path().display().to_string();
    let name = crate::providers::python::uv_dir_name("3.12.13").unwrap();
    let alias = crate::providers::python::uv_dir_name("3.12").unwrap();
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let text = r#"#!/bin/sh
ROOT='@ROOT@'
NAME='@NAME@'
ALIAS='@ALIAS@'
ARCH='@ARCH@'
if [ "$1" = '--version' ]; then printf 'uv 0.12.1 (fake)\n'; exit 0; fi
if [ "$1" = '--help' ]; then printf '%s\n' '--no-config --no-progress --color never'; exit 0; fi
if [ "$1" = 'python' ] && [ "$3" = '--help' ]; then
  if [ "$2" = 'install' ]; then printf '%s\n' '--no-bin --install-dir'; else printf '%s\n' '--install-dir'; fi
  exit 0
fi
MODE='normal'
[ -f "$ROOT/mode" ] && MODE=$(/bin/cat "$ROOT/mode")
printf '%s\n' "$*" >> "$ROOT/write-calls"
printf '%s|%s|%s|%s|%s|%s|%s\n' "$UV_PYTHON_INSTALL_DIR" "$UV_CACHE_DIR" "$UV_NO_CONFIG" "$UV_NO_PROGRESS" "$UV_COLOR" "${PYTHONHOME-unset}" "${PYTHONPATH-unset}" > "$ROOT/env"
if [ "$1" = '--no-config' ] && [ "$2" = '--no-progress' ] && [ "$3" = 'python' ]; then
  if [ "$4" = 'install' ]; then
    DIR="$7"
    TARGET="$8"
    VERSION=${TARGET#cpython@}
    [ "$MODE" = 'zero' ] && exit 0
    [ "$MODE" = 'permission' ] && exit 13
    SHOWN="$VERSION"
    [ "$MODE" = 'wrong' ] && SHOWN='3.12.99'
    /bin/mkdir -p "$DIR/$NAME/bin"
    /bin/cat > "$DIR/$NAME/bin/python3" <<PY
#!/bin/sh
printf '%s' '{"version":"$SHOWN","implementation":"cpython","releaseLevel":"final","freeThreaded":false,"debug":false,"architecture":"$ARCH","execPath":"$DIR/$NAME/bin/python3"}'
PY
    /bin/chmod 755 "$DIR/$NAME/bin/python3"
    /bin/ln -sf "$DIR/$NAME" "$DIR/$ALIAS"
    exit 0
  fi
  if [ "$4" = 'uninstall' ]; then
    DIR="$6"
    /bin/rm -rf "$DIR/$NAME"
    /bin/rm -f "$DIR/$ALIAS"
    exit 0
  fi
fi
exit 99
"#
    .replace("@ROOT@", &root)
    .replace("@NAME@", &name)
    .replace("@ALIAS@", &alias)
    .replace("@ARCH@", arch);
    script(&f.dir.path().join("bin/uv"), &text);
    (name, alias)
}
fn install_request(f: &Fixture) -> PlanRequest {
    let (_, snapshot) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    PlanRequest {
        snapshot_request_id: snapshot.request_id,
        intent: PlanIntent::Install {
            provider_id: ProviderId::Uv,
            package_id: "python".into(),
            version: Some("3.12.13".into()),
        },
    }
}
fn rescan(f: &Fixture) {
    f.query
        .begin_tool(
            "uv-rescan".into(),
            Some(ProviderId::Uv),
            InventoryTool::Python,
        )
        .unwrap()
        .execute_python(f.options.clone())
        .unwrap();
}
fn uv_target(f: &Fixture) -> (OperationPlan, PlanRequest) {
    let (_, snapshot) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    let target = snapshot
        .installations
        .iter()
        .find(|item| {
            item.provider_package.provider_id == ProviderId::Uv && item.health == Health::Healthy
        })
        .unwrap()
        .clone();
    let request = PlanRequest {
        snapshot_request_id: snapshot.request_id,
        intent: PlanIntent::Uninstall {
            installation_id: target.installation_id,
        },
    };
    (f.preview(request.clone()).unwrap(), request)
}
fn canonical_root(f: &Fixture) -> PathBuf {
    f.options.uv_python_dir.canonicalize().unwrap()
}
#[test]
fn uv_installs_and_uninstalls_exact_managed_target_without_touching_configuration() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let (name, alias) = fake_uv(&f);
    let root = canonical_root(&f);
    let before = fs::read_to_string(&f.config).unwrap();
    let p = f.preview(install_request(&f)).unwrap();
    assert!(p.capability.can_execute);
    assert!(p.capability.can_preview);
    assert_eq!(p.tool_id, InventoryTool::Python);
    assert_eq!(p.target.provider_package.provider_id, ProviderId::Uv);
    assert_eq!(
        p.steps[1].argv,
        vec![
            "--no-config",
            "--no-progress",
            "python",
            "install",
            "--no-bin",
            "--install-dir",
            root.to_str().unwrap(),
            "cpython@3.12.13",
        ]
    );
    assert!(p.steps[1]
        .environment_keys
        .iter()
        .any(|key| key == "UV_PYTHON_INSTALL_DIR"));
    assert!(!f.dir.path().join("journal").exists());
    let record = f.execute(&p).unwrap();
    assert_eq!(record.status, ExecutionStatus::Completed);
    assert_eq!(record.outcome, Some(Outcome::Installed));
    assert!(root.join(&name).is_dir());
    assert!(root.join(&alias).is_symlink());
    assert_eq!(fs::read_to_string(&f.config).unwrap(), before);
    let env = fs::read_to_string(f.dir.path().join("env")).unwrap();
    assert_eq!(
        env.trim(),
        format!(
            "{}|{}|1|1|never|unset|unset",
            root.display(),
            f.options.common.home.join(".cache/uv").display()
        )
    );
    let (plan, _) = uv_target(&f);
    assert!(plan.capability.can_execute);
    assert_eq!(plan.action, Action::Uninstall);
    let record = f.execute(&plan).unwrap();
    assert_eq!(record.status, ExecutionStatus::Completed);
    assert_eq!(record.outcome, Some(Outcome::Uninstalled));
    assert!(!root.join(&name).exists());
    assert!(!root.join(&alias).exists());
    assert_eq!(fs::read_to_string(&f.config).unwrap(), before);
}
#[test]
fn uv_existing_version_unknown_layout_and_ranges_never_gain_write_permission() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    fake_uv(&f);
    let root = canonical_root(&f);
    fs::create_dir_all(root.join("cpython-3.12.13-macos-aarch64-none/bin")).unwrap();
    let p = f.preview(install_request(&f)).unwrap();
    assert!(!p.capability.can_execute);
    assert!(p.capability.reason.unwrap().contains("不覆盖"));
    fs::remove_dir_all(root.join("cpython-3.12.13-macos-aarch64-none")).unwrap();
    fs::create_dir_all(root.join("pypy-3.12-macos-aarch64-none")).unwrap();
    let p = f.preview(install_request(&f)).unwrap();
    assert!(!p.capability.can_execute);
    assert!(p.capability.reason.is_some());
    for version in ["3.12", "latest", "3.12.13+freethreaded", "3.12.13;rm -rf /"] {
        let mut request = install_request(&f);
        if let PlanIntent::Install { version: slot, .. } = &mut request.intent {
            *slot = Some(version.into());
        }
        assert!(f.preview(request).is_err(), "{version}");
    }
}
#[test]
fn uv_zero_exit_and_wrong_probe_product_never_report_success() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["zero", "wrong", "permission"] {
        let f = Fixture::new();
        let (name, _) = fake_uv(&f);
        fs::write(f.dir.path().join("mode"), mode).unwrap();
        let p = f.preview(install_request(&f)).unwrap();
        assert!(p.capability.can_execute, "{mode}");
        let error = f.execute(&p).unwrap_err();
        assert!(
            matches!(
                error,
                PlanError::Verification | PlanError::Command | PlanError::ScanIncomplete
            ),
            "{mode}: {error}"
        );
        let record = f.service.records().0.remove(0);
        assert_ne!(record.status, ExecutionStatus::Completed, "{mode}");
        assert!(!record
            .steps
            .iter()
            .all(|step| step.status == StepStatus::Completed));
        if mode == "zero" {
            let root = canonical_root(&f);
            assert!(!root.join(&name).exists());
        }
    }
}
#[test]
fn uv_uninstall_requires_real_exact_instance_and_refuses_external_link() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    fake_uv(&f);
    let root = canonical_root(&f);
    let name = crate::providers::python::uv_dir_name("3.12.13").unwrap();
    let outside = f.dir.path().join("outside");
    fs::create_dir_all(outside.join(&name).join("bin")).unwrap();
    let output = serde_json::json!({"version":"3.12.13","implementation":"cpython","releaseLevel":"final","freeThreaded":false,"debug":false,"architecture":"arm64","execPath":outside.join(&name).join("bin/python3")});
    script(
        &outside.join(&name).join("bin/python3"),
        &format!("#!/bin/sh\nprintf '%s' '{}'\n", output),
    );
    symlink(outside.join(&name), root.join(&name)).unwrap();
    rescan(&f);
    let (_plan, request) = uv_target(&f);
    let p = f.preview(request).unwrap();
    assert!(!p.capability.can_execute);
    assert!(p.capability.reason.is_some());
    assert!(f.execute(&p).is_err());
}
#[test]
fn uv_preview_rechecks_the_managed_directory_before_the_command_step() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    fake_uv(&f);
    let root = canonical_root(&f);
    let p = f.preview(install_request(&f)).unwrap();
    assert!(p.capability.can_execute);
    fs::create_dir_all(root.join("cpython-3.12.13-macos-aarch64-none/bin")).unwrap();
    assert!(f.execute(&p).is_err());
}
#[test]
fn shared_python_uv_contract_has_fixed_managed_commands_and_explicit_environment() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/operations/python-uv-v1.json"
    ))
    .unwrap();
    for key in ["installPlan", "uninstallPlan"] {
        let plan: OperationPlan = serde_json::from_value(value[key].clone()).unwrap();
        assert_eq!(plan.tool_id, InventoryTool::Python);
        assert_eq!(plan.target.provider_package.provider_id, ProviderId::Uv);
        assert!(plan.capability.can_execute);
        let command = &plan.steps[1];
        assert_eq!(command.program.as_deref(), Some("/fixture/bin/uv"));
        let mut expected = vec!["--no-config", "--no-progress", "python"];
        if key == "installPlan" {
            expected.extend(["install", "--no-bin"]);
        } else {
            expected.push("uninstall");
        }
        expected.extend([
            "--install-dir",
            "/fixture/.local/share/uv/python",
            "cpython@3.13.15",
        ]);
        assert_eq!(command.argv, expected);
        assert!(command
            .environment_keys
            .iter()
            .any(|key| key == "UV_PYTHON_INSTALL_DIR"));
        let json = serde_json::to_string(&plan).unwrap();
        assert!(!json.contains("PRIVATE"));
    }
}
