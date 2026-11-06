// 应用级变更任务：所有写环境的入口共用互斥，不依赖页面是否挂载。
use crate::error::AppError;
use serde::Serialize;
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

thread_local! { static CURRENT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) }; }
pub fn current_id() -> Option<String> {
    CURRENT.with(|id| id.borrow().clone())
}
struct Scope(Option<String>);
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|id| *id.borrow_mut() = self.0.take());
    }
}
pub fn without_cancellation<T>(work: impl FnOnce() -> T) -> T {
    let previous = CURRENT.with(|id| id.borrow_mut().take());
    let _scope = Scope(previous);
    work()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub label: String,
    pub status: String,
    pub lines: VecDeque<String>,
    pub cancel_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<crate::operation_plan::ExecutionStep>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compensation: Option<crate::operation_plan::CompensationOutcome>,
    #[serde(skip)]
    partial_failure: bool,
    #[serde(skip)]
    interrupted_failure: bool,
}

pub fn persisted(record: crate::operation_plan::ExecutionRecord) -> Operation {
    Operation {
        id: record.id,
        label: record.action.label_for(record.tool_id).into(),
        status: record.status.as_str().into(),
        lines: ["本地步骤记录；重启时未完成的任务不会自动重放".to_string()].into(),
        cancel_requested: false,
        steps: Some(record.steps),
        compensation: record.compensation,
        partial_failure: false,
        interrupted_failure: false,
    }
}

pub fn interrupted(id: &str) {
    if let Ok(mut jobs) = store().lock() {
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            job.interrupted_failure = true;
        }
    }
}

fn store() -> &'static Mutex<VecDeque<Operation>> {
    static STORE: OnceLock<Mutex<VecDeque<Operation>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub fn list() -> Vec<Operation> {
    store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

pub fn cancel(id: &str) -> Result<(), AppError> {
    let mut jobs = store().lock().map_err(|e| AppError::Other(e.to_string()))?;
    let job = jobs
        .iter_mut()
        .find(|j| j.id == id && j.status == "running")
        .ok_or_else(|| AppError::Other("任务已结束".into()))?;
    job.cancel_requested = true;
    Ok(())
}

pub fn cancelled(id: &str) -> bool {
    store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|j| j.id == id && j.cancel_requested)
}

pub fn log(id: &str, line: impl Into<String>) {
    if let Ok(mut jobs) = store().lock() {
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            let line: String = line.into();
            job.lines.push_back(line.chars().take(4096).collect());
            while job.lines.len() > 500 {
                job.lines.pop_front();
            }
        }
    }
}

pub fn partial(id: &str) {
    if let Ok(mut jobs) = store().lock() {
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            job.partial_failure = true;
        }
    }
    log(id, "部分步骤未完成，请检查操作详情");
}

pub struct Guard {
    id: String,
    finished: bool,
}

/// 内部阶段借用父 Guard；不能构造、结束或拥有一个新的应用级写任务。
/// 序号使用 Cell，使作用域引用不能跨线程共享；取消仍通过父任务 ID 接收。
pub(crate) struct WriteScope<'guard> {
    guard: &'guard Guard,
    nonce: u64,
    query_sequence: Cell<u32>,
}

impl WriteScope<'_> {
    pub(crate) fn id(&self) -> &str {
        &self.guard.id
    }

    pub(crate) fn cancelled(&self) -> bool {
        cancelled(self.id())
    }

    pub(crate) fn log(&self, line: impl Into<String>) {
        log(self.id(), line);
    }

    /// 固定长度父 nonce 避免原任务 ID 占用查询预算；所有阶段共享有界单调序号。
    pub(crate) fn query_id(&self, item: u8, phase: &'static str) -> Result<String, AppError> {
        if item > 1 || !matches!(phase, "command" | "refresh" | "compensate") {
            return Err(AppError::Other("内部阶段查询范围无效".into()));
        }
        let sequence = self
            .query_sequence
            .get()
            .checked_add(1)
            .filter(|sequence| *sequence <= 256)
            .ok_or_else(|| AppError::Other("内部阶段查询数量超出预算".into()))?;
        let id = format!("stage-{:016x}-i{item}-{phase}-s{sequence}", self.nonce);
        crate::inventory::query::validate_request_id(&id)
            .map_err(|_| AppError::Other("内部阶段查询身份无效".into()))?;
        self.query_sequence.set(sequence);
        Ok(id)
    }
}

