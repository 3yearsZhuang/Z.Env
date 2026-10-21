//! 清单查询的请求租约与内存缓存；扫描始终在锁外执行。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{
    InventorySnapshot, ProviderError, ProviderId, ProviderReport, ProviderScan, ProviderStatus,
    ScanCoverage, ToolId,
};
use crate::providers::python::{scan_python_provider, scan_python_sources, PythonScanOptions};
use crate::providers::{scan_node_provider, scan_node_sources, CancellationToken, NodeScanOptions};

use super::{merge_inventory, MergeError, MAX_SAFE_TIMESTAMP};

const PROVIDERS: [ProviderId; 4] = [
    ProviderId::Mise,
    ProviderId::Brew,
    ProviderId::Nvm,
    ProviderId::Path,
];
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    serde::Deserialize,
    serde::Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum InventoryTool {
    #[default]
    Node,
    Python,
}
impl InventoryTool {
    pub fn is_node(&self) -> bool {
        *self == Self::Node
    }
    pub fn id(self) -> ToolId {
        ToolId::from(match self {
            Self::Node => "node",
            Self::Python => "python",
        })
    }
    fn providers(self) -> &'static [ProviderId] {
        match self {
            Self::Node => &PROVIDERS,
            Self::Python => &crate::providers::python::PROVIDERS,
        }
    }
}
#[derive(Default)]
struct CachedInventory {
    scans: BTreeMap<ProviderId, ProviderScan>,
    committed: Option<(u64, InventorySnapshot)>,
}

const REQUEST_HISTORY_LIMIT: usize = 256;
const PENDING_CANCELLATION_LIMIT: usize = 128;
// 默认扫描最多返回 128 个候选，缓存另留相同数量的旧证据供失败时展示。
const CACHED_OBSERVATION_LIMIT: usize = 256;

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum QueryError {
    #[error("清单请求 ID 必须是 1 到 128 个英文字母、数字、短横线或下划线")]
    InvalidRequestId,
    #[error("清单请求 ID 已使用，请生成新的请求 ID")]
    ReusedRequestId,
    #[error("清单扫描仍在运行，请等待结束后重试")]
    Busy,
    #[error("工具清单刷新已取消")]
    Cancelled,
    #[error("清单查询已失效")]
    Superseded,
    #[error("清单查询序号已耗尽")]
    GenerationExhausted,
    #[error("请先完成一次清单刷新")]
    NotReady,
    #[error("来源扫描结果无效：{0}")]
    InvalidScan(&'static str),
    #[error(transparent)]
    Merge(#[from] MergeError),
}

struct ActiveQuery {
    request_id: String,
    generation: u64,
    cancellation: CancellationToken,
}

#[derive(Default)]
struct QueryState {
    generation: u64,
    active: Option<ActiveQuery>,
    caches: BTreeMap<InventoryTool, CachedInventory>,
    request_history: VecDeque<String>,
    pending_cancellations: VecDeque<String>,
}

/// 一个服务最多保留一个运行中的扫描，取消后也需等旧工作线程真正退出才释放。
/// 这样文件系统调用阻塞时，连续刷新不会不断创建新的工作线程和探针。
#[derive(Clone, Default)]
pub struct InventoryQueryService {
    state: Arc<Mutex<QueryState>>,
}

fn lock_state(state: &Mutex<QueryState>) -> MutexGuard<'_, QueryState> {
    // 临界区只修改本模块状态；恢复中毒锁可确保异常退出仍能归还扫描槽。
    state.lock().unwrap_or_else(|error| error.into_inner())
}

fn validate_request_id(request_id: &str) -> Result<(), QueryError> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(QueryError::InvalidRequestId);
    }
    Ok(())
}

fn remember_bounded(queue: &mut VecDeque<String>, request_id: String, limit: usize) {
    queue.push_back(request_id);
    while queue.len() > limit {
        queue.pop_front();
    }
}

