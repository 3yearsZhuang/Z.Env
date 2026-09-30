mod cache;
mod doctor;
mod env;
mod env_center;
mod error;
mod managed;
mod mise;
mod net;
mod sources;
mod syscache;
mod system;

use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tauri_plugin_autostart::MacosLauncher;

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
) -> Result<String, String> {
    let mgr = manager.clone();
    let r = tauri::async_runtime::spawn_blocking(move || {
        env::install_system_package(&app, &manager, &name)
    })
    .await
    .map_err(|e| e.to_string())?;
    // 安装结束后（无论成败）失效该管理器的探测缓存，下次探测拿到真实状态
    syscache::invalidate(&mgr);
    r.map_err(String::from)
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
    let r = env::uninstall_system_package(&manager, &name);
    // 卸载结束后失效该管理器的探测缓存
    syscache::invalidate(&manager);
    r.map_err(String::from)
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
    Ok(mise::install_version(&tool, &version)?)
}

/// 流式安装指定工具版本，实时推送进度事件到前端。
#[tauri::command]
async fn install_version_streaming(
    app: tauri::AppHandle,
    tool: String,
    version: String,
) -> Result<String, String> {
    // 第一个 ? 处理 JoinError，第二个 ? 经 From<AppError> for String 转换内部错误
    Ok(tauri::async_runtime::spawn_blocking(move || {
        mise::install_version_streaming(&app, &tool, &version)
    })
    .await
    .map_err(|e| e.to_string())??)
}

/// 卸载指定工具版本。
#[tauri::command(async)]
fn uninstall_version(tool: String, version: String) -> Result<String, String> {
    Ok(mise::uninstall_version(&tool, &version)?)
}

/// 切换（激活）某个工具版本。global 决定是否写入全局配置。
#[tauri::command(async)]
fn use_version(tool: String, version: String, global: bool) -> Result<String, String> {
    Ok(mise::use_version(&tool, &version, global)?)
}

/// 接管：把已存在的外部环境目录链接为 mise 管理版本。
#[tauri::command(async)]
fn link_version(tool: String, version: String, path: String) -> Result<String, String> {
    Ok(mise::link_version(&tool, &version, &path)?)
}

/// 解除接管：移除某版本与外部目录的链接。
#[tauri::command(async)]
fn unlink_version(tool: String, version: String) -> Result<String, String> {
    Ok(mise::unlink_version(&tool, &version)?)
}

/// 读取项目根目录的 mise 配置文件内容。
#[tauri::command(async)]
fn read_project_config(path: String) -> Result<String, String> {
    Ok(mise::read_project_config(&path)?)
}

/// 将内容写入项目的 mise.toml。
#[tauri::command(async)]
fn write_project_config(path: String, content: String) -> Result<String, String> {
    Ok(mise::write_project_config(&path, &content)?)
}

/// 写入配置并在项目内一键安装全套环境。
#[tauri::command(async)]
fn install_all_project(path: String, content: String) -> Result<String, String> {
    Ok(mise::install_all_project(&path, &content)?)
}

/// 托管接入：按来源自动选策略（用户级目录直连 / brew 走托管软链），接入后校验 mise 注册结果
#[tauri::command(async)]
fn managed_adopt(
    tool: String,
    version: String,
    manager: String,
    path: String,
) -> Result<String, String> {
    Ok(managed::adopt(&tool, &version, &manager, &path)?)
}

/// 解除托管接入（移除 mise 链接与农场记录）
#[tauri::command(async)]
fn managed_unadopt(tool: String, version: String) -> Result<String, String> {
    Ok(managed::unadopt(&tool, &version)?)
}

/// 对账：brew 升级/卸载导致的失效接入自动重连到新版本或移除，返回事件供前端展示
#[tauri::command(async)]
fn managed_reconcile() -> Vec<managed::ReconcileEvent> {
    managed::reconcile()
}

/// 托管接入清单与健康状态
#[tauri::command(async)]
fn managed_list() -> Vec<managed::ManagedEntryHealthy> {
    managed::list()
}

/// 环境体检：全量巡检整机环境健康度，返回各项结果与修复建议（只读，不做修复动作）
#[tauri::command(async)]
fn doctor_run() -> Vec<doctor::DoctorCheck> {
    doctor::run()
}

/// 环境变量中心：全局 mise config 的 [env] 列表 + 系统 env 冲突检测
#[tauri::command(async)]
fn env_center_list() -> env_center::EnvCenterSnapshot {
    env_center::list()
}

/// 设置一个全局 env（写入全局 mise config 的 [env] 段，文本手术保留其余内容）
#[tauri::command(async)]
fn env_center_set(key: String, value: String) -> Result<String, String> {
    env_center::set(&key, &value).map_err(String::from)
}

/// 移除一个全局 env
#[tauri::command(async)]
fn env_center_remove(key: String) -> Result<String, String> {
    env_center::remove(&key).map_err(String::from)
}

/// 策略 C：包管理器自维护的稳定 bin 路径（供项目 mise.toml 的 env._path 绑定）
#[tauri::command(async)]
fn stable_bin_path(manager: String, name: String, path: String) -> Result<String, String> {
    Ok(managed::stable_bin_path(&manager, &name, &path)?
        .display()
        .to_string())
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
        .setup(|app| {
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
            write_project_config,
            install_all_project,
            managed_adopt,
            managed_unadopt,
            managed_reconcile,
            managed_list,
            doctor_run,
            env_center_list,
            env_center_set,
            env_center_remove,
            stable_bin_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
