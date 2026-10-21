mod cache;
mod caches;
mod config;
mod discover;
mod doctor;
pub mod domain;
mod env;
mod env_center;
pub use env_center::global_config_path;
mod error;
mod history;
pub mod inventory;
pub mod launcher;
mod managed;
mod mise;
mod net;
pub mod operation_plan;
mod operations;
mod preset;
mod process;
mod project;
pub mod project_environment;
pub mod providers;
mod services;
mod snapshot;
mod sources;
mod syscache;
mod system;
pub mod tool_catalog;

use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tauri_plugin_autostart::MacosLauncher;

/// 记录一条操作历史；失败详情一并留痕，记录本身不影响业务结果。
fn traced<E: std::fmt::Display>(kind: &str, detail: String, r: &Result<String, E>) {
    match r {
        Ok(_) => history::record(kind, detail),
        Err(_) => history::record(kind, format!("{detail} 失败（详情见操作页面）")),
    }
}

#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

// 说明：Tauri 的同步 command 默认在主线程执行，任何子进程/网络调用（如 brew list 可达数十秒）
// 都会冻结整个窗口并饿死 system_stats 轮询（表现为页面卡顿、本机信息刷不出来）。
// 因此除已是 async fn 的流式命令外，全部命令标记 (async) 转到后台线程执行；
// 函数签名与前端调用方式保持不变。
//
// 可共享的全局状态：持有复用的 System 实例以计算 CPU 利用率，
// NetworkState 以计算网络速率，以及缓存的硬件静态信息。
pub struct AppData {
    pub system: Mutex<sysinfo::System>,
    pub network: Mutex<system::NetworkState>,
    pub hardware: Mutex<Option<system::Hardware>>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            system: Mutex::new(sysinfo::System::new()),
            network: Mutex::new(system::NetworkState::default()),
            hardware: Mutex::new(None),
        }
    }
}

/// 收集一次系统资源快照。硬件静态信息只采集一次并缓存；
/// 每次轮询仅计算动态指标（快），避免 system_profiler 反复执行。
#[tauri::command(async)]
fn system_stats(state: tauri::State<'_, AppData>) -> Result<system::SystemStats, String> {
    let mut sys = state.system.lock().map_err(|e| e.to_string())?;
    let mut net = state.network.lock().map_err(|e| e.to_string())?;
    let mut hw = state.hardware.lock().map_err(|e| e.to_string())?;
    if hw.is_none() {
        *hw = Some(system::collect_hardware(&mut sys));
    }
    Ok(system::collect_dynamic(
        &mut sys,
        &mut net,
        hw.as_ref().unwrap(),
    ))
}

/// 纯工具目录与当前平台接入账本，不探测或修改本机。
#[tauri::command]
fn tool_catalog() -> tool_catalog::ToolCatalog {
    tool_catalog::current_catalog()
}

/// 检测 mise 的安装状态与版本。
#[tauri::command(async)]
fn check_mise() -> mise::MiseStatus {
    mise::mise_status()
}

/// 采集一次性环境信息：mise/git 版本、系统信息与各包管理器版本。
#[tauri::command(async)]
fn get_env_info() -> env::EnvInfo {
    env::collect_env_info()
}