fn next_scope_nonce() -> Result<u64, AppError> {
    static NEXT_NONCE: AtomicU64 = AtomicU64::new(1);
    let mut nonce = NEXT_NONCE.load(Ordering::Relaxed);
    loop {
        let next = nonce
            .checked_add(1)
            .ok_or_else(|| AppError::Other("内部写任务序号已耗尽".into()))?;
        match NEXT_NONCE.compare_exchange_weak(nonce, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Ok(nonce),
            Err(current) => nonce = current,
        }
    }
}

impl Guard {
    fn finish(&mut self, status: &str) {
        if let Ok(mut jobs) = store().lock() {
            if let Some(job) = jobs.iter_mut().find(|j| j.id == self.id) {
                job.status = status.into();
            }
        }
        self.finished = true;
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("failed");
        }
    }
}

fn begin(id: &str, label: &str) -> Result<Guard, AppError> {
    let mut jobs = store().lock().map_err(|e| AppError::Other(e.to_string()))?;
    if let Some(job) = jobs.iter().find(|j| j.status == "running") {
        return Err(AppError::Other(format!(
            "{}正在执行，请等待完成或在任务面板取消",
            job.label
        )));
    }
    if jobs.iter().any(|j| j.id == id) {
        return Err(AppError::Other("任务 ID 已使用，请重试".into()));
    }
    jobs.push_front(Operation {
        id: id.into(),
        label: label.into(),
        status: "running".into(),
        lines: VecDeque::new(),
        cancel_requested: false,
        steps: None,
        compensation: None,
        partial_failure: false,
        interrupted_failure: false,
    });
    jobs.truncate(20);
    Ok(Guard {
        id: id.into(),
        finished: false,
    })
}

pub fn run<T>(
    id: Option<String>,
    label: &str,
    work: impl FnOnce(&str) -> Result<T, AppError>,
) -> Result<T, AppError> {
    run_scoped(id, label, |scope| work(scope.id()))
}

