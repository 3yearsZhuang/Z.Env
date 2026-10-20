// 应用级变更任务：所有写环境的入口共用互斥，不依赖页面是否挂载。
use crate::error::AppError;
use serde::Serialize;
use std::collections::VecDeque;
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
    let id = id.unwrap_or_else(|| {
        format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    });
    let mut guard = begin(&id, label)?;
    let previous = CURRENT.with(|current| current.replace(Some(id.clone())));
    let _scope = Scope(previous);
    let result = work(&id);
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
}