/// 通过系统包管理器（brew/winget/apt/pacman）原生安装指定软件，流式推送进度。
#[tauri::command]
async fn install_system_package(
    app: tauri::AppHandle,
    manager: String,
    name: String,
    kind: String,
    job_id: Option<String>,
) -> Result<String, String> {
    let mgr = manager.clone();
    let (m2, n2) = (manager.clone(), name.clone());
    let r = tauri::async_runtime::spawn_blocking(move || {
        operations::run(job_id, "安装系统软件", |id| {
            env::install_system_package(&app, &m2, &n2, &kind, id)
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    // 安装结束后（无论成败）失效该管理器的探测缓存，下次探测拿到真实状态
    syscache::invalidate(&mgr);
    let r = r.map_err(String::from);
    traced("sys-install", format!("{manager} install {name}"), &r);
    r
}

/// 探测指定系统包管理器当前已安装的软件名列表。
#[tauri::command(async)]
fn detect_system_installed(manager: String) -> Result<Vec<String>, String> {
    // brew list 在包多时可达数十秒：TTL 内直接命中缓存（安装/卸载后主动失效）
    if let Some(cached) = syscache::get_installed(&manager) {
        return Ok(cached);
    }
    let pkgs = env::detect_system_installed(&manager)?;
    syscache::set_installed(&manager, pkgs.clone());
    Ok(pkgs)
}

/// 探测指定系统包管理器当前已装的软件及其版本。
#[tauri::command(async)]
fn detect_system_versions(manager: String) -> Result<Vec<env::SystemPkg>, String> {
    if let Some(cached) = syscache::get_versions(&manager) {
        return Ok(cached);
    }
    let pkgs = env::detect_system_versions(&manager)?;
    syscache::set_versions(&manager, pkgs.clone());
    Ok(pkgs)
}

/// 原生卸载系统软件包（brew/winget/apt/pacman）。
#[tauri::command(async)]
fn uninstall_system_package(manager: String, name: String) -> Result<String, String> {
    let r = operations::run(None, "卸载软件", |_| {
        env::uninstall_system_package(&manager, &name)
    });
    // 卸载结束后失效该管理器的探测缓存
    syscache::invalidate(&manager);
    let r = r.map_err(String::from);
    traced("sys-uninstall", format!("{manager} uninstall {name}"), &r);
    r
}

/// 扫描其他工具托管（nvm/pyenv/asdf/sdkman/rvm 等）的运行时。
#[tauri::command(async)]
fn detect_tool_sources() -> Vec<mise::ToolSource> {
    mise::detect_tool_sources()
}

/// 列出系统当前安装的所有工具。
#[tauri::command(async)]
fn list_tools() -> Result<Vec<mise::ToolInfo>, String> {
    Ok(mise::list_tools()?)
}

/// 查看某个工具的远程可安装版本。
#[tauri::command(async)]
fn list_remote_versions(tool: String) -> Result<Vec<String>, String> {
    Ok(sources::list_remote_versions(&tool)?)
}

/// 通过 ASDF 源获取远程版本。
#[tauri::command(async)]
fn list_remote_versions_asdf(tool: String) -> Result<Vec<String>, String> {
    Ok(sources::list_remote_versions_asdf(&tool)?)
}

/// 通过 GitHub Releases Tags 获取远程版本。
#[tauri::command(async)]
fn list_remote_versions_github(tool: String) -> Result<Vec<String>, String> {
    Ok(sources::list_remote_versions_github(&tool)?)
}

/// 通过“官方生态源”（node/go/python/java 官方）获取远程版本。
#[tauri::command(async)]
fn list_remote_versions_official(tool: String) -> Result<Vec<String>, String> {
    Ok(sources::list_remote_versions_official(&tool)?)
}

/// 列出 mise 支持的全部运行时名。
#[tauri::command(async)]
fn list_registry() -> Result<Vec<String>, String> {
    Ok(mise::list_registry()?)
}

/// 安装指定工具版本。
#[tauri::command(async)]
fn install_version(tool: String, version: String) -> Result<String, String> {
    let r = operations::run(None, "安装运行时", |_| {
        mise::install_version(&tool, &version)
    });
    traced("install", format!("mise {tool}@{version}"), &r);
    Ok(r?)
}

/// 流式安装指定工具版本，实时推送进度事件到前端。
#[tauri::command]
async fn install_version_streaming(
    app: tauri::AppHandle,
    tool: String,
    version: String,
    job_id: Option<String>,
) -> Result<String, String> {
    let (t2, v2) = (tool.clone(), version.clone());
    let inner = tauri::async_runtime::spawn_blocking(move || {
        operations::run(job_id, "安装运行时", |id| {
            mise::install_version_streaming(&app, &t2, &v2, id)
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    let r = inner.map_err(String::from);
    traced("install", format!("mise {tool}@{version}"), &r);
    r
}

/// 卸载指定工具版本。
#[tauri::command(async)]
fn uninstall_version(tool: String, version: String) -> Result<String, String> {
    let r = operations::run(None, "卸载运行时", |_| {
        mise::uninstall_version(&tool, &version)
    });
    traced("uninstall", format!("mise {tool}@{version}"), &r);
    Ok(r?)
}

/// 切换（激活）某个工具版本。global 决定是否写入全局配置。
#[tauri::command(async)]
fn use_version(tool: String, version: String, global: bool) -> Result<String, String> {
    let r = operations::run(None, "激活运行时", |_| {
        mise::use_version(&tool, &version, global)
    });
    traced("use", format!("{tool} → {version}（global={global}）"), &r);
    Ok(r?)
}

/// 接管：把已存在的外部环境目录链接为 mise 管理版本。
#[tauri::command(async)]
fn link_version(tool: String, version: String, path: String) -> Result<String, String> {
    let r = operations::run(None, "接管运行时", |_| {
        mise::link_version(&tool, &version, &path)
    });
    traced("adopt", format!("mise link {tool}@{version} ← {path}"), &r);
    Ok(r?)
}

/// 解除接管：移除某版本与外部目录的链接。
#[tauri::command(async)]
fn unlink_version(tool: String, version: String) -> Result<String, String> {
    let r = operations::run(None, "解除接管", |_| {
        mise::unlink_version(&tool, &version)
    });
    traced("unadopt", format!("mise unlink {tool}@{version}"), &r);
    Ok(r?)
}

/// 读取项目根目录的 mise 配置文件内容。
#[tauri::command(async)]
fn read_project_config(path: String) -> Result<project::ProjectDocument, String> {
    project::read(&path).map_err(String::from)
}

#[tauri::command(async)]
fn write_project_config(
    path: String,
    content: String,
    original: Option<String>,
) -> Result<String, String> {
    operations::run(None, "保存项目配置", |_| {
        project::save(&path, &content, original.as_deref())
    })
    .map_err(String::from)
}

#[tauri::command(async)]
fn config_bind_path(content: String, path: String) -> Result<String, String> {
    config::bind_path(&content, &path).map_err(String::from)
}

#[tauri::command(async)]
fn config_replace_tools(
    content: String,
    tools: std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    config::replace_tools(&content, &tools).map_err(String::from)
}

#[tauri::command(async)]
fn install_all_project(
    app: tauri::AppHandle,
    path: String,
    content: String,
    original: Option<String>,
    job_id: Option<String>,
) -> Result<String, String> {
    let r = operations::run(job_id, "项目安装", |id| {
        project::save(&path, &content, original.as_deref())?;
        mise::install_all_project(&app, &path, id)
    });
    traced("install-all", format!("项目一键安装 {path}"), &r);
    r.map_err(String::from)
}

/// 托管接入：按来源自动选策略（用户级目录直连 / brew 走托管软链），接入后校验 mise 注册结果
#[tauri::command(async)]
fn managed_adopt(
    tool: String,
    version: String,
    manager: String,
    path: String,
) -> Result<String, String> {
    let r = operations::run(None, "托管接入", |_| {
        managed::adopt(&tool, &version, &manager, &path)
    });
    traced("adopt", format!("{tool}@{version} ← {manager}:{path}"), &r);
    Ok(r?)
}

/// 解除托管接入（移除 mise 链接与农场记录）
#[tauri::command(async)]
fn managed_unadopt(tool: String, version: String) -> Result<String, String> {
    let r = operations::run(None, "解除托管", |_| managed::unadopt(&tool, &version));
    traced("unadopt", format!("{tool}@{version}"), &r);
    Ok(r?)
}

/// 对账：只修复同软件同版本，缺失版本保留待处理，返回事件供前端展示。
#[tauri::command(async)]
fn managed_reconcile() -> Result<Vec<managed::ReconcileEvent>, String> {
    let events =
        operations::run(None, "托管对账", |_| managed::reconcile()).map_err(String::from)?;
    if !events.is_empty() {
        let rel = events.iter().filter(|e| e.action == "relinked").count();
        let pending = events.iter().filter(|e| e.action == "needs-review").count();
        let fail = events.iter().filter(|e| e.action == "failed").count();
        history::record(
            "reconcile",
            format!("托管对账：重连 {rel}、待处理 {pending}、失败 {fail}"),
        );
    }
    Ok(events)
}

/// 托管接入清单与健康状态
#[tauri::command(async)]
fn managed_list() -> Result<Vec<managed::ManagedEntryHealthy>, String> {
    managed::list().map_err(String::from)
}

/// 环境体检：全量巡检整机环境健康度，返回各项结果与修复建议（只读，不做修复动作）
#[tauri::command(async)]
fn doctor_run() -> Vec<doctor::DoctorCheck> {
    doctor::run()
}

/// 环境体检修复：执行一个检查项的修复动作（托管对账 / shell 集成追加）
#[tauri::command(async)]
fn doctor_fix(id: String) -> Result<String, String> {
    let r = operations::run(None, "环境修复", |_| doctor::fix(&id)).map_err(String::from);
    traced("doctor-fix", format!("体检修复 {id}"), &r);
    r
}

/// 环境变量中心：全局 mise config 的 [env] 列表 + 系统 env 冲突检测
#[tauri::command(async)]
fn env_center_list() -> Result<env_center::EnvCenterSnapshot, String> {
    env_center::list().map_err(String::from)
}

/// 设置一个全局 env（结构化更新全局 mise config 的 [env] 段，保留其余内容）
#[tauri::command(async)]
fn env_center_set(key: String, value: String) -> Result<String, String> {
    let r = operations::run(None, "修改环境变量", |_| {
        env_center::set(&key, &value)
    })
    .map_err(String::from);
    // 不把变量值或可能含值的解析错误复制到历史。
    history::record(
        "env-set",
        format!("{key}：{}", if r.is_ok() { "已更新" } else { "失败" }),
    );
    r
}

/// 移除一个全局 env
#[tauri::command(async)]
fn env_center_remove(key: String) -> Result<String, String> {
    let r =
        operations::run(None, "删除环境变量", |_| env_center::remove(&key)).map_err(String::from);
    traced("env-remove", key.clone(), &r);
    r
}

/// 操作历史：最近 200 条管理动作（新 → 旧）
#[tauri::command(async)]
fn history_list() -> Vec<history::HistoryEntry> {
    history::list(200)
}

/// 环境快照：导出整机环境清单（mise 工具 + 全局 env + brew 清单）为 TOML
#[tauri::command(async)]
fn snapshot_export(path: String, include_env: bool) -> Result<snapshot::SnapshotSummary, String> {
    let r = snapshot::export(&path, include_env);
    match &r {
        Ok(s) => history::record(
            "snapshot-export",
            format!(
                "{}（工具 {}、env {}、brew {}）",
                path, s.tools, s.env_vars, s.brew_packages
            ),
        ),
        Err(_) => history::record("snapshot-export", format!("{path} 失败（详情见操作页面）")),
    }
    r.map_err(String::from)
}

/// 环境快照：重建（全局 env 写入 + mise 工具逐个流式安装 + brew 清单生成 Brewfile）
#[tauri::command(async)]
fn snapshot_restore(
    app: tauri::AppHandle,
    path: String,
    expected: String,
    original: Option<String>,
    job_id: String,
) -> Result<String, String> {
    let r = operations::run(Some(job_id), "快照恢复", |id| {
        snapshot::restore(&app, &path, &expected, original.as_deref(), id)
    })
    .map_err(String::from);
    traced("snapshot-restore", path.clone(), &r);
    r
}

#[tauri::command(async)]
fn snapshot_preview(path: String) -> Result<snapshot::SnapshotPlan, String> {
    snapshot::preview(&path).map_err(String::from)
}

/// 环境预设：从 mise.toml 风格文本提取工具清单（供「装到整机」解析用户预设）
#[tauri::command(async)]
fn preset_parse_tools(toml: String) -> Result<Vec<preset::PresetTool>, String> {
    preset::parse_tools(&toml).map_err(String::from)
}

/// 环境预设：导出为可分享的预设文件（Z.Env 预设 TOML v1）
#[tauri::command(async)]
fn preset_export(
    path: String,
    name: String,
    description: String,
    toml: String,
) -> Result<String, String> {
    let r = preset::export(&path, &name, &description, &toml, env!("CARGO_PKG_VERSION"))
        .map(|n| format!("已导出 {n} 个工具到 {path}"));
    traced("preset-export", format!("环境预设「{name}」→ {path}"), &r);
    r.map_err(String::from)
}

/// 环境预设：导入预设文件（Z.Env 预设 TOML v1，兼容纯 mise.toml）
#[tauri::command(async)]
fn preset_import(path: String) -> Result<preset::PresetFile, String> {
    let r = preset::import(&path);
    // 返回值不是 String，用不了 traced，按同样方式手写留痕
    match &r {
        Ok(p) => history::record(
            "preset-import",
            format!("环境预设「{}」（{} 个工具）← {path}", p.name, p.tools.len()),
        ),
        Err(_) => history::record("preset-import", format!("{path} 失败（详情见操作页面）")),
    }
    r.map_err(String::from)
}

/// 项目发现：扫描常用目录，识别 .git / 技术栈指纹 / mise.toml，对照已装工具给出缺失清单
#[tauri::command(async)]
fn discover_projects() -> Vec<discover::DiscoveredProject> {
    discover::run()
}

/// 本地服务列表：brew services 与 systemd 用户级服务（缺哪个跳哪个）
#[tauri::command(async)]
fn service_list() -> services::ServiceOverview {
    services::list()
}

/// 对本地服务执行操作（start / stop / restart）
#[tauri::command(async)]
fn service_action(manager: String, name: String, act: String) -> Result<String, String> {
    let r = operations::run(None, "服务操作", |_| {
        services::action(&manager, &name, &act)
    })
    .map_err(String::from);
    traced("service", format!("{manager} {act} {name}"), &r);
    r
}

/// 开发缓存列表：常见缓存目录与体积（探测到哪个列哪个）
#[tauri::command(async)]
fn cache_list() -> Vec<caches::CacheInfo> {
    caches::list()
}

/// 用官方命令清理指定开发缓存
#[tauri::command(async)]
fn cache_clean(id: String) -> Result<String, String> {
    let r = operations::run(None, "清理缓存", |_| caches::clean(&id)).map_err(String::from);
    traced("cache-clean", id.clone(), &r);
    r
}

/// 策略 C：包管理器自维护的稳定 bin 路径（供项目 mise.toml 的 env._path 绑定）
#[tauri::command(async)]
fn stable_bin_path(manager: String, name: String, path: String) -> Result<String, String> {
    Ok(managed::stable_bin_path(&manager, &name, &path)?
        .display()
        .to_string())
}

#[tauri::command(async)]
fn package_catalog() -> Vec<env::ProviderPackage> {
    env::package_catalog()
}

#[tauri::command(async)]
fn operation_list(
    state: tauri::State<'_, operation_plan::OperationPlanService>,
) -> Vec<operations::Operation> {
    let mut jobs = operations::list();
    let (records, unavailable) = state.records();
    for record in records {
        if let Some(job) = jobs.iter_mut().find(|job| job.id == record.id) {
            job.steps = Some(record.steps);
            job.compensation = record.compensation;
        } else {
            jobs.push(operations::persisted(record));
        }
    }
    if unavailable {
        jobs.push(operations::persisted(operation_plan::ExecutionRecord {
            schema_version: 1,
            tool_id: crate::inventory::query::InventoryTool::Node,
            id: "journal-unavailable".into(),
            action: operation_plan::Action::Verify,
            status: operation_plan::ExecutionStatus::Failed,
            created_at: 0,
            updated_at: 0,
            steps: vec![],
            outcome: None,
            compensation: None,
        }));
        if let Some(job) = jobs.last_mut() {
            job.label = "操作记录不可用".into();
            job.lines.push_back(
                "新操作已停止执行，请检查 ~/.zenv/operations-v1.json；旧入口继续保留".into(),
            );
        }
    }
    jobs
}

#[tauri::command(async)]
fn operation_plan(
    state: tauri::State<'_, operation_plan::OperationPlanService>,
    inventory: tauri::State<'_, inventory::query::InventoryQueryService>,
    request: operation_plan::PlanRequest,
    tool_id: Option<inventory::query::InventoryTool>,
) -> Result<operation_plan::OperationPlan, String> {
    let result = match tool_id.unwrap_or_default() {
        inventory::query::InventoryTool::Node => state.preview_current(&inventory, request),
        inventory::query::InventoryTool::Python => {
            state.preview_python_current(&inventory, request)
        }
    };
    result.map_err(|e| e.to_string())
}

/// v2 明确工具目标和步骤依赖；后端仍在同一服务签发可单次执行的计划。
#[tauri::command(async)]
fn operation_plan_v2(
    state: tauri::State<'_, operation_plan::OperationPlanService>,
    inventory: tauri::State<'_, inventory::query::InventoryQueryService>,
    request: operation_plan::PlanRequestV2,
) -> Result<operation_plan::OperationPlanV2, String> {
    state
        .preview_v2_current(&inventory, request)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn operation_bindings(
    state: tauri::State<'_, operation_plan::OperationPlanService>,
) -> Result<Vec<operation_plan::BindingSummary>, String> {
    state.bindings().map_err(|e| e.to_string())
}

/// 只读项目核对共用清单租约，取消与变更互斥边界保持一致。
#[tauri::command]
async fn project_environment(
    inventory: tauri::State<'_, inventory::query::InventoryQueryService>,
    request: project_environment::EnvironmentRequest,
) -> Result<project_environment::ProjectEnvironment, String> {
    let lease = inventory
        .begin(request.request_id.clone(), None)
        .map_err(|e| e.to_string())?;
    let options =
        providers::NodeScanOptions::from_environment().map_err(|_| "无法读取扫描上下文")?;
    let global = env_center::global_config_path().ok_or("无法定位全局配置")?;
    let expected = request.snapshot_request_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        lease
            .read_snapshot(&expected, |snapshot, token| {
                project_environment::inspect(&request, snapshot, &options, &global, token)
            })
            .map_err(|e| e.to_string())?
    })
    .await
    .map_err(|_| "项目核对线程中断，请重试".to_owned())?
}

/// Python 项目声明、候选匹配与当前选择只读核对；不写配置、不执行解释器。
#[tauri::command]
async fn python_project_environment(
    inventory: tauri::State<'_, inventory::query::InventoryQueryService>,
    request: project_environment::PythonProjectRequest,
) -> Result<project_environment::PythonProjectEnvironment, String> {
    let lease = inventory
        .begin_tool(
            request.request_id.clone(),
            None,
            inventory::query::InventoryTool::Python,
        )
        .map_err(|e| e.to_string())?;
    let expected = request.snapshot_request_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        lease
            .read_snapshot(&expected, |snapshot, _| {
                project_environment::inspect_python(snapshot, &request)
            })
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Python 项目核对线程中断，请重试".to_owned())?
}

/// Python 绑定收据列表；只读私有账本，不探测安装或改写配置。
#[tauri::command(async)]
fn python_bindings() -> Result<Vec<operation_plan::PythonBinding>, String> {
    let options = providers::python::PythonScanOptions::from_environment()
        .map_err(|_| "无法读取 Python 扫描上下文")?;
    operation_plan::python_bindings(&options).map_err(|error| error.to_string())
}

#[tauri::command]
async fn operation_execute(
    state: tauri::State<'_, operation_plan::OperationPlanService>,
    inventory: tauri::State<'_, inventory::query::InventoryQueryService>,
    plan_id: String,
) -> Result<operation_plan::ExecutionRecord, String> {
    let service = state.inner().clone();
    let query = inventory.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.execute(plan_id, query).map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "操作执行线程中断，请查看任务记录".to_string())?
}

#[tauri::command(async)]
fn operation_cancel(
    state: tauri::State<'_, operation_plan::OperationPlanService>,
    id: String,
) -> Result<(), String> {
    match operations::cancel(&id) {
        Ok(()) => Ok(()),
        Err(_) if state.cancel_pending(&id) => Ok(()),
        Err(error) => Err(String::from(error)),
    }
}

/// 请求先登记再转后台执行，取消可在工作线程启动前命中同一租约。
#[tauri::command]
async fn inventory_snapshot(
    state: tauri::State<'_, inventory::query::InventoryQueryService>,
    request_id: String,
    provider_id: Option<domain::ProviderId>,
    tool_id: Option<inventory::query::InventoryTool>,
) -> Result<domain::InventorySnapshot, String> {
    let tool = tool_id.unwrap_or_default();
    let lease = state
        .begin_tool(request_id, provider_id, tool)
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || match tool {
        inventory::query::InventoryTool::Node => {
            let options = providers::NodeScanOptions::from_environment()
                .map_err(|error| error.to_string())?;
            lease.execute(options).map_err(|error| error.to_string())
        }
        inventory::query::InventoryTool::Python => {
            let options = providers::python::PythonScanOptions::from_environment()
                .map_err(|error| error.to_string())?;
            lease
                .execute_python(options)
                .map_err(|error| error.to_string())
        }
    })
    .await
    .map_err(|_| "工具清单刷新异常中断".to_string())?
}

#[tauri::command(async)]
fn inventory_cancel(
    state: tauri::State<'_, inventory::query::InventoryQueryService>,
    request_id: String,
) -> Result<bool, String> {
    state.cancel(&request_id).map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn install_preset(
    app: tauri::AppHandle,
    tools: Vec<preset::PresetTool>,
    job_id: String,
) -> Result<usize, String> {
    operations::run(Some(job_id), "整机预设安装", |id| {
        for t in &tools {
            mise::validate_spec(&t.name, &t.version)?;
        }
        let mut ok = 0;
        for t in &tools {
            if operations::cancelled(id) {
                break;
            }
            match mise::install_version_streaming(&app, &t.name, &t.version, id) {
                Ok(_) => {
                    ok += 1;
                    operations::log(id, format!("{}@{} 已安装", t.name, t.version));
                }
                Err(_) => operations::log(id, format!("{}@{} 安装失败", t.name, t.version)),
            }
        }
        if ok < tools.len() {
            operations::partial(id);
        }
        Ok(ok)
    })
    .map_err(String::from)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppData::default())
        .manage(inventory::query::InventoryQueryService::default())
        .manage(operation_plan::OperationPlanService::new(
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
                .join(".zenv/operations-v1.json"),
        ))
        .setup(|app| {
            // 只恢复已有记录；不重放安装，也不在新的只读会话中创建 journal。
            let _ = app
                .state::<operation_plan::OperationPlanService>()
                .recover();
            // 不再在启动时同步采集硬件信息；改为首次 system_stats 时惰性计算，窗口可立即显示，
            // 避免首次安装/启动因 system_profiler / 磁盘扫描（尤其 Windows）导致加载缓慢。
            // macOS：应用“液态玻璃”风格的毛玻璃背景
            #[cfg(target_os = "macos")]
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = apply_vibrancy(
                        &win,
                        NSVisualEffectMaterial::HudWindow,
                        Some(NSVisualEffectState::Active),
                        Some(10.0),
                    );
                }
            }
            // Windows：应用亚克力毛玻璃背景，避免 transparent 窗口呈现全透明
            #[cfg(target_os = "windows")]
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = window_vibrancy::apply_acrylic(&win, Some((18, 18, 18, 150)));
                }
            }

            // 系统托盘：左键点击显示/聚焦主窗口，右键菜单提供打开与退出
            #[cfg(desktop)]
            {
                let open = MenuItem::with_id(app, "open", "打开 Z.Env", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &quit])?;
                TrayIconBuilder::with_id("main-tray")
                    .icon(app.default_window_icon().unwrap().clone())
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(w) = tray.app_handle().get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    })
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "open" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .build(app)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_stats,
            check_mise,
            operation_list,
            package_catalog,
            operation_cancel,
            operation_plan,
            operation_plan_v2,
            operation_bindings,
            operation_execute,
            inventory_snapshot,
            tool_catalog,
            project_environment,
            python_project_environment,
            python_bindings,
            inventory_cancel,
            install_preset,
            get_env_info,
            install_system_package,
            detect_system_installed,
            detect_system_versions,
            uninstall_system_package,
            detect_tool_sources,
            list_tools,
            list_remote_versions,
            list_remote_versions_asdf,
            list_remote_versions_github,
            list_remote_versions_official,
            list_registry,
            install_version,
            install_version_streaming,
            uninstall_version,
            use_version,
            link_version,
            unlink_version,
            read_project_config,
            config_bind_path,
            config_replace_tools,
            write_project_config,
            install_all_project,
            managed_adopt,
            managed_unadopt,
            managed_reconcile,
            managed_list,
            doctor_run,
            doctor_fix,
            env_center_list,
            env_center_set,
            env_center_remove,
            discover_projects,
            service_list,
            service_action,
            cache_list,
            cache_clean,
            history_list,
            snapshot_export,
            snapshot_restore,
            snapshot_preview,
            preset_export,
            preset_import,
            preset_parse_tools,
            stable_bin_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
