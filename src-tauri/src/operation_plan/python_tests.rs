#![cfg(unix)]
use super::*;
use crate::providers::python::PythonScanOptions;
use std::{fs, os::unix::fs::PermissionsExt};
struct Fixture {
    dir: tempfile::TempDir,
    options: PythonScanOptions,
    config: PathBuf,
    query: InventoryQueryService,
    service: OperationPlanService,
    executable: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let options = PythonScanOptions {
            common: NodeScanOptions {
                platform: Platform::Macos,
                home: root.to_owned(),
                mise_data_dir: root.join("mise"),
                nvm_dir: root.join("nvm"),
                brew_cellars: vec![root.join("Cellar")],
                path_entries: vec![root.join("bin")],
                limits: Default::default(),
            },
            pyenv_root: root.join("pyenv"),
            uv_python_dir: root.join("uv/python"),
        };
        for path in [
            &options.common.brew_cellars[0],
            &options.pyenv_root.join("versions"),
            &options.uv_python_dir,
            &root.join("bin"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        let executable = options
            .common
            .mise_data_dir
            .join("installs/python/3.13.15/bin/python3");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let output = serde_json::json!({"version":"3.13.15","implementation":"cpython","releaseLevel":"final","freeThreaded":false,"debug":false,"architecture":"arm64","execPath":executable});
        let text = format!("#!/bin/sh\nif [ -f '{0}/slow' ]; then /usr/bin/touch '{0}/entered'; /bin/sleep 30; fi\nif [ -f '{0}/wrong' ]; then printf '%s' '{{}}'; else printf '%s' '{1}'; fi\n", root.display(), output);
        script(&executable, &text);
        for name in ["mise", "brew", "pyenv", "uv"] {
            script(
                &root.join("bin").join(name),
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{0}/manager-calls'\n/usr/bin/touch '{0}/MANAGER_MUST_NOT_RUN'\nexit 91\n",
                    root.display()
                ),
            );
        }
        let query = InventoryQueryService::default();
        query
            .begin("node-baseline".into(), None)
            .unwrap()
            .execute(options.common.clone())
            .unwrap();
        query
            .begin_tool("python-baseline".into(), None, InventoryTool::Python)
            .unwrap()
            .execute_python(options.clone())
            .unwrap();
        let config = root.join("config.toml");
        fs::write(&config, "[env]\nPRIVATE='must-not-persist'\n").unwrap();
        let service = OperationPlanService::new(root.join("journal/operations-v1.json"));
        Self {
            dir,
            options,
            config,
            query,
            service,
            executable,
        }
    }
    fn request(&self, action: Action) -> PlanRequest {
        let (_, snapshot) = self
            .query
            .current_snapshot_for(InventoryTool::Python)
            .unwrap();
        let target = snapshot
            .installations
            .iter()
            .find(|i| i.health == Health::Healthy)
            .unwrap();
        PlanRequest {
            snapshot_request_id: snapshot.request_id.clone(),
            intent: match action {
                Action::Verify => PlanIntent::Verify {
                    installation_id: target.installation_id.clone(),
                },
                Action::Bind => PlanIntent::Bind {
                    installation_id: target.installation_id.clone(),
                    binding: None,
                },
                Action::Uninstall => PlanIntent::Uninstall {
                    installation_id: target.installation_id.clone(),
                },
                _ => unreachable!(),
            },
        }
    }
    fn preview(&self, request: PlanRequest) -> Result<OperationPlan, PlanError> {
        self.service.preview_python(
            &self.query,
            request,
            self.options.clone(),
            self.config.clone(),
        )
    }
    fn plan(&self) -> OperationPlan {
        self.preview(self.request(Action::Verify)).unwrap()
    }
    fn execute(&self, plan: &OperationPlan) -> Result<ExecutionRecord, PlanError> {
        self.service
            .execute_python_with_context(&plan.plan_id, self.query.clone(), || {
                Ok((self.options.clone(), self.config.clone()))
            })
    }
}
fn script(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
#[test]
fn python_verify_uses_own_cache_shared_once_only_executor_and_minimal_journal() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let node = serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap();
    let p = f.plan();
    assert_eq!(p.tool_id, InventoryTool::Python);
    assert!(p.capability.can_execute);
    assert!(!f.dir.path().join("journal").exists());
    let result = f.execute(&p).unwrap();
    assert_eq!(result.tool_id, InventoryTool::Python);
    assert_eq!(result.outcome, Some(Outcome::Verified));
    assert!(result
        .steps
        .iter()
        .all(|s| s.status == StepStatus::Completed));
    assert_eq!(
        node,
        serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap()
    );
    assert!(matches!(f.execute(&p), Err(PlanError::Consumed)));
    assert_eq!(
        operations::persisted(result.clone()).label,
        "Python 实例重新验证"
    );
    let data = fs::read_to_string(f.dir.path().join("journal/operations-v1.json")).unwrap();
    assert!(data.contains("\"toolId\": \"python\"") || data.contains("\"toolId\":\"python\""));
    for forbidden in [
        "must-not-persist",
        f.dir.path().to_str().unwrap(),
        "3.13.15",
        "stdout",
        "argv",
    ] {
        assert!(!data.contains(forbidden));
    }
    let legacy: ExecutionRecord = serde_json::from_value(serde_json::json!({"schemaVersion":1,"id":"legacy","action":"verify","status":"completed","createdAt":0,"updatedAt":0,"steps":[],"outcome":"verified"})).unwrap();
    assert_eq!(legacy.tool_id, InventoryTool::Node);
    assert!(serde_json::to_value(legacy)
        .unwrap()
        .get("toolId")
        .is_none());
    let recovery_path = f.dir.path().join("recovery/operations-v1.json");
    fs::create_dir_all(recovery_path.parent().unwrap()).unwrap();
    let mut pending = result;
    pending.id = "python-recovered".into();
    pending.status = ExecutionStatus::Running;
    pending.steps[0].status = StepStatus::Running;
    fs::write(
        &recovery_path,
        serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"records":[pending]})).unwrap(),
    )
    .unwrap();
    let recovery = OperationPlanService::new(recovery_path);
    recovery.recover().unwrap();
    let recovered = recovery.records().0.remove(0);
    assert_eq!(recovered.status, ExecutionStatus::Interrupted);
    assert_eq!(recovered.tool_id, InventoryTool::Python);
    assert_eq!(
        operations::persisted(recovered).label,
        "Python 实例重新验证"
    );
    assert!(!f.dir.path().join("MANAGER_MUST_NOT_RUN").exists());
}
#[test]
fn four_install_previews_keep_channel_identity_and_can_never_execute() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    for provider in [
        ProviderId::Mise,
        ProviderId::Brew,
        ProviderId::Pyenv,
        ProviderId::Uv,
    ] {
        let request = PlanRequest {
            snapshot_request_id: "python-baseline".into(),
            intent: PlanIntent::Install {
                provider_id: provider,
                package_id: if provider == ProviderId::Brew {
                    "python@3.13"
                } else {
                    "python"
                }
                .into(),
                version: if provider == ProviderId::Brew {
                    None
                } else {
                    Some("3.13.15".into())
                },
            },
        };
        let p = f.preview(request).unwrap();
        assert!(p.capability.can_preview, "{:?}", p.capability);
        assert!(!p.capability.can_execute);
        assert_eq!(p.target.provider_package.provider_id, provider);
        assert_eq!(p.steps.len(), 4);
        assert!(!p.impact.is_empty());
        assert!(matches!(f.execute(&p), Err(PlanError::Disabled)));
        let args = &p.steps[1].argv;
        if provider == ProviderId::Brew {
            assert_eq!(args.last().unwrap(), "homebrew/core/python@3.13");
            assert!(p.version_promise.contains("不承诺精确补丁版"));
        } else {
            assert!(args.last().unwrap().contains("3.13.15"));
        }
    }
    let p = f.preview(f.request(Action::Uninstall)).unwrap();
    assert!(!p.capability.can_execute);
    // 健康目标可以预览卸载步骤；缺少 core 收据时仍在准备阶段关闭执行。
    assert_eq!(p.steps.len(), 4);
    assert!(p.capability.can_preview);
    assert!(p.capability.reason.is_some());
    assert!(matches!(f.execute(&p), Err(PlanError::Disabled)));
    // 绑定必须显式给出作用域与项目路径，缺省请求不再落到 Node 或猜测默认值。
    assert!(f.preview(f.request(Action::Bind)).is_err());
    // 预览只允许读能力证据（--version / --help）；不能执行安装或卸载。
    let calls = fs::read_to_string(f.dir.path().join("manager-calls")).unwrap_or_default();
    assert!(
        calls
            .lines()
            .all(|line| line.contains("--help") || line.contains("--version")),
        "preview must not execute a write command: {calls}"
    );
    assert!(!f.dir.path().join("journal").exists());
}
#[test]
fn precision_and_package_allowlist_reject_ranges_variants_cross_tool_and_injection() {
    let f = Fixture::new();
    for (provider, package, version) in [
        (ProviderId::Mise, "node", Some("3.13.15")),
        (ProviderId::Mise, "python", Some("latest")),
        (ProviderId::Pyenv, "python", Some("3.13")),
        (ProviderId::Uv, "python", Some("3.13.15+freethreaded")),
        (ProviderId::Uv, "python", Some("3.13.15;touch /tmp/x")),
        (ProviderId::Brew, "evil/tap/python@3.13", None),
        (ProviderId::Brew, "python@3.13", Some("3.13.15")),
        (ProviderId::Path, "python", Some("3.13.15")),
        (ProviderId::Nvm, "python", Some("3.13.15")),
    ] {
        assert!(matches!(
            f.preview(PlanRequest {
                snapshot_request_id: "python-baseline".into(),
                intent: PlanIntent::Install {
                    provider_id: provider,
                    package_id: package.into(),
                    version: version.map(str::to_owned)
                }
            }),
            Err(PlanError::Unsupported)
        ));
    }
    let mut req = f.request(Action::Verify);
    req.snapshot_request_id = "node-baseline".into();
    assert!(matches!(f.preview(req), Err(PlanError::Stale)));
    assert!(matches!(
        f.service.preview(
            &f.query,
            f.request(Action::Verify),
            f.options.common.clone(),
            f.config.clone()
        ),
        Err(PlanError::Stale)
    ));
}
#[test]
fn changed_roots_file_config_snapshot_and_expiry_stop_before_probe() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mode in ["root", "file", "config", "snapshot", "expiry"] {
        let f = Fixture::new();
        let p = f.plan();
        let mut options = f.options.clone();
        match mode {
            "root" => options.pyenv_root = f.dir.path().join("elsewhere"),
            "file" => fs::write(&f.executable, "#!/bin/sh\nexit 92\n").unwrap(),
            "config" => fs::write(&f.config, "changed=true").unwrap(),
            "snapshot" => {
                f.query
                    .begin_tool(
                        "new-python".into(),
                        Some(ProviderId::Mise),
                        InventoryTool::Python,
                    )
                    .unwrap()
                    .execute_python(options.clone())
                    .unwrap();
            }
            "expiry" => {
                lock(&f.service.state)
                    .plans
                    .back_mut()
                    .unwrap()
                    .view
                    .expires_at = 0
            }
            _ => unreachable!(),
        }
        let result = f
            .service
            .execute_python_with_context(&p.plan_id, f.query.clone(), || {
                Ok((options.clone(), f.config.clone()))
            });
        assert!(result.is_err(), "{mode}");
        assert!(!f.dir.path().join("journal").exists(), "{mode}");
    }
}
#[test]
fn rescanning_rejects_changed_runtime_output_even_if_file_stamp_is_unchanged() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    fs::write(f.dir.path().join("wrong"), "").unwrap();
    assert!(f.execute(&p).is_err());
    let (records, _) = f.service.records();
    assert_eq!(records[0].outcome, Some(Outcome::PreconditionChanged));
    assert_ne!(records[0].status, ExecutionStatus::Completed);
}
#[test]
fn cancellation_reaps_python_probe_preserves_both_caches_and_returns_shared_slot() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let p = f.plan();
    let node = serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap();
    let python =
        serde_json::to_value(f.query.current_snapshot_for(InventoryTool::Python).unwrap()).unwrap();
    fs::write(f.dir.path().join("slow"), "").unwrap();
    let start = Instant::now();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| f.execute(&p));
        while !f.dir.path().join("entered").exists() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(f.dir.path().join("entered").exists());
        operations::cancel(&p.plan_id).unwrap();
        assert!(worker.join().unwrap().is_err());
    });
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(f.service.records().0[0].status, ExecutionStatus::Cancelled);
    assert_eq!(
        node,
        serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap()
    );
    assert_eq!(
        python,
        serde_json::to_value(f.query.current_snapshot_for(InventoryTool::Python).unwrap()).unwrap()
    );
    assert!(f.query.begin("slot-released".into(), None).is_ok());
}
#[test]
fn unknown_path_and_unsupported_platform_never_gain_verify_permission() {
    let f = Fixture::new();
    let target = f.dir.path().join("bin/python3");
    script(&target, "#!/bin/sh\nexit 97\n");
    let snapshot = f
        .query
        .begin_tool(
            "with-path".into(),
            Some(ProviderId::Path),
            InventoryTool::Python,
        )
        .unwrap()
        .execute_python(f.options.clone())
        .unwrap();
    let item = snapshot
        .installations
        .iter()
        .find(|i| i.provider_package.provider_id == ProviderId::Path)
        .unwrap();
    let p = f
        .preview(PlanRequest {
            snapshot_request_id: snapshot.request_id,
            intent: PlanIntent::Verify {
                installation_id: item.installation_id.clone(),
            },
        })
        .unwrap();
    assert!(!p.capability.can_execute);
    assert!(p.steps.is_empty());
    let mut options = f.options.clone();
    options.common.platform = Platform::Linux;
    let p = f
        .service
        .preview_python(
            &f.query,
            f.request(Action::Verify),
            options,
            f.config.clone(),
        )
        .unwrap();
    assert!(!p.capability.can_preview);
    assert!(p.steps.is_empty());
    let request = f.request(Action::Verify);
    let lease = f.query.begin("busy".into(), None).unwrap();
    assert!(matches!(f.preview(request), Err(PlanError::Inventory)));
    drop(lease);
}
#[test]
fn healthy_target_on_partial_source_can_be_verified_without_install_permission() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let alias = f
        .options
        .common
        .mise_data_dir
        .join("installs/python/latest/bin");
    fs::create_dir_all(&alias).unwrap();
    script(&alias.join("python3"), "#!/bin/sh\nexit 99\n");
    f.query
        .begin_tool(
            "python-partial".into(),
            Some(ProviderId::Mise),
            InventoryTool::Python,
        )
        .unwrap()
        .execute_python(f.options.clone())
        .unwrap();
    let p = f.plan();
    assert!(p.capability.can_execute);
    assert_eq!(f.execute(&p).unwrap().outcome, Some(Outcome::Verified));
    let (_, snapshot) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    assert!(
        !snapshot
            .providers
            .iter()
            .find(|p| p.provider_id == ProviderId::Mise)
            .unwrap()
            .coverage
            .complete
    );
    let p = f
        .preview(PlanRequest {
            snapshot_request_id: snapshot.request_id,
            intent: PlanIntent::Install {
                provider_id: ProviderId::Mise,
                package_id: "python".into(),
                version: Some("3.13.15".into()),
            },
        })
        .unwrap();
    assert!(!p.capability.can_preview);
    assert!(!p.capability.can_execute);
}
#[test]
fn shared_python_contract_keeps_tool_scope_and_nonexecutable_channel_previews() {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/operations/python-v1.json")).unwrap();
    let request: PlanRequest = serde_json::from_value(value["scope"]["request"].clone()).unwrap();
    assert_eq!(value["scope"]["toolId"], "python");
    assert!(matches!(request.intent, PlanIntent::Verify { .. }));
    let p: OperationPlan = serde_json::from_value(value["verifyPlan"].clone()).unwrap();
    assert_eq!(p.tool_id, InventoryTool::Python);
    assert_eq!(serde_json::to_value(&p).unwrap(), value["verifyPlan"]);
    let options = PythonScanOptions {
        common: NodeScanOptions {
            platform: Platform::Macos,
            home: PathBuf::from("/fixture"),
            mise_data_dir: PathBuf::from("/fixture/.local/share/mise"),
            nvm_dir: PathBuf::from("/fixture/.nvm"),
            brew_cellars: vec![PathBuf::from("/fixture/Cellar")],
            path_entries: vec![PathBuf::from("/fixture/bin")],
            limits: Default::default(),
        },
        pyenv_root: PathBuf::from("/fixture/.pyenv"),
        uv_python_dir: PathBuf::from("/fixture/.local/share/uv/python"),
    };
    for value in value["installPlans"].as_array().unwrap() {
        let p: OperationPlan = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(p.tool_id, InventoryTool::Python);
        assert!(!p.capability.can_execute);
        let version = if p.target.provider_package.provider_id == ProviderId::Brew {
            None
        } else {
            Some("3.13.15")
        };
        let (_, argv, _, _) = python::install(
            p.target.provider_package.provider_id,
            &p.target.provider_package.package_id,
            version,
            &options,
        )
        .unwrap();
        assert_eq!(argv, p.steps[1].argv);
    }
    let record: ExecutionRecord = serde_json::from_value(value["record"].clone()).unwrap();
    assert_eq!(record.tool_id, InventoryTool::Python);
    assert_eq!(serde_json::to_value(record).unwrap(), value["record"]);
}
#[path = "python_mise_tests.rs"]
mod mise;
#[cfg(target_os = "macos")]
#[path = "python_uv_tests.rs"]
mod uv;