impl InventoryQueryService {
    /// 只返回最近一次成功提交，活动扫描期间不能签发或执行计划。
    pub(crate) fn current_snapshot(&self) -> Result<(u64, InventorySnapshot), QueryError> {
        self.current_snapshot_for(InventoryTool::Node)
    }
    pub(crate) fn current_snapshot_for(
        &self,
        tool: InventoryTool,
    ) -> Result<(u64, InventorySnapshot), QueryError> {
        let state = lock_state(&self.state);
        if state.active.is_some() {
            return Err(QueryError::Busy);
        }
        state
            .caches
            .get(&tool)
            .and_then(|c| c.committed.clone())
            .ok_or(QueryError::NotReady)
    }

    /// 在分派工作线程之前注册租约，避免取消请求早于扫描注册时丢失。
    pub fn begin(
        &self,
        request_id: String,
        provider_id: Option<ProviderId>,
    ) -> Result<QueryLease, QueryError> {
        self.begin_tool(request_id, provider_id, InventoryTool::Node)
    }
    pub fn begin_tool(
        &self,
        request_id: String,
        provider_id: Option<ProviderId>,
        tool: InventoryTool,
    ) -> Result<QueryLease, QueryError> {
        validate_request_id(&request_id)?;
        if provider_id.is_some_and(|p| !tool.providers().contains(&p)) {
            return Err(QueryError::InvalidScan("来源不属于所请求工具"));
        }
        let mut state = lock_state(&self.state);
        if state.request_history.contains(&request_id) {
            return Err(QueryError::ReusedRequestId);
        }
        if let Some(index) = state
            .pending_cancellations
            .iter()
            .position(|pending| pending == &request_id)
        {
            state.pending_cancellations.remove(index);
            remember_bounded(
                &mut state.request_history,
                request_id,
                REQUEST_HISTORY_LIMIT,
            );
            return Err(QueryError::Cancelled);
        }
        if state.active.is_some() {
            return Err(QueryError::Busy);
        }
        let generation = state
            .generation
            .checked_add(1)
            .ok_or(QueryError::GenerationExhausted)?;
        let cancellation = CancellationToken::default();
        state.generation = generation;
        state.caches.entry(tool).or_default();
        state.active = Some(ActiveQuery {
            request_id: request_id.clone(),
            generation,
            cancellation: cancellation.clone(),
        });
        remember_bounded(
            &mut state.request_history,
            request_id.clone(),
            REQUEST_HISTORY_LIMIT,
        );
        Ok(QueryLease {
            state: Arc::clone(&self.state),
            request_id,
            generation,
            provider_id,
            tool,
            cancellation,
        })
    }

    /// 返回是否接受本次取消；已结束的请求返回 false，未知请求保存有界取消记录。
    pub fn cancel(&self, request_id: &str) -> Result<bool, QueryError> {
        validate_request_id(request_id)?;
        let mut state = lock_state(&self.state);
        if let Some(active) = &state.active {
            if active.request_id == request_id {
                active.cancellation.cancel();
                return Ok(true);
            }
        }
        if state.request_history.iter().any(|id| id == request_id) {
            return Ok(false);
        }
        if !state
            .pending_cancellations
            .iter()
            .any(|id| id == request_id)
        {
            remember_bounded(
                &mut state.pending_cancellations,
                request_id.to_owned(),
                PENDING_CANCELLATION_LIMIT,
            );
        }
        Ok(true)
    }
}

/// 租约不能复制；无论执行成功、错误、恐慌还是尚未分派就丢弃，都归还扫描槽。
pub struct QueryLease {
    state: Arc<Mutex<QueryState>>,
    request_id: String,
    generation: u64,
    provider_id: Option<ProviderId>,
    tool: InventoryTool,
    cancellation: CancellationToken,
}