pub(crate) fn run_scoped<T>(
    id: Option<String>,
    label: &str,
    work: impl FnOnce(&WriteScope<'_>) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let id = id.unwrap_or_else(|| {
        format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    });
    let nonce = next_scope_nonce()?;
    let mut guard = begin(&id, label)?;
    let previous = CURRENT.with(|current| current.replace(Some(id.clone())));
    let _scope = Scope(previous);
    let result = {
        let scope = WriteScope {
            guard: &guard,
            nonce,
            query_sequence: Cell::new(0),
        };
        work(&scope)
    };
    let partial_failure = list().iter().any(|job| job.id == id && job.partial_failure);
    let interrupted_failure = list()
        .iter()
        .any(|job| job.id == id && job.interrupted_failure);
    let status = if interrupted_failure {
        "interrupted"
    } else if (result.is_err() || partial_failure) && cancelled(&id) {
        "cancelled"
    } else if result.is_err() {
        "failed"
    } else if partial_failure {
        "partial"
    } else {
        "completed"
    };
    // 错误可能包含配置秘密，完整详情只通过命令响应返回，不持久保存。
    log(
        &id,
        match status {
            "completed" => "任务完成",
            "cancelled" => "任务已取消",
            "partial" => "任务部分失败，已完成的步骤保留",
            "interrupted" => "任务中断，未自动重放；请检查操作步骤",
            _ => "任务失败，请查看操作页面的错误详情",
        },
    );
    guard.finish(status);
    result
}

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn status(id: &str) -> String {
        list().into_iter().find(|job| job.id == id).unwrap().status
    }

    #[test]
    fn concurrent_entry_rejected_and_guard_released_after_failure() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result: Result<(), AppError> = run(Some("first-test".into()), "测试安装", |id| {
            assert!(run(Some("nested-test".into()), "另一路安装", |_| Ok(())).is_err());
            cancel(id).unwrap();
            assert!(cancelled(id));
            for _ in 0..600 {
                log(id, "bounded");
            }
            assert_eq!(list()[0].lines.len(), 500);
            Err(AppError::Other("测试错误".into()))
        });
        assert!(result.is_err());
        assert_eq!(list()[0].status, "cancelled");
        run(Some("next-test".into()), "后续任务", |_| Ok(())).unwrap();
        run(Some("partial-test".into()), "部分失败", |id| {
            partial(id);
            assert!(run(Some("partial-nested".into()), "并发", |_| Ok(())).is_err());
            Ok(())
        })
        .unwrap();
        assert_eq!(list()[0].status, "partial");
        #[cfg(unix)]
        {
            let result = run(Some("cancel-process-test".into()), "取消进程", |id| {
                let id = id.to_string();
                let worker = std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    cancel(&id).unwrap();
                });
                let started = std::time::Instant::now();
                let result = crate::process::output(
                    std::process::Command::new("sh").args(["-c", "sleep 20 & wait"]),
                    std::time::Duration::from_secs(30),
                );
                worker.join().unwrap();
                assert!(started.elapsed() < std::time::Duration::from_secs(3));
                result
            });
            assert!(result.is_err());
            assert_eq!(list()[0].status, "cancelled");
        }
    }

    #[test]
    fn scoped_stages_share_parent_identity_and_never_reenter_the_write_guard() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let parent = "scoped-parent-identity-test";
        let previous = current_id();
        run_scoped(Some(parent.into()), "父任务", |scope| {
            for stage in ["command", "refresh", "compensate"] {
                assert_eq!(scope.id(), parent);
                assert_eq!(current_id().as_deref(), Some(parent));
                assert_eq!(status(parent), "running");
                scope.log(format!("内部阶段 {stage}"));
                assert!(run(
                    Some(format!("scoped-nested-{stage}")),
                    "嵌套任务",
                    |_| -> Result<(), AppError> {
                        panic!("nested public run must not enter its closure");
                    }
                )
                .is_err());
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(status(parent), "completed");
        assert_eq!(current_id(), previous);
        let job = list().into_iter().find(|job| job.id == parent).unwrap();
        assert!(job.lines.iter().any(|line| line == "内部阶段 command"));
        assert!(job.lines.iter().any(|line| line == "内部阶段 compensate"));
    }

    #[test]
    fn parent_write_scope_excludes_an_independent_thread_until_it_finishes() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let parent = "scoped-cross-thread-parent-test";
        run_scoped(Some(parent.into()), "跨线程父任务", |scope| {
            let contender = std::thread::spawn(|| {
                assert!(current_id().is_none());
                let mut entered = false;
                let result = run(
                    Some("scoped-cross-thread-contender-test".into()),
                    "另一路写入",
                    |_| {
                        entered = true;
                        Ok(())
                    },
                );
                (result.is_err(), entered)
            });
            assert_eq!(contender.join().unwrap(), (true, false));
            assert_eq!(scope.id(), parent);
            assert_eq!(current_id().as_deref(), Some(parent));
            assert_eq!(status(parent), "running");
            Ok(())
        })
        .unwrap();
        run(
            Some("scoped-cross-thread-after-test".into()),
            "后续写入",
            |_| Ok(()),
        )
        .unwrap();
    }

    #[test]
    fn scoped_cancellation_stays_on_the_parent_and_releases_after_cleanup() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let parent = "scoped-cancel-parent-test";
        let previous = current_id();
        let result: Result<(), AppError> =
            run_scoped(Some(parent.into()), "可取消父任务", |scope| {
                assert!(!scope.cancelled());
                let id = scope.id().to_owned();
                std::thread::spawn(move || cancel(&id).unwrap())
                    .join()
                    .unwrap();
                assert!(scope.cancelled());
                assert_eq!(current_id().as_deref(), Some(parent));
                assert_eq!(status(parent), "running");
                scope.log("必要清理保留父任务归属");
                assert!(run(
                    Some("scoped-cancel-contender-test".into()),
                    "清理期间写入",
                    |_| Ok(())
                )
                .is_err());
                Err(AppError::Other("阶段收到父任务取消".into()))
            });
        assert!(result.is_err());
        assert_eq!(status(parent), "cancelled");
        assert_eq!(current_id(), previous);
        run(
            Some("scoped-cancel-after-test".into()),
            "取消后写入",
            |_| Ok(()),
        )
        .unwrap();
    }

    #[test]
    fn scoped_panic_restores_previous_context_and_releases_the_parent_guard() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let original =
            CURRENT.with(|current| current.replace(Some("previous-context-test".into())));
        let _restore = Scope(original);
        let parent = "scoped-panic-parent-test";
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_scoped::<()>(Some(parent.into()), "恐慌父任务", |scope| {
                assert_eq!(scope.id(), parent);
                assert_eq!(current_id().as_deref(), Some(parent));
                panic!("injected scoped stage panic");
            })
        }));
        assert!(result.is_err());
        assert_eq!(status(parent), "failed");
        assert_eq!(current_id().as_deref(), Some("previous-context-test"));
        run(
            Some("scoped-panic-after-test".into()),
            "恐慌后写入",
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(current_id().as_deref(), Some("previous-context-test"));
    }

    #[test]
    fn phase_query_ids_are_bounded_unique_and_accepted_by_inventory_leases() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let query = crate::inventory::query::InventoryQueryService::default();
        let parent = "long-parent-身份-".repeat(32);
        let ids = run_scoped(Some(parent.clone()), "查询身份父任务", |scope| {
            assert_eq!(scope.id(), parent);
            assert!(scope.query_id(2, "command").is_err());
            assert!(scope.query_id(0, "unknown").is_err());
            assert!(scope.query_id(0, "command;touch x").is_err());
            let mut ids = Vec::new();
            let mut unique = std::collections::BTreeSet::new();
            for index in 0..256 {
                let item = (index % 2) as u8;
                let phase = ["command", "refresh", "compensate"][index % 3];
                let id = scope.query_id(item, phase)?;
                assert!(id.len() <= 128);
                crate::inventory::query::validate_request_id(&id).unwrap();
                assert!(unique.insert(id.clone()));
                let tool = if item == 0 {
                    crate::inventory::query::InventoryTool::Node
                } else {
                    crate::inventory::query::InventoryTool::Python
                };
                let lease = query
                    .begin_tool(id.clone(), Some(crate::domain::ProviderId::Mise), tool)
                    .unwrap();
                drop(lease);
                ids.push(id);
            }
            assert!(scope.query_id(0, "command").is_err());
            assert!(scope.query_id(1, "refresh").is_err());
            assert!(matches!(
                query.begin(ids[0].clone(), Some(crate::domain::ProviderId::Mise)),
                Err(crate::inventory::query::QueryError::ReusedRequestId)
            ));
            Ok(ids)
        })
        .unwrap();
        let next = run_scoped(
            Some("query-id-next-parent-test".into()),
            "另一个父任务",
            |scope| scope.query_id(0, "command"),
        )
        .unwrap();
        assert!(!ids.contains(&next));
        drop(
            query
                .begin(next, Some(crate::domain::ProviderId::Mise))
                .unwrap(),
        );
    }
}
