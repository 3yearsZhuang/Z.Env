use super::{unix::Fixture, *};
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

/// 复用既有 mise Python 门禁的 argv 和环境约定，禁止安装/卸载命令。
fn mise_gate(fixture: &Fixture) {
    let root = fixture.root.path();
    let existing = fixture
        .options
        .common
        .mise_data_dir
        .join("installs/python/3.12.14");
    fs::write(
        root.join("python-registry.json"),
        serde_json::json!([{
            "version": "3.12.14", "install_path": existing, "installed": true,
        }])
        .to_string(),
    )
    .unwrap();
    fs::write(
        fixture
            .options
            .common
            .mise_data_dir
            .join("installs/python/.mise.backend.toml"),
        "full='core:python'\n",
    )
    .unwrap();
    let script = format!(
        r##"#!/bin/sh
ROOT='{root}'
if [ "$1" = '--help' ]; then
 if [ -f "$ROOT/block-python-gate" ]; then
  printf 1 > "$ROOT/gate-entered.fifo"
  IFS= read -r release < "$ROOT/gate-release.fifo" || exit 94
 fi
 printf '%s' '--no-config --no-env --no-hooks'
 exit 0
fi
[ "$1|$2|$3" = '--no-config|--no-env|--no-hooks' ] || exit 95
shift 3
case "$1" in
settings)
 case "$3" in
 python.compile) if [ -f "$ROOT/deny-compile" ]; then printf true; else printf false; fi ;;
 python.default_packages_file) printf '%s' "$MISE_PYTHON_DEFAULT_PACKAGES_FILE" ;;
 *) exit 96 ;;
 esac ;;
ls) /bin/cat "$ROOT/python-registry.json" ;;
install|uninstall) printf '%s' "$*" > "$ROOT/EXECUTION_MUST_NOT_RUN"; exit 97 ;;
*) exit 98 ;;
esac
"##,
        root = root.display(),
    );
    let program = fixture.options.common.path_entries[0].join("mise");
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
}

fn preview(
    fixture: &Fixture,
    project: Option<&PythonProjectRequest>,
) -> Result<RecipeReport, String> {
    preview_recipe(
        &[requested("python", "3.12.12")],
        project,
        &fixture.options,
        &fixture.service,
        &fixture.query,
        &fixture.root.path().join(".config/mise/config.toml"),
    )
}

fn assert_no_install_artifacts(fixture: &Fixture) {
    assert!(!fixture
        .options
        .common
        .mise_data_dir
        .join("installs/python/3.12.12")
        .exists());
    assert!(!fixture
        .options
        .common
        .mise_data_dir
        .join("zenv-command-config")
        .exists());
    assert!(!fixture.root.path().join("EXECUTION_MUST_NOT_RUN").exists());
    assert!(!fixture
        .root
        .path()
        .join(".config/mise/config.toml")
        .exists());
}

fn assert_bindings(report: &RecipeReport, captured: &CapturedInventories) {
    assert_eq!(report.snapshot_bindings.len(), captured.len());
    for binding in &report.snapshot_bindings {
        let (generation, snapshot) = captured[&binding.tool].as_ref().unwrap();
        assert_eq!(
            binding.request_id.as_deref(),
            Some(snapshot.request_id.as_str())
        );
        assert_eq!(
            binding.generation.as_deref(),
            Some(generation.to_string().as_str())
        );
    }
}

/// FIFO 确认真实 probe 已进入门禁，再由测试释放；timeout 只作失败预算。
struct ProbeBarrier {
    entered: Receiver<Result<(), String>>,
    wake_entered: File,
    release: File,
    listener: Option<JoinHandle<()>>,
    released: bool,
}