#[test]
fn v2_python_preview_reuses_execution_journal_and_leaves_node_cache_unchanged() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let node_before = serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap();
    let request = PlanRequestV2 {
        schema_version: 2,
        subject: OperationSubjectV2::Tool {
            tool_id: ToolId::from("python"),
        },
        request: f.request(Action::Verify),
    };
    let wrapped = f
        .service
        .preview_v2_using(request, |tool, request| {
            assert_eq!(tool, InventoryTool::Python);
            f.preview(request)
        })
        .unwrap();
    assert_eq!(lock(&f.service.state).plans.len(), 1);
    assert!(!f.dir.path().join("journal").exists());
    assert_eq!(wrapped.plan.tool_id, InventoryTool::Python);
    let record = f.execute(&wrapped.plan).unwrap();
    assert_eq!(record.status, ExecutionStatus::Completed);
    assert_eq!(record.outcome, Some(Outcome::Verified));
    assert_eq!(record.id, wrapped.plan.plan_id);
    assert_eq!(record.steps.len(), wrapped.step_graph.len());
    assert!(matches!(f.execute(&wrapped.plan), Err(PlanError::Consumed)));
    assert_eq!(
        serde_json::to_value(f.query.current_snapshot().unwrap()).unwrap(),
        node_before
    );
    assert!(!f.dir.path().join("MANAGER_MUST_NOT_RUN").exists());
    let (records, failed) = f.service.records();
    assert!(!failed);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, wrapped.plan.plan_id);
}

