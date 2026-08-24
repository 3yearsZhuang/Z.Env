//! 系统资源占用统计：CPU / 内存 / 磁盘 / 电量 / GPU / 网络。

use serde::Serialize;
use std::time::Instant;
use sysinfo::{Disks, Networks, System};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub model: Option<String>,
    pub total_gb: f64,
    pub available_gb: f64,
    pub percent: f64,
}

/// 内存条信息（型号 / 大小 / 速度）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub title: String,
    pub size: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vram: Option<String>,
}

/// 单张网卡的实时速率（字节/秒）与累计流量。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInterface {
    pub name: String,
    pub rx_rate: f64,
    pub tx_rate: f64,
    pub total_rx: u64,
    pub total_tx: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub cpu_cores: usize,
    pub cpu_brand: Option<String>,
    pub memory_total_gb: f64,
    pub memory_used_gb: f64,
    pub memory_percent: f64,
    pub memories: Vec<MemoryInfo>,
    pub storage: Vec<MemoryInfo>,
    pub disks: Vec<DiskInfo>,
    pub battery: Option<BatteryInfo>,
    pub gpus: Vec<GpuInfo>,
    pub interfaces: Vec<NetInterface>,
    pub os_name: Option<String>,
    pub host_name: Option<String>,
}

/// 用于跨两次调用计算网络速率的缓存状态。
#[derive(Default)]
pub struct NetworkState {
    prev: Option<(Instant, Vec<NetSample>)>,
}

struct NetSample {
    name: String,
    rx: u64,
    tx: u64,
}

const KB: f64 = 1024.0;
fn to_gb(bytes: u64) -> f64 {
    bytes as f64 / (KB * KB * KB)
}

/// 缓存的硬件静态信息：不随轮询频繁变化，避免反复执行缓慢的
/// `system_profiler` 导致概览页卡顿。首次获取后复用。
#[derive(Debug, Clone, Default)]
pub struct Hardware {
    pub cpu_brand: Option<String>,
    pub memories: Vec<MemoryInfo>,
    pub storage: Vec<MemoryInfo>,
    pub gpus: Vec<GpuInfo>,
}

/// 采集一次硬件静态信息（较慢，仅需少量次）。
pub fn collect_hardware(sys: &mut System) -> Hardware {
    // 刷新 CPU 详情以填充品牌（brand）信息；`refresh_cpu_usage()` 只更新占用率，
    // 不一定会带出型号，因此这里显式刷新 CPU 列表。
    sys.refresh_cpu_specifics(sysinfo::CpuRefreshKind::everything());
    Hardware {
        cpu_brand: sys.cpus().first().and_then(|c| {
            let b = c.brand().trim();
            if b.is_empty() {
                None
            } else {
                Some(b.to_string())
            }
        }),
        memories: read_memories(),
        storage: read_storage_devices(),
        gpus: read_gpus(),
    }
}

/// 收集一次动态统计快照（快速）。`sys` 需在应用生命周期内复用，
/// `net` 用于跨调用计算网络速率；硬件字段取自缓存的 `hw`。
pub fn collect_dynamic(sys: &mut System, net: &mut NetworkState, hw: &Hardware) -> SystemStats {
    // --- CPU ---
    sys.refresh_cpu_usage();
    let cpu_usage = sys.global_cpu_usage();
    let cpu_cores = sys.cpus().len();

    // --- 内存 ---
    sys.refresh_memory();
    let memory_total = to_gb(sys.total_memory());
    let memory_used = to_gb(sys.used_memory());
    let memory_percent = if memory_total > 0.0 {
        memory_used / memory_total * 100.0
    } else {
        0.0
    };

    // --- 磁盘（不读型号，快） ---
    let disks = collect_disks();

    // --- 网络 ---
    let interfaces = collect_networks(net);

    // --- 电量 ---
    let battery = read_battery();

    SystemStats {
        cpu_usage,
        cpu_cores,
        cpu_brand: hw.cpu_brand.clone(),
        memory_total_gb: memory_total,
        memory_used_gb: memory_used,
        memory_percent,
        memories: hw.memories.clone(),
        storage: hw.storage.clone(),
        disks,
        battery,
        gpus: hw.gpus.clone(),
        interfaces,
        os_name: System::name(),
        host_name: System::host_name(),
    }
}

