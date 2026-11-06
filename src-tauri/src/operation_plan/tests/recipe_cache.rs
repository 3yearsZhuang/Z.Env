#![cfg(unix)]

use super::*;
use crate::{
    preset::PresetTool,
    providers::python::PythonScanOptions,
    recipe::{self, RecipeDisposition},
};
use std::collections::BTreeMap;

fn recipe_options(f: &Fixture) -> PythonScanOptions {
    PythonScanOptions {
        common: f.options.clone(),
        pyenv_root: f._dir.path().join("pyenv"),
        uv_python_dir: f._dir.path().join("uv/python"),
    }
}

fn cache_state(service: &OperationPlanService) -> (u64, Vec<String>) {
    let state = lock(&service.state);
    (
        state.sequence,
        state
            .plans
            .iter()
            .map(|plan| plan.view.plan_id.clone())
            .collect(),
    )
}

/// 同时保全目录集合与文件内容，确认预览未创建日志、缓存或安装产物。
fn fixture_files(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn read(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if kind.is_dir() {
                files.insert(relative, None);
                read(root, &path, files);
            } else {
                assert!(kind.is_file(), "fixture 不应包含外部链接或特殊文件");
                files.insert(relative, Some(std::fs::read(path).unwrap()));
            }
        }
    }
    let mut files = BTreeMap::new();
    read(root, root, &mut files);
    files
}

fn preview_install_recipe(f: &Fixture) -> recipe::RecipeReport {
    // fixture 已有 nvm 22.1.0；使用缺少的精确版本，确保真正进入安装计划预览。
    recipe::preview_recipe(
        &[PresetTool {
            name: "node".into(),
            version: "22.1.1".into(),
        }],
        None,
        &recipe_options(f),
        &f.service,
        &f.query,
        &f.config,
    )
    .unwrap()
}

#[test]
fn recipe_install_preview_preserves_full_live_plan_cache_and_files() {
    let f = Fixture::new();
    channels::setup_mise(&f);
    f.service.recover().unwrap();
    let first = f.plan();
    assert!(first.capability.can_execute);
    for _ in 1..MAX_PLANS {
        f.plan();
    }
    let before_cache = cache_state(&f.service);
    assert_eq!(before_cache.1.len(), MAX_PLANS);
    let before_files = fixture_files(f._dir.path());

    for _ in 0..3 {
        let report = preview_install_recipe(&f);
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].disposition, RecipeDisposition::Install);
        assert!(report.steps[0].plan_ready, "{}", report.steps[0].reason);
        assert!(!report.executable, "只读报告不能开放配方执行");
        assert_eq!(cache_state(&f.service), before_cache);
        assert_eq!(fixture_files(f._dir.path()), before_files);
    }

    // 缓存满额时的额外预览也未淘汰用户已打开的首个真实计划。
    assert!(f.service.cancel_pending(&first.plan_id));
    assert_eq!(cache_state(&f.service), before_cache);
    assert_eq!(fixture_files(f._dir.path()), before_files);
    assert!(!f._dir.path().join("journal").exists());
}

#[test]
fn recipe_isolated_preview_inherits_live_journal_failure() {
    let f = Fixture::new();
    channels::setup_mise(&f);
    f.service.recover().unwrap();
    let healthy = preview_install_recipe(&f);
    assert_eq!(healthy.steps[0].disposition, RecipeDisposition::Install);
    assert!(healthy.steps[0].plan_ready, "{}", healthy.steps[0].reason);
    assert!(cache_state(&f.service).1.is_empty());

    lock(&f.service.state).journal_failed = true;
    let before_cache = cache_state(&f.service);
    let before_files = fixture_files(f._dir.path());
    let report = preview_install_recipe(&f);
    assert_eq!(report.steps.len(), 1);
    assert!(!report.steps[0].plan_ready);
    assert!(!report.executable);
    assert!(
        report.steps[0]
            .reason
            .contains(&PlanError::Journal.to_string()),
        "{}",
        report.steps[0].reason
    );
    assert_eq!(cache_state(&f.service), before_cache);
    assert_eq!(fixture_files(f._dir.path()), before_files);
    assert!(!f._dir.path().join("journal").exists());
}