impl ProbeBarrier {
    fn new(root: &Path) -> Self {
        let open_fifo = |name: &str| {
            let path = root.join(name);
            let raw = CString::new(path.as_os_str().as_bytes()).unwrap();
            let status = unsafe { libc::mkfifo(raw.as_ptr(), 0o600) };
            assert_eq!(status, 0, "{}", std::io::Error::last_os_error());
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .unwrap()
        };
        let wake_entered = open_fifo("gate-entered.fifo");
        let mut reader = wake_entered.try_clone().unwrap();
        let release = open_fifo("gate-release.fifo");
        fs::write(root.join("block-python-gate"), "").unwrap();
        let (sender, entered) = mpsc::channel();
        let listener = thread::spawn(move || {
            let result = reader
                .read_exact(&mut [0])
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        Self {
            entered,
            wake_entered,
            release,
            listener: Some(listener),
            released: false,
        }
    }

    fn wait(&self) {
        self.entered
            .recv_timeout(Duration::from_secs(5))
            .expect("真实 Python 门禁未进入 FIFO 屏障")
            .expect("读取 probe 屏障失败");
    }

    fn release(&mut self) {
        if !self.released {
            self.release.write_all(b"continue\n").unwrap();
            self.released = true;
        }
    }
}

impl Drop for ProbeBarrier {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.release.write_all(b"continue\n");
        }
        // 失败时也唤醒监听线程和子进程，避免断言留下阻塞线程。
        let _ = self.wake_entered.write_all(b"1");
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

fn during_gate<T>(
    fixture: &Fixture,
    project: Option<&PythonProjectRequest>,
    change: impl FnOnce() -> T,
) -> (Result<RecipeReport, String>, T) {
    thread::scope(|scope| {
        let mut barrier = ProbeBarrier::new(fixture.root.path());
        let worker = scope.spawn(|| preview(fixture, project));
        barrier.wait();
        let changed = change();
        barrier.release();
        (worker.join().unwrap(), changed)
    })
}

#[test]
fn node_declaration_changed_during_python_preparation_invalidates_the_mixed_recipe() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    fixture
        .query
        .begin("node-before-project-change".into(), None)
        .unwrap()
        .execute(fixture.options.common.clone())
        .unwrap();
    fixture.python("python-before-node-project-change");
    let project = fixture.root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let package = project.join("package.json");
    fs::write(&package, r#"{"engines":{"node":">=22 <23"}}"#).unwrap();
    fs::write(
        project.join("pyproject.toml"),
        "[project]\nrequires-python='>=3.12,<3.13'\n",
    )
    .unwrap();
    let request = PythonProjectRequest {
        request_id: "mixed-project-read".into(),
        snapshot_request_id: "python-before-node-project-change".into(),
        path: project.to_string_lossy().into(),
    };
    let result = thread::scope(|scope| {
        let mut barrier = ProbeBarrier::new(fixture.root.path());
        let worker = scope.spawn(|| {
            preview_recipe(
                &[requested("node", "22.23.3"), requested("python", "3.12.12")],
                Some(&request),
                &fixture.options,
                &fixture.service,
                &fixture.query,
                &fixture.root.path().join(".config/mise/config.toml"),
            )
        });
        barrier.wait();
        fs::write(&package, r#"{"engines":{"node":">=24 <25"}}"#).unwrap();
        barrier.release();
        worker.join().unwrap()
    });
    assert!(result.unwrap_err().contains("Node 项目声明"));
    assert_eq!(
        fs::read_to_string(&package).unwrap(),
        r#"{"engines":{"node":">=24 <25"}}"#
    );
    fixture.assert_readonly();
    assert_no_install_artifacts(&fixture);
}

#[test]
fn missing_exact_python_uses_real_mise_gate_and_preserves_each_tool_binding_without_writes() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    let node = fixture
        .query
        .begin("node-install-baseline".into(), None)
        .unwrap()
        .execute(fixture.options.common.clone())
        .unwrap();
    let python = fixture.python("python-install-baseline");
    assert!(complete(&node, InventoryTool::Node));
    assert!(complete(&python, InventoryTool::Python));
    let captured = fixture
        .query
        .capture_snapshots(&[InventoryTool::Node, InventoryTool::Python])
        .unwrap();
    let report = preview_recipe(
        &[requested("node", "22.23.3"), requested("python", "3.12.12")],
        None,
        &fixture.options,
        &fixture.service,
        &fixture.query,
        &fixture.root.path().join(".config/mise/config.toml"),
    )
    .unwrap();
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Reuse);
    assert_eq!(report.steps[1].disposition, RecipeDisposition::Install);
    assert!(report.steps[1].plan_ready);
    assert_eq!(report.steps[1].channel.as_deref(), Some("mise"));
    assert_eq!(report.steps[1].version.as_deref(), Some("3.12.12"));
    assert_eq!(report.steps[1].installation_id, None);
    assert_eq!(report.snapshot_request_id, None);
    assert!(!report.executable);
    assert_bindings(&report, &captured);
    fixture.query.validate_snapshots(&captured).unwrap();
    let python_only = preview(&fixture, None).unwrap();
    assert_eq!(python_only.steps[0].disposition, RecipeDisposition::Install);
    assert!(python_only.steps[0].plan_ready);
    assert_eq!(
        python_only.snapshot_request_id.as_deref(),
        Some("python-install-baseline")
    );
    fixture.assert_readonly();
    assert_no_install_artifacts(&fixture);
    assert!(fixture.service.records().0.is_empty());
}

#[test]
fn denied_python_compile_gate_stays_unknown_and_does_not_write_or_install() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    fixture.python("python-denied-baseline");
    fs::write(fixture.root.path().join("deny-compile"), "").unwrap();
    let report = preview(&fixture, None).unwrap();
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Unknown);
    assert!(!report.steps[0].plan_ready);
    assert!(report.steps[0].reason.contains("Python 源码构建"));
    assert!(!report.executable);
    fixture.assert_readonly();
    assert_no_install_artifacts(&fixture);
}

