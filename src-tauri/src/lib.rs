mod mise;
mod system;

use std::sync::Mutex;
use tauri::Manager;

#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

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
#[tauri::command]
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
#[tauri::command]
fn check_mise() -> mise::MiseStatus {
    mise::mise_status()
}

/// 扫描其他工具托管（nvm/pyenv/asdf/sdkman/rvm 等）的运行时。
#[tauri::command]
fn detect_tool_sources() -> Vec<mise::ToolSource> {
    mise::detect_tool_sources()
}

/// 列出系统当前安装的所有工具。
#[tauri::command]
fn list_tools() -> Result<Vec<mise::ToolInfo>, String> {
    mise::list_tools()
}

/// 查看某个工具的远程可安装版本。
#[tauri::command]
fn list_remote_versions(tool: String) -> Result<Vec<String>, String> {
    mise::list_remote_versions(&tool)
}

/// 通过 ASDF 源获取远程版本。
#[tauri::command]
fn list_remote_versions_asdf(tool: String) -> Result<Vec<String>, String> {
    mise::list_remote_versions_asdf(&tool)
}

/// 通过 GitHub Releases Tags 获取远程版本。
#[tauri::command]
fn list_remote_versions_github(tool: String) -> Result<Vec<String>, String> {
    mise::list_remote_versions_github(&tool)
}

/// 通过“官方生态源”（node/go/python/java 官方）获取远程版本。
#[tauri::command]
fn list_remote_versions_official(tool: String) -> Result<Vec<String>, String> {
    mise::list_remote_versions_official(&tool)
}

/// 列出 mise 支持的全部运行时名。
#[tauri::command]
fn list_registry() -> Result<Vec<String>, String> {
    mise::list_registry()
}

/// 安装指定工具版本。
#[tauri::command]
fn install_version(tool: String, version: String) -> Result<String, String> {
    mise::install_version(&tool, &version)
}

/// 流式安装指定工具版本，实时推送进度事件到前端。
#[tauri::command]
async fn install_version_streaming(
    app: tauri::AppHandle,
    tool: String,
    version: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mise::install_version_streaming(&app, &tool, &version)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 卸载指定工具版本。
#[tauri::command]
fn uninstall_version(tool: String, version: String) -> Result<String, String> {
    mise::uninstall_version(&tool, &version)
}

/// 切换（激活）某个工具版本。global 决定是否写入全局配置。
#[tauri::command]
fn use_version(tool: String, version: String, global: bool) -> Result<String, String> {
    mise::use_version(&tool, &version, global)
}

/// 接管：把已存在的外部环境目录链接为 mise 管理版本。
#[tauri::command]
fn link_version(tool: String, version: String, path: String) -> Result<String, String> {
    mise::link_version(&tool, &version, &path)
}

/// 解除接管：移除某版本与外部目录的链接。
#[tauri::command]
fn unlink_version(tool: String, version: String) -> Result<String, String> {
    mise::unlink_version(&tool, &version)
}

/// 读取项目根目录的 mise 配置文件内容。
#[tauri::command]
fn read_project_config(path: String) -> Result<String, String> {
    mise::read_project_config(&path)
}

/// 将内容写入项目的 mise.toml。
#[tauri::command]
fn write_project_config(path: String, content: String) -> Result<String, String> {
    mise::write_project_config(&path, &content)
}

/// 写入配置并在项目内一键安装全套环境。
#[tauri::command]
fn install_all_project(path: String, content: String) -> Result<String, String> {
    mise::install_all_project(&path, &content)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppData::default())
        .setup(|app| {
            // 启动时一次性读取硬件静态信息（CPU/内存/存储/GPU 型号），此后仅复用缓存
            {
                let state = app.state::<AppData>();
                let mut sys = state.system.lock().unwrap();
                let mut hw = state.hardware.lock().unwrap();
                if hw.is_none() {
                    *hw = Some(system::collect_hardware(&mut sys));
                }
            }
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_stats,
            check_mise,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