impl QueryLease {
    /// 项目只读核对共用查询槽，但不重新提交或改写清单缓存。
    pub(crate) fn read_snapshot<R>(
        self,
        expected: &str,
        read: impl FnOnce(&InventorySnapshot, &CancellationToken) -> R,
    ) -> Result<R, QueryError> {
        let snapshot = {
            let state = lock_state(&self.state);
            if !self.owns_active(&state) {
                return Err(QueryError::Superseded);
            }
            state
                .caches
                .get(&self.tool)
                .and_then(|c| c.committed.as_ref())
                .filter(|(_, s)| s.request_id == expected)
                .map(|(_, s)| s.clone())
                .ok_or(QueryError::NotReady)?
        };
        if self.cancellation.is_cancelled() {
            return Err(QueryError::Cancelled);
        }
        let result = read(&snapshot, &self.cancellation);
        if self.cancellation.is_cancelled() {
            return Err(QueryError::Cancelled);
        }
        if !self.owns_active(&lock_state(&self.state)) {
            return Err(QueryError::Superseded);
        }
        Ok(result)
    }
    /// 写入开始前保留旧观测但明确失效；租约同时阻止普通查询提交陈旧扫描。
    pub(crate) fn mark_mutating(&self) -> Result<(), QueryError> {
        let provider = self
            .provider_id
            .ok_or(QueryError::InvalidScan("变更必须限定单来源"))?;
        if self.tool == InventoryTool::Python
            && !matches!(
                provider,
                ProviderId::Mise | ProviderId::Uv | ProviderId::Brew | ProviderId::Pyenv
            )
        {
            return Err(QueryError::InvalidScan(
                "Python 仅开放 mise/uv/brew/pyenv 变更租约",
            ));
        }
        let mut state = lock_state(&self.state);
        if !self.owns_active(&state) {
            return Err(QueryError::Superseded);
        }
        let mut scans = state
            .caches
            .get(&self.tool)
            .map(|c| c.scans.clone())
            .unwrap_or_default();
        let source = scans
            .entry(provider)
            .or_insert_with(|| unscanned_source_for(provider, self.tool));
        source.report.stale = true;
        source.report.status = ProviderStatus::Error;
        source.report.coverage.complete = false;
        source.report.error = Some(ProviderError {
            code: "mutationPending".into(),
            message: "来源正在变更，等待执行后重扫确认".into(),
            retryable: true,
        });
        let snapshot = merge_inventory(
            self.request_id.clone(),
            now_millis(),
            scans.values().cloned().collect(),
        )?;
        let cache = state.caches.entry(self.tool).or_default();
        cache.scans = scans;
        cache.committed = Some((self.generation, snapshot));
        Ok(())
    }
    pub fn execute(self, options: NodeScanOptions) -> Result<InventorySnapshot, QueryError> {
        if self.tool != InventoryTool::Node {
            return Err(QueryError::InvalidScan("扫描器与工具不一致"));
        }
        self.execute_with(|provider, cancellation| match provider {
            Some(provider) => vec![scan_node_provider(provider, &options, cancellation)],
            None => scan_node_sources(&options, cancellation),
        })
    }

    pub fn execute_python(
        self,
        options: PythonScanOptions,
    ) -> Result<InventorySnapshot, QueryError> {
        if self.tool != InventoryTool::Python {
            return Err(QueryError::InvalidScan("扫描器与工具不一致"));
        }
        self.execute_with(|provider, cancellation| match provider {
            Some(provider) => vec![scan_python_provider(provider, &options, cancellation)],
            None => scan_python_sources(&options, cancellation),
        })
    }
    fn owns_active(&self, state: &QueryState) -> bool {
        state.active.as_ref().is_some_and(|active| {
            active.request_id == self.request_id && active.generation == self.generation
        })
    }

    fn execute_with(
        self,
        scan: impl FnOnce(Option<ProviderId>, &CancellationToken) -> Vec<ProviderScan>,
    ) -> Result<InventorySnapshot, QueryError> {
        if self.cancellation.is_cancelled() {
            return Err(QueryError::Cancelled);
        }
        let fresh_scans = scan(self.provider_id, &self.cancellation);
        if self.cancellation.is_cancelled() {
            return Err(QueryError::Cancelled);
        }
        validate_scan_scope(self.provider_id, &fresh_scans, self.tool)?;

        let mut scans = {
            let state = lock_state(&self.state);
            if !self.owns_active(&state) {
                return Err(QueryError::Superseded);
            }
            state
                .caches
                .get(&self.tool)
                .map(|c| c.scans.clone())
                .unwrap_or_default()
        };
        for fresh in fresh_scans {
            let provider = fresh.report.provider_id;
            let refreshed = refresh_source(scans.get(&provider), fresh);
            scans.insert(provider, refreshed);
        }
        let observed_at = now_millis();
        let complete_scans = self
            .tool
            .providers()
            .iter()
            .copied()
            .map(|provider| {
                scans
                    .get(&provider)
                    .cloned()
                    .unwrap_or_else(|| unscanned_source_for(provider, self.tool))
            })
            .collect();
        let snapshot = merge_inventory(self.request_id.clone(), observed_at, complete_scans)?;

        let mut state = lock_state(&self.state);
        if !self.owns_active(&state) {
            return Err(QueryError::Superseded);
        }
        // 取消和提交共用状态锁；旧结果不能在取消成功之后写回缓存。
        if self.cancellation.is_cancelled() {
            return Err(QueryError::Cancelled);
        }
        let cache = state.caches.entry(self.tool).or_default();
        cache.scans = scans;
        cache.committed = Some((self.generation, snapshot.clone()));
        Ok(snapshot)
    }
}