#[test]
fn custom_non_file_global_config_is_kept_unknown_instead_of_using_the_default_path() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    fixture.python("python-custom-config");
    let custom = fixture.root.path().join("custom-global-config.toml");
    fs::create_dir(&custom).unwrap();
    let report = preview_recipe(
        &[requested("python", "3.12.12")],
        None,
        &fixture.options,
        &fixture.service,
        &fixture.query,
        &custom,
    )
    .unwrap();
    assert_eq!(report.steps[0].disposition, RecipeDisposition::Unknown);
    assert!(!report.steps[0].plan_ready);
    assert!(report.steps[0].reason.contains("安装能力无法确认"));
    assert!(!report.executable);
    assert!(custom.is_dir());
    fixture.assert_readonly();
    assert_no_install_artifacts(&fixture);
}

#[test]
fn refreshing_python_snapshot_during_gate_rejects_the_old_recipe() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    fixture.python("python-before-refresh");
    let (result, refreshed) =
        during_gate(&fixture, None, || fixture.python("python-after-refresh"));
    assert!(result.is_err(), "刷新后的清单不能返回旧配方");
    assert_eq!(
        fixture
            .query
            .current_snapshot_for(InventoryTool::Python)
            .unwrap()
            .1
            .request_id,
        refreshed.request_id
    );
    fixture.assert_readonly();
    assert_no_install_artifacts(&fixture);
}

#[test]
fn active_python_scan_during_gate_rejects_the_old_recipe() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    fixture.python("python-before-active");
    let (result, lease) = during_gate(&fixture, None, || {
        fixture
            .query
            .begin_tool("python-active".into(), None, InventoryTool::Python)
            .unwrap()
    });
    assert!(result.is_err(), "活动扫描期间不能返回旧配方");
    drop(lease);
    fixture.assert_readonly();
    assert_no_install_artifacts(&fixture);
}

#[test]
fn external_project_declaration_change_during_gate_rejects_the_old_recipe() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    fixture.python("python-before-project-change");
    let project = fixture.root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let declaration = project.join(".python-version");
    fs::write(&declaration, "3.12.12\n").unwrap();
    let request = PythonProjectRequest {
        request_id: "project-during-gate".into(),
        snapshot_request_id: "python-before-project-change".into(),
        path: project.to_string_lossy().into(),
    };
    let (result, ()) = during_gate(&fixture, Some(&request), || {
        fs::write(&declaration, "3.13.1\n").unwrap();
    });
    assert!(result.unwrap_err().contains("项目声明已变化"));
    assert_eq!(fs::read_to_string(declaration).unwrap(), "3.13.1\n");
    fixture.assert_readonly();
    assert_no_install_artifacts(&fixture);
}

#[test]
fn live_journal_failure_during_gate_rejects_the_old_install_capability() {
    let fixture = Fixture::new();
    mise_gate(&fixture);
    fixture.python("python-before-journal-failure");
    let captured = fixture
        .query
        .capture_snapshots(&[InventoryTool::Python])
        .unwrap();
    assert!(!fixture.service.records().1);
    let journal = fixture.root.path().join("journal/operations.json");
    let corrupt = b"not a valid journal";
    let (result, ()) = during_gate(&fixture, None, || {
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        fs::write(&journal, corrupt).unwrap();
        assert!(fixture.service.recover().is_err());
        assert!(fixture.service.records().1);
    });
    let error = result.unwrap_err();
    assert!(error.contains("操作记录") || error.contains("计划能力已变化"));
    assert_eq!(fs::read(journal).unwrap(), corrupt);
    assert!(fixture.service.records().1);
    fixture.query.validate_snapshots(&captured).unwrap();
    assert_no_install_artifacts(&fixture);
}