/// 采集各网卡速率（字节/秒）。通过与前一次采样的字节差除以时间间隔计算。
pub fn collect_networks(state: &mut NetworkState) -> Vec<NetInterface> {
    let networks = Networks::new_with_refreshed_list();
    let now = Instant::now();

    let mut current: Vec<NetSample> = Vec::new();
    for (name, data) in &networks {
        let name = name.to_string();
        // 跳过回环接口，其速率参考意义不大
        if name.starts_with("lo") || name.eq_ignore_ascii_case("Loopback") {
            continue;
        }
        current.push(NetSample {
            name,
            rx: data.total_received(),
            tx: data.total_transmitted(),
        });
    }

    let mut out: Vec<NetInterface> = Vec::new();
    if let Some((prev_time, prev)) = &state.prev {
        let dt = now.duration_since(*prev_time).as_secs_f64();
        for sample in &current {
            let before = prev.iter().find(|p| p.name == sample.name);
            let (rx_rate, tx_rate) = match before {
                Some(b) if dt > 0.0 => (
                    sample.rx.saturating_sub(b.rx) as f64 / dt,
                    sample.tx.saturating_sub(b.tx) as f64 / dt,
                ),
                _ => (0.0, 0.0),
            };
            out.push(NetInterface {
                name: sample.name.clone(),
                rx_rate,
                tx_rate,
                total_rx: sample.rx,
                total_tx: sample.tx,
            });
        }
    } else {
        // 首次采样：无历史，速率为 0
        out = current
            .iter()
            .map(|s| NetInterface {
                name: s.name.clone(),
                rx_rate: 0.0,
                tx_rate: 0.0,
                total_rx: s.rx,
                total_tx: s.tx,
            })
            .collect();
    }

    // 更新缓存，供下次调用计算
    state.prev = Some((now, current));

    // 按下载速率降序排列，方便查看活跃接口
    out.sort_by(|a, b| {
        b.rx_rate
            .partial_cmp(&a.rx_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// 汇总所有磁盘的总容量与可用容量。
/// 按“(名称, 总容量)”去重，避免同一物理磁盘的多个挂载点重复展示。
fn collect_disks() -> Vec<DiskInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for disk in &disks {
        let total = disk.total_space();
        let available = disk.available_space();
        if total == 0 {
            continue;
        }
        // 去重 key：名称 + 总容量
        let name = disk.name().to_string_lossy().to_string();
        if !seen.insert((name.clone(), total)) {
            continue;
        }
        let percent = ((total - available) as f64 / total as f64) * 100.0;
        out.push(DiskInfo {
            name,
            // 磁盘型号由硬件缓存里的 storage 提供，避免每轮跑 diskutil
            model: None,
            total_gb: to_gb(total),
            available_gb: to_gb(available),
            percent,
        });
    }
    out
}

/// 跨平台读取内存硬件信息（型号 / 大小）。
fn read_memories() -> Vec<MemoryInfo> {
    #[cfg(target_os = "macos")]
    {
        read_macos_memories()
    }
    #[cfg(target_os = "windows")]
    {
        read_windows_memories()
    }
    #[cfg(target_os = "linux")]
    {
        read_linux_memories()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Vec::new()
    }
}

/// macOS：通过 `system_profiler SPMemoryDataType -json` 读取内存条信息。
#[cfg(target_os = "macos")]
fn read_macos_memories() -> Vec<MemoryInfo> {
    let out = std::process::Command::new("system_profiler")
        .args(["SPMemoryDataType", "-json"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else {
        return Vec::new();
    };
    let mut memories = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(list) = json.get("SPMemoryDataType").and_then(|v| v.as_array()) {
            for mem in list {
                // 容量在 "SPMemoryDataType" 字段（如 "16 GB"）
                let size = mem
                    .get("SPMemoryDataType")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "未知".to_string());
                let manufacturer = mem
                    .get("dimm_manufacturer")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let mem_type = mem
                    .get("dimm_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let parts: Vec<String> = [manufacturer, mem_type]
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect();
                let title = if parts.is_empty() {
                    "内存".to_string()
                } else {
                    parts.join(" ")
                };
                memories.push(MemoryInfo { title, size });
            }
        }
    }
    memories
}

/// 跨平台读取物理存储设备（型号 / 容量）。
fn read_storage_devices() -> Vec<MemoryInfo> {
    #[cfg(target_os = "macos")]
    {
        read_macos_storage()
    }
    #[cfg(target_os = "windows")]
    {
        read_windows_storage()
    }
    #[cfg(target_os = "linux")]
    {
        read_linux_storage()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Vec::new()
    }
}

/// macOS：通过 `system_profiler SPStorageDataType -json` 读取物理存储。
#[cfg(target_os = "macos")]
fn read_macos_storage() -> Vec<MemoryInfo> {
    let out = std::process::Command::new("system_profiler")
        .args(["SPStorageDataType", "-json"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else {
        return Vec::new();
    };
    let mut storage = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(list) = json.get("SPStorageDataType").and_then(|v| v.as_array()) {
            for dev in list {
                // Apple 芯片与 Intel 机型下字段名略有差异：优先取 device_model，
                // 缺失时回退到 _name / bsd_name。
                let model = dev
                    .get("device_model")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        dev.get("_name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    })
                    .or_else(|| {
                        dev.get("bsd_name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    })
                    .unwrap_or_else(|| "内建磁盘".to_string());
                let size_bytes = dev
                    .get("size_in_bytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let size = if size_bytes > 0 {
                    format!("{:.0} GB", to_gb(size_bytes))
                } else {
                    "未知容量".to_string()
                };
                storage.push(MemoryInfo { title: model, size });
            }
        }
    }
    storage
}

/// 读取 Windows 内存条（CIM）。
#[cfg(target_os = "windows")]
fn read_windows_memories() -> Vec<MemoryInfo> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_PhysicalMemory | Select-Object Manufacturer,ConfiguredClockSpeed,Capacity | ConvertTo-Json -Compress",
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else { return Vec::new() };
    let mut memories = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        let items: Vec<&serde_json::Value> = match json {
            serde_json::Value::Array(a) => a.iter().collect(),
            ref v => std::iter::once(v).collect(),
        };
        for m in items {
            let manu = m
                .get("Manufacturer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let speed = m
                .get("ConfiguredClockSpeed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let capacity = m.get("Capacity").and_then(|v| v.as_u64()).unwrap_or(0);
            let title = if speed > 0 {
                format!("{} {} MHz", manu, speed)
            } else if !manu.is_empty() {
                manu
            } else {
                "内存".to_string()
            };
            let size = if capacity > 0 {
                format!("{:.0} GB", to_gb(capacity))
            } else {
                "未知".to_string()
            };
            memories.push(MemoryInfo { title, size });
        }
    }
    memories
}

/// 读取 Windows 磁盘（CIM）。
#[cfg(target_os = "windows")]
fn read_windows_storage() -> Vec<MemoryInfo> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_DiskDrive | Select-Object Model,Size | ConvertTo-Json -Compress",
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else { return Vec::new() };
    let mut storage = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        let items: Vec<&serde_json::Value> = match json {
            serde_json::Value::Array(a) => a.iter().collect(),
            ref v => std::iter::once(v).collect(),
        };
        for d in items {
            let model = d
                .get("Model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if model.is_empty() {
                continue;
            }
            let size = d.get("Size").and_then(|v| v.as_u64()).unwrap_or(0);
            storage.push(MemoryInfo {
                title: model,
                size: if size > 0 {
                    format!("{:.0} GB", to_gb(size))
                } else {
                    "未知容量".to_string()
                },
            });
        }
    }
    storage
}

/// 读取 Linux 内存（经 dmidecode，可能需要权限；失败则返回空）。
#[cfg(target_os = "linux")]
fn read_linux_memories() -> Vec<MemoryInfo> {
    let out = std::process::Command::new("dmidecode")
        .args(["-t", "memory"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else { return Vec::new() };
    if text.contains("Permission denied") {
        return Vec::new();
    }
    let mut memories = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim().starts_with("Memory Device") {
            let mut size = String::new();
            let mut memtype = String::new();
            let mut speed = String::new();
            for j in i + 1..lines.len() {
                let l = lines[j].trim();
                if l.starts_with("Size:") {
                    size = l.trim_start_matches("Size:").trim().to_string();
                } else if l.starts_with("Type:") {
                    memtype = l.trim_start_matches("Type:").trim().to_string();
                } else if l.starts_with("Configured Memory Speed:") {
                    speed = l
                        .trim_start_matches("Configured Memory Speed:")
                        .trim()
                        .to_string();
                } else if l.is_empty() && j > i + 1 {
                    break;
                }
            }
            if memtype == "Synchronous" || memtype.is_empty() {
                memtype = "内存".to_string();
            }
            let title = if speed.is_empty() {
                memtype
            } else {
                format!("{} {}", memtype, speed)
            };
            if !size.is_empty() && size != "No Module Installed" {
                memories.push(MemoryInfo { title, size });
            }
        }
        i += 1;
    }
    memories
}

/// 读取 Linux 磁盘（lsblk -J）。
#[cfg(target_os = "linux")]
fn read_linux_storage() -> Vec<MemoryInfo> {
    let out = std::process::Command::new("lsblk")
        .args(["-J", "-b", "-o", "NAME,MODEL,SIZE"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else { return Vec::new() };
    let mut storage = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(list) = json.get("blockdevices").and_then(|v| v.as_array()) {
            for dev in list {
                let model = dev
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let name = dev
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if name.starts_with("loop") {
                    continue;
                }
                if model.is_empty() {
                    continue;
                }
                let size = dev.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                storage.push(MemoryInfo {
                    title: model,
                    size: if size > 0 {
                        format!("{:.0} GB", to_gb(size))
                    } else {
                        "未知容量".to_string()
                    },
                });
            }
        }
    }
    storage
}

/// 跨平台读取电池电量（未检测到电池时返回 None）。
fn read_battery() -> Option<BatteryInfo> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("pmset")
            .args(["-g", "batt"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        parse_pmset(&text)
    }
    #[cfg(target_os = "linux")]
    {
        read_linux_battery()
    }
    #[cfg(target_os = "windows")]
    {
        read_windows_battery()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// 解析 `pmset -g batt` 输出，例如 `87%; charging; 2:31 remaining`。
#[cfg(target_os = "macos")]
fn parse_pmset(text: &str) -> Option<BatteryInfo> {
    // 找到类似 "NN%;" 的百分比
    let mut percent: Option<u8> = None;
    for ch in text.chars() {
        if ch == '%' {
            let idx = text.find('%')?;
            let mut num_start = idx;
            while num_start > 0 && text.as_bytes()[num_start - 1].is_ascii_digit() {
                num_start -= 1;
            }
            let digits = &text[num_start..idx];
            percent = digits.parse::<u8>().ok();
            break;
        }
    }
    let percent = percent?;
    // 接通电源（AC）视为"充电中"，使用电池（Battery/discharging）视为"使用中"
    let charging = text.contains("AC attached") && !text.contains("discharging");
    Some(BatteryInfo { percent, charging })
}

/// 读取 Linux 的电池信息（/sys/class/power_supply/BAT*）。
#[cfg(target_os = "linux")]
fn read_linux_battery() -> Option<BatteryInfo> {
    let base = std::path::Path::new("/sys/class/power_supply");
    let mut cap: Option<u8> = None;
    let mut status: Option<String> = None;
    if let Ok(entries) = std::fs::read_dir(base) {
        for e in entries.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("BAT") {
                continue;
            }
            let p = e.path();
            cap = std::fs::read_to_string(p.join("capacity"))
                .ok()
                .and_then(|s| s.trim().parse().ok());
            status = std::fs::read_to_string(p.join("status")).ok();
            if cap.is_some() {
                break;
            }
        }
    }
    let percent = cap?;
    let charging = status
        .map(|s| s.trim().eq_ignore_ascii_case("Charging") || s.trim().eq_ignore_ascii_case("Full"))
        .unwrap_or(false);
    Some(BatteryInfo { percent, charging })
}

/// 读取 Windows 电池信息（WMIC）。
#[cfg(target_os = "windows")]
fn read_windows_battery() -> Option<BatteryInfo> {
    let out = std::process::Command::new("WMIC")
        .args([
            "Path",
            "Win32_Battery",
            "Get",
            "EstimatedChargeRemaining,Status",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut percent: Option<u8> = None;
    let mut charging = false;
    for line in text.lines().skip(1) {
        let mut it = line.split_whitespace();
        // Status 可能为数字或字符串，取后一个字段作为电量
        let mut parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(last) = parts.pop() {
            if let Ok(v) = last.parse::<u8>() {
                percent = Some(v);
            }
        }
        if text.contains("Charging") {
            charging = true;
        }
        // 简化：只取第一行有效数据
        if percent.is_some() {
            break;
        }
    }
    percent.map(|p| BatteryInfo {
        percent: p,
        charging,
    })
}

/// 跨平台读取 GPU 名称与显存信息。
fn read_gpus() -> Vec<GpuInfo> {
    #[cfg(target_os = "macos")]
    {
        read_macos_gpus()
    }
    #[cfg(target_os = "linux")]
    {
        read_linux_gpus()
    }
    #[cfg(target_os = "windows")]
    {
        read_windows_gpus()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// macOS：通过 `system_profiler SPDisplaysDataType -json` 解析 GPU。
#[cfg(target_os = "macos")]
fn read_macos_gpus() -> Vec<GpuInfo> {
    let out = std::process::Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else {
        return Vec::new();
    };
    let mut gpus = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(list) = json.get("SPDisplaysDataType").and_then(|v| v.as_array()) {
            for gpu in list {
                let name = gpu
                    .get("_name")
                    .or_else(|| gpu.get("sppci_model"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知 GPU")
                    .to_string();
                let vram = gpu
                    .get("spdisplays_vram")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                gpus.push(GpuInfo { name, vram });
            }
        }
    }
    gpus
}

/// Linux：通过 `lspci -mm` 过滤显卡。
#[cfg(target_os = "linux")]
fn read_linux_gpus() -> Vec<GpuInfo> {
    let out = std::process::Command::new("lspci")
        .args(["-mm"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else {
        return Vec::new();
    };
    let mut gpus = Vec::new();
    for line in text.lines() {
        if line.to_lowercase().contains("vga") || line.to_lowercase().contains(" 3d ") {
            // 通用格式: "CLASS-ID \"controller\" [chip]"
            let mut parts = line.split('"');
            // parts[1] 是 controller 类型，parts[3] 是具体型号（可能不存在）
            let name = parts
                .nth(3)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let fallback = parts.nth(0).unwrap_or("").trim().to_string();
            let name = name.unwrap_or(fallback);
            if !name.is_empty() {
                gpus.push(GpuInfo {
                    name: name.trim().to_string(),
                    vram: None,
                });
            }
        }
    }
    gpus
}

/// Windows：通过 WMIC 读取显卡名称。
#[cfg(target_os = "windows")]
fn read_windows_gpus() -> Vec<GpuInfo> {
    let out = std::process::Command::new("WMIC")
        .args(["path", "win32_videocontroller", "get", "name"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(text) = out else {
        return Vec::new();
    };
    text.lines()
        .skip(1)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(|name| GpuInfo { name, vram: None })
        .collect()
}
