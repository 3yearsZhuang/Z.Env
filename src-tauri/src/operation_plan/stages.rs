//! 已批准任务内部的阶段核心；不获取写权限、消费计划或创建持久记录。
use super::{CompensationOutcome, PlanError, StepKind, StepStatus};
use crate::operations::WriteScope;

pub(super) enum Progress {
    Step(usize, StepStatus),
    CompensationStarted,
    CompensationFinished(StepStatus, CompensationOutcome),
}

pub(super) struct ResultState {
    pub changed: bool,
    pub failure: Option<PlanError>,
}

/// scope 必须借自仍持有 Guard 的父任务。记录投影和批准重验由调用方提供，
/// 驱动持有自己的 query lease/事务；本核心不能重入公开执行器或结束父任务。
pub(super) fn run(
    scope: &WriteScope<'_>,
    steps: &[StepKind],
    compensate: bool,
    mut before_command: impl FnMut() -> Result<(), PlanError>,
    mut driver: impl FnMut(StepKind, &WriteScope<'_>) -> Result<(), PlanError>,
    mut record: impl FnMut(Progress) -> Result<(), PlanError>,
) -> ResultState {
    let mut changed = false;
    let mut refreshed = false;
    let mut failure = None;
    for (index, kind) in steps.iter().copied().enumerate() {
        if scope.cancelled() {
            failure = Some(PlanError::Cancelled);
            break;
        }
        if let Err(error) = record(Progress::Step(index, StepStatus::Running)) {
            failure = Some(error);
            break;
        }
        let mut entered = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if kind == StepKind::Command {
                before_command()?;
                entered = true;
            }
            if kind == StepKind::Refresh {
                refreshed = true;
            }
            driver(kind, scope)
        }))
        .unwrap_or(Err(PlanError::Interrupted));
        // 一旦进入变更驱动，即使它失败或恐慌，也必须核对实际残留。
        changed |= entered;
        let status = if result.is_ok() {
            StepStatus::Completed
        } else {
            StepStatus::Failed
        };
        if let Err(error) = record(Progress::Step(index, status)) {
            failure = Some(error);
            break;
        }
        if let Err(error) = result {
            failure = Some(error);
            break;
        }
        scope.log(match kind {
            StepKind::Preflight => "前置条件核对完成",
            StepKind::Command => "变更步骤完成",
            StepKind::Refresh => "来源重扫完成",
            StepKind::Verify => "结果验证完成",
            StepKind::Compensate => "补偿完成",
        });
    }
    if failure.is_some() && changed && compensate {
        let _ = record(Progress::CompensationStarted);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            driver(StepKind::Compensate, scope)
        }))
        .unwrap_or(Err(PlanError::Interrupted));
        let outcome = match &result {
            Ok(()) => CompensationOutcome::Completed,
            Err(PlanError::BindingConflict) => CompensationOutcome::Conflict,
            Err(_) => CompensationOutcome::Failed,
        };
        let _ = record(Progress::CompensationFinished(
            if result.is_ok() {
                StepStatus::Completed
            } else {
                StepStatus::Failed
            },
            outcome,
        ));
        if let Err(error) = result {
            // 清理 I/O 错误与外部改写冲突保持各自原因。
            failure = Some(error);
        }
        refreshed = false;
    }
    // 清理不能因取消、命令失败、恐慌或记录故障被跳过。
    if changed && !refreshed {
        if let Some(index) = steps.iter().position(|kind| *kind == StepKind::Refresh) {
            let _ = record(Progress::Step(index, StepStatus::Running));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                driver(StepKind::Refresh, scope)
            }))
            .unwrap_or(Err(PlanError::Interrupted));
            let _ = record(Progress::Step(
                index,
                if result.is_ok() {
                    StepStatus::Completed
                } else {
                    StepStatus::Failed
                },
            ));
            if result.is_err()
                && !matches!(failure, Some(PlanError::Journal | PlanError::Interrupted))
                && !matches!(failure, Some(PlanError::BindingConflict))
            {
                failure = Some(PlanError::ScanIncomplete);
            }
        }
    }
    ResultState { changed, failure }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::ProviderId,
        error::AppError,
        inventory::query::{InventoryQueryService, InventoryTool},
        operations,
    };

    #[test]
    fn two_stage_groups_share_parent_exclusion_and_cancelled_cleanup() {
        let _lock = operations::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let query = InventoryQueryService::default();
        let steps = [
            StepKind::Preflight,
            StepKind::Command,
            StepKind::Refresh,
            StepKind::Verify,
        ];
        let mut calls = Vec::new();
        let mut query_ids = std::collections::BTreeSet::new();
        let result: Result<(), AppError> =
            operations::run_scoped(Some("stage-groups-parent".into()), "阶段测试", |scope| {
                for item in 0..2 {
                    let state = run(
                        scope,
                        &steps,
                        true,
                        || Ok(()),
                        |kind, stage_scope| {
                            assert_eq!(stage_scope.id(), scope.id());
                            assert_eq!(operations::current_id().as_deref(), Some(scope.id()));
                            assert!(operations::run(None, "阶段内重入", |_| Ok(())).is_err());
                            calls.push((item, kind));
                            let phase = match kind {
                                StepKind::Command => Some("command"),
                                StepKind::Compensate => Some("compensate"),
                                StepKind::Refresh => Some("refresh"),
                                _ => None,
                            };
                            if let Some(phase) = phase {
                                let id = stage_scope.query_id(item, phase).unwrap();
                                assert!(query_ids.insert(id.clone()));
                                // 两工具共用查询槽；父任务没有持有阻止子阶段的 query lease。
                                let lease = query
                                    .begin_tool(
                                        id,
                                        Some(ProviderId::Mise),
                                        if item == 0 {
                                            InventoryTool::Node
                                        } else {
                                            InventoryTool::Python
                                        },
                                    )
                                    .unwrap();
                                drop(lease);
                            }
                            if item == 1 && kind == StepKind::Command {
                                operations::cancel(stage_scope.id()).unwrap();
                            }
                            if item == 1 && matches!(kind, StepKind::Compensate | StepKind::Refresh)
                            {
                                assert!(stage_scope.cancelled());
                                operations::without_cancellation(|| {
                                    assert_eq!(operations::current_id(), None);
                                    assert!(stage_scope.cancelled());
                                    stage_scope.log("取消后的必要清理");
                                });
                            }
                            Ok(())
                        },
                        |_| Ok(()),
                    );
                    assert!(state.changed);
                    // 两组阶段之间仍由同一个父 Guard 排斥新入口。
                    assert!(operations::run(None, "阶段间重入", |_| Ok(())).is_err());
                    if item == 0 {
                        assert!(state.failure.is_none());
                    } else {
                        assert!(matches!(state.failure, Some(PlanError::Cancelled)));
                        return Err(AppError::Other("父任务取消".into()));
                    }
                }
                unreachable!()
            });
        assert!(result.is_err());
        assert_eq!(
            calls,
            [
                (0, StepKind::Preflight),
                (0, StepKind::Command),
                (0, StepKind::Refresh),
                (0, StepKind::Verify),
                (1, StepKind::Preflight),
                (1, StepKind::Command),
                (1, StepKind::Compensate),
                (1, StepKind::Refresh),
            ]
        );
        assert_eq!(query_ids.len(), 5);
        let parent = operations::list()
            .into_iter()
            .find(|job| job.id == "stage-groups-parent")
            .unwrap();
        assert_eq!(parent.status, "cancelled");
        assert_eq!(
            parent
                .lines
                .iter()
                .filter(|line| *line == "取消后的必要清理")
                .count(),
            2
        );
        assert_eq!(operations::current_id(), None);
        operations::run(Some("after-stage-groups".into()), "后续任务", |_| Ok(())).unwrap();
    }
}