#[test]
fn python_bind_plan_writes_project_config_and_verifies_instance() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let project = tempfile::tempdir().unwrap();
    let config = project.path().join("mise.toml");
    fs::write(&config, "[tools]\nnode = \"22\"\n").unwrap();
    let (_, snapshot) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    let target = snapshot
        .installations
        .iter()
        .find(|item| {
            item.provider_package.provider_id == ProviderId::Mise && item.health == Health::Healthy
        })
        .unwrap()
        .clone();
    let request = PlanRequest {
        snapshot_request_id: snapshot.request_id,
        intent: PlanIntent::Bind {
            installation_id: target.installation_id.clone(),
            binding: Some(BindingOptions {
                strategy: BindingStrategy::Direct,
                scope: BindingScope::Project,
                project_path: Some(project.path().to_string_lossy().into()),
            }),
        },
    };
    let plan = f.preview(request).unwrap();
    assert_eq!(plan.action, Action::Bind);
    assert!(plan.capability.can_execute);
    assert_eq!(plan.steps.len(), 4);
    assert_eq!(plan.configuration_diff.len(), 1);
    assert_eq!(plan.configuration_diff[0].field, "tools.python");
    assert_eq!(plan.configuration_diff[0].before, None);
    assert_eq!(
        plan.configuration_diff[0].after.as_deref(),
        Some(target.version.as_deref().unwrap())
    );
    let record = f.execute(&plan).unwrap();
    assert_eq!(record.status, ExecutionStatus::Completed);
    let text = fs::read_to_string(&config).unwrap();
    assert!(text.contains("python = \"3.13.15\""));
    assert!(text.contains("node = \"22\""));
    assert!(matches!(f.execute(&plan), Err(PlanError::Consumed)));
}

#[test]
fn python_bind_preview_rejects_stale_target_and_unknown_scope() {
    let _guard = operations::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let project = tempfile::tempdir().unwrap();
    let (_, snapshot) = f.query.current_snapshot_for(InventoryTool::Python).unwrap();
    let target = snapshot
        .installations
        .iter()
        .find(|item| item.provider_package.provider_id == ProviderId::Mise)
        .unwrap()
        .clone();
    for binding in [
        BindingOptions {
            strategy: BindingStrategy::Managed,
            scope: BindingScope::Project,
            project_path: Some(project.path().to_string_lossy().into()),
        },
        BindingOptions {
            strategy: BindingStrategy::Direct,
            scope: BindingScope::Project,
            project_path: Some("relative".into()),
        },
    ] {
        let request = PlanRequest {
            snapshot_request_id: snapshot.request_id.clone(),
            intent: PlanIntent::Bind {
                installation_id: target.installation_id.clone(),
                binding: Some(binding),
            },
        };
        assert!(f.preview(request).is_err());
    }
}