impl Drop for QueryLease {
    fn drop(&mut self) {
        let mut state = lock_state(&self.state);
        if self.owns_active(&state) {
            state.active = None;
        }
    }
}

fn validate_scan_scope(
    requested: Option<ProviderId>,
    scans: &[ProviderScan],
    tool: InventoryTool,
) -> Result<(), QueryError> {
    let providers: BTreeSet<_> = scans.iter().map(|scan| scan.report.provider_id).collect();
    if providers.len() != scans.len() {
        return Err(QueryError::InvalidScan("同一来源重复返回"));
    }
    if scans
        .iter()
        .any(|scan| scan.observations.len() > CACHED_OBSERVATION_LIMIT)
    {
        return Err(QueryError::InvalidScan("单来源本次观测数量超过缓存上限"));
    }
    if scans.iter().any(|s| {
        s.report.coverage.tool_ids.iter().any(|id| *id != tool.id())
            || s.observations.iter().any(|o| o.tool_id != tool.id())
    }) {
        return Err(QueryError::InvalidScan("工具覆盖与请求范围不一致"));
    }
    match requested {
        Some(provider) if providers.len() == 1 && providers.contains(&provider) => Ok(()),
        None if providers == tool.providers().iter().copied().collect() => Ok(()),
        _ => Err(QueryError::InvalidScan("返回来源与请求范围不一致")),
    }
}

fn refresh_source(previous: Option<&ProviderScan>, mut fresh: ProviderScan) -> ProviderScan {
    let authoritative = fresh.report.status == ProviderStatus::Ok
        && fresh.report.error.is_none()
        && !fresh.report.stale
        && fresh.report.coverage.complete
        && !fresh.report.coverage.truncated;
    if authoritative {
        return fresh;
    }
    if let Some(previous) = previous {
        let current_locators: BTreeSet<_> = fresh
            .observations
            .iter()
            .map(|observation| observation.locator.clone())
            .collect();
        let mut retained: Vec<_> = previous
            .observations
            .iter()
            .filter(|observation| !current_locators.contains(&observation.locator))
            .cloned()
            .collect();
        // 优先保留全部本次观测；旧证据按内容排序，使保留范围不依赖目录遍历顺序。
        retained.sort();
        let remaining = CACHED_OBSERVATION_LIMIT.saturating_sub(fresh.observations.len());
        if retained.len() > remaining {
            retained.truncate(remaining);
            fresh.report.coverage.complete = false;
            fresh.report.coverage.truncated = true;
            fresh.report.stale = true;
        }
        if !retained.is_empty() {
            fresh.report.stale = true;
            fresh.observations.extend(retained);
        }
    }
    fresh
}

fn unscanned_source_for(provider_id: ProviderId, tool: InventoryTool) -> ProviderScan {
    ProviderScan {
        report: ProviderReport {
            provider_id,
            status: ProviderStatus::Error,
            observed_at: 0,
            stale: false,
            coverage: ScanCoverage {
                tool_ids: vec![tool.id()],
                complete: false,
                truncated: false,
            },
            error: Some(ProviderError {
                code: "notScanned".to_owned(),
                message: "此来源尚未扫描".to_owned(),
                retryable: true,
            }),
        },
        observations: Vec::new(),
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(MAX_SAFE_TIMESTAMP)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
