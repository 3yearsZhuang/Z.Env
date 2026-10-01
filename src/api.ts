import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

// ---------- 与 Rust 后端数据结构对应的类型 ----------

/** 单个已安装工具版本 */
export interface ToolVersion {
  version: string;
  requested_version?: string | null;
  install_path?: string | null;
  installed?: boolean | null;
  active?: boolean | null;
}

/** 聚合后的工具信息 */
export interface ToolInfo {
  name: string;
  versions: ToolVersion[];
  active_versions: string[];
}

/** 系统资源占用快照 */
export interface SystemStats {
  cpuUsage: number;
  cpuCores: number;
  cpuBrand: string | null;
  memoryTotalGb: number;
  memoryUsedGb: number;
  memoryPercent: number;
  memories: { title: string; size: string }[];
  storage: { title: string; size: string }[];
  disks: {
    name: string;
    model: string | null;
    totalGb: number;
    availableGb: number;
    percent: number;
  }[];
  battery: { percent: number; charging: boolean } | null;
  gpus: { name: string; vram: string | null }[];
  interfaces: NetInterface[];
  osName: string | null;
  hostName: string | null;
}

/** 单张网卡的实时速率与累计流量 */
export interface NetInterface {
  name: string;
  rxRate: number;
  txRate: number;
  totalRx: number;
  totalTx: number;
}

/** 获取一次系统资源快照 */
export function systemStats(): Promise<SystemStats> {
  return invoke("system_stats");
}

/** mise 安装状态 */
export interface MiseStatus {
  installed: boolean;
  version: string | null;
}

/** 检测 mise 安装状态与版本 */
export function checkMise(): Promise<MiseStatus> {
  return invoke("check_mise");
}

/** 操作系统信息 */
export interface OsInfo {
  name: string;
  version: string;
  arch: string;
}

/** 包管理器信息 */
export interface PkgInfo {
  name: string;
  version: string | null;
  available: boolean;
}

/** 一次性环境汇总信息 */
export interface EnvInfo {
  mise_version: string | null;
  git_version: string | null;
  os: OsInfo;
  pkg: PkgInfo[];
}

/** 采集环境信息：mise/git 版本、系统版本、包管理器版本 */
export function getEnvInfo(): Promise<EnvInfo> {
  return invoke("get_env_info");
}

/** 其他工具托管的一个运行时版本 */
export interface ToolSource {
  tool: string;
  version: string;
  manager: string;
  path: string;
}

/** 扫描 nvm/pyenv/asdf/sdkman/rvm 等托管的运行时 */
export function detectToolSources(): Promise<ToolSource[]> {
  return invoke("detect_tool_sources");
}

// ---------- Rust 命令封装 ----------

/** 列出所有已安装工具 */
export function listTools(): Promise<ToolInfo[]> {
  return invoke("list_tools");
}

/** 查看某个工具的远程可安装版本 */
export function listRemoteVersions(tool: string): Promise<string[]> {
  return invoke("list_remote_versions", { tool });
}

/** 通过 ASDF 源获取远程版本 */
export function listRemoteVersionsAsdf(tool: string): Promise<string[]> {
  return invoke("list_remote_versions_asdf", { tool });
}

/** 通过 GitHub Releases Tags 获取远程版本 */
export function listRemoteVersionsGithub(tool: string): Promise<string[]> {
  return invoke("list_remote_versions_github", { tool });
}

/** 通过“官方生态源”（node/go/python/java 官方）获取远程版本 */
export function listRemoteVersionsOfficial(tool: string): Promise<string[]> {
  return invoke("list_remote_versions_official", { tool });
}

/** 列出 mise 支持的全部运行时名（用于自检覆盖） */
export function listRegistry(): Promise<string[]> {
  return invoke("list_registry");
}

/** 安装指定版本 */
export function installVersion(tool: string, version: string): Promise<string> {
  return invoke("install_version", { tool, version });
}

/** 安装进度事件负载 */
export interface InstallProgressPayload {
  tool: string;
  version: string;
  line: string;
}

/** 以流式方式安装，安装期间通过事件实时推送进度 */
export function installVersionStreaming(tool: string, version: string): Promise<string> {
  return invoke("install_version_streaming", { tool, version });
}

/** 订阅安装进度事件，返回取消监听的函数 */
export function onInstallProgress(
  cb: (payload: InstallProgressPayload) => void,
): Promise<UnlistenFn> {
  return listen<InstallProgressPayload>("mise:install-progress", (e) => cb(e.payload));
}

/** 卸载指定版本 */
export function uninstallVersion(tool: string, version: string): Promise<string> {
  return invoke("uninstall_version", { tool, version });
}

/** 切换版本（global 控制是否写入全局配置） */
export function useVersion(tool: string, version: string, global: boolean): Promise<string> {
  return invoke("use_version", { tool, version, global });
}

/** 接管：把已存在的外部环境目录链接为 mise 版本（不重新下载） */
export function linkVersion(tool: string, version: string, path: string): Promise<string> {
  return invoke("link_version", { tool, version, path });
}

/** 解除接管：移除某版本与外部目录的链接 */
export function unlinkVersion(tool: string, version: string): Promise<string> {
  return invoke("unlink_version", { tool, version });
}

/** 读取项目配置文件内容 */
export function readProjectConfig(path: string): Promise<string> {
  return invoke("read_project_config", { path });
}

/** 写入项目配置 */
export function writeProjectConfig(path: string, content: string): Promise<string> {
  return invoke("write_project_config", { path, content });
}

/** 写入配置并在项目内一键安装全套环境 */
export function installAllProject(path: string, content: string): Promise<string> {
  return invoke("install_all_project", { path, content });
}

/** 通过系统包管理器（brew/winget/apt/pacman）原生安装指定软件 */
export function installSystemPackage(manager: string, name: string): Promise<string> {
  return invoke("install_system_package", { manager, name });
}

/** 探测指定系统包管理器当前已安装的软件名列表 */
export function detectSystemInstalled(manager: string): Promise<string[]> {
  return invoke("detect_system_installed", { manager });
}

/** 系统包管理器已装的一个软件及其版本 */
export interface SystemPkg {
  name: string;
  version: string | null;
}

/** 探测指定系统包管理器当前已装的软件及其版本 */
export function detectSystemVersions(manager: string): Promise<SystemPkg[]> {
  return invoke("detect_system_versions", { manager });
}

/** 原生卸载系统软件包（brew/winget/apt/pacman） */
export function uninstallSystemPackage(manager: string, name: string): Promise<string> {
  return invoke("uninstall_system_package", { manager, name });
}

/** 原生安装进度事件负载 */
export interface SysInstallProgressPayload {
  manager: string;
  name: string;
  line: string;
}

/** 订阅系统包管理器安装进度事件，返回取消监听的函数 */
export function onSysInstallProgress(
  cb: (payload: SysInstallProgressPayload) => void,
): Promise<UnlistenFn> {
  return listen<SysInstallProgressPayload>("sys:install-progress", (e) => cb(e.payload));
}

// ---------- 托管接入（整机运行时 → mise） ----------

/** 托管清单条目（含健康状态） */
export interface ManagedEntry {
  tool: string;
  version: string;
  manager: string;
  target: string;
  healthy: boolean;
}

/** 对账事件：brew 升级/卸载后的自动重连或移除 */
export interface ReconcileEvent {
  tool: string;
  from: string;
  to: string | null;
  action: "relinked" | "removed" | "failed";
  message: string;
}

/** 接入 mise：按来源自动选策略（用户级目录直连 / brew 托管软链），带注册校验与回滚 */
export function managedAdopt(
  tool: string,
  version: string,
  manager: string,
  path: string,
): Promise<string> {
  return invoke("managed_adopt", { tool, version, manager, path });
}

/** 解除接入 */
export function managedUnadopt(tool: string, version: string): Promise<string> {
  return invoke("managed_unadopt", { tool, version });
}

/** 对账：失效接入自动重连或移除，返回事件列表 */
export function managedReconcile(): Promise<ReconcileEvent[]> {
  return invoke("managed_reconcile");
}

/** 托管清单与健康状态 */
export function managedList(): Promise<ManagedEntry[]> {
  return invoke("managed_list");
}

/** 策略 C：包管理器自维护的稳定 bin 路径（写入项目 mise.toml 的 env._path） */
export function stableBinPath(manager: string, name: string, path: string): Promise<string> {
  return invoke("stable_bin_path", { manager, name, path });
}

// ---------- 环境体检（doctor） ----------

/** 单项体检结果：level 为结论等级，detail 描述现状，hint 给出修复建议；fixable 表示可一键修复 */
export interface DoctorCheck {
  id: string;
  title: string;
  level: "ok" | "warn" | "fail";
  detail: string;
  hint: string;
  fixable: boolean;
}

/** 环境体检：全量巡检整机环境健康度（只读，不做修复动作） */
export function doctorRun(): Promise<DoctorCheck[]> {
  return invoke("doctor_run");
}

/** 环境体检修复：执行一个检查项的修复动作（托管对账 / shell 集成追加） */
export function doctorFix(id: string): Promise<string> {
  return invoke("doctor_fix", { id });
}

// ---------- 环境变量中心 ----------

/** 全局 [env] 单条：kind = simple（标量，可编辑）| complex（表/数组等高级值，只读） */
export interface GlobalEnvEntry {
  key: string;
  value: string;
  kind: string;
}

/** 同名冲突：系统 env 与全局 mise env 同键不同值 */
export interface EnvConflict {
  key: string;
  miseValue: string;
  systemValue: string;
}

/** 系统/用户级 env 单条（只读展示） */
export interface SystemEnvVar {
  key: string;
  value: string;
}

/** 环境变量中心快照 */
export interface EnvCenterSnapshot {
  configPath: string;
  exists: boolean;
  entries: GlobalEnvEntry[];
  conflicts: EnvConflict[];
  systemEnv: SystemEnvVar[];
}

/** 读取环境变量中心快照 */
export function envCenterList(): Promise<EnvCenterSnapshot> {
  return invoke("env_center_list");
}

/** 设置一个全局 env（写入全局 mise config 的 [env] 段） */
export function envCenterSet(key: string, value: string): Promise<string> {
  return invoke("env_center_set", { key, value });
}

/** 移除一个全局 env */
export function envCenterRemove(key: string): Promise<string> {
  return invoke("env_center_remove", { key });
}

// ---------- 项目发现 ----------

/** 发现的项目：tools 为指纹/mise.toml 推断，missingTools 为其中本机未安装的 */
export interface DiscoveredProject {
  name: string;
  path: string;
  hasMiseToml: boolean;
  tools: string[];
  missingTools: string[];
}

/** 项目发现：扫描常用目录（只读） */
export function discoverProjects(): Promise<DiscoveredProject[]> {
  return invoke("discover_projects");
}

// ---------- 本地服务 ----------

/** 单个服务：state 为 started/stopped/error 或 systemd 的 enabled/disabled 等 */
export interface ServiceInfo {
  name: string;
  state: string;
  /** 管理来源：brew | systemd */
  manager: string;
  pid: number | null;
}

/** 服务总览：列表 + 平台说明 */
export interface ServiceOverview {
  services: ServiceInfo[];
  notes: string[];
}

/** 本地服务列表（brew services / systemd 用户级） */
export function serviceList(): Promise<ServiceOverview> {
  return invoke("service_list");
}

/** 对本地服务执行操作（start / stop / restart） */
export function serviceAction(manager: string, name: string, act: string): Promise<string> {
  return invoke("service_action", { manager, name, act });
}

// ---------- 开发缓存 ----------

/** 单个缓存项：approx 表示体积因预算截断为下限值 */
export interface CacheInfo {
  id: string;
  name: string;
  path: string;
  exists: boolean;
  sizeBytes: number;
  approx: boolean;
  cleanable: boolean;
}

/** 开发缓存列表（探测到哪个列哪个） */
export function cacheList(): Promise<CacheInfo[]> {
  return invoke("cache_list");
}

/** 用官方命令清理指定开发缓存 */
export function cacheClean(id: string): Promise<string> {
  return invoke("cache_clean", { id });
}

// ---------- 操作历史 ----------

/** 单条操作记录：time 为 unix 秒，kind 为操作类别（前端映射中文标签） */
export interface HistoryEntry {
  time: number;
  kind: string;
  detail: string;
}

/** 操作历史：最近 200 条管理动作（新 → 旧） */
export function historyList(): Promise<HistoryEntry[]> {
  return invoke("history_list");
}

// ---------- 环境快照 ----------

/** 快照导出摘要 */
export interface SnapshotSummary {
  tools: number;
  envVars: number;
  brewPackages: number;
  path: string;
}

/** 导出环境快照（mise 工具 + 全局 env + brew 清单）为 TOML */
export function snapshotExport(path: string): Promise<SnapshotSummary> {
  return invoke("snapshot_export", { path });
}

/** 从快照重建：写入全局 env + 逐个安装 mise 工具 + 生成 Brewfile（brew 不自动执行） */
export function snapshotRestore(path: string): Promise<string> {
  return invoke("snapshot_restore", { path });
}

// ---------- 环境预设（可分享的编程环境配置） ----------

/** 预设中的一个工具与版本请求（版本可为 20 / latest / temurin-21 等 mise 请求式） */
export interface PresetTool {
  name: string;
  version: string;
}

/** 导入的环境预设文件内容 */
export interface PresetFile {
  name: string;
  description: string | null;
  tools: PresetTool[];
  path: string;
}

/** 导出环境预设为可分享文件（Z.Env 预设 TOML v1），返回摘要 */
export function presetExport(
  path: string,
  name: string,
  description: string,
  toml: string,
): Promise<string> {
  return invoke("preset_export", { path, name, description, toml });
}

/** 导入环境预设文件（Z.Env 预设 TOML v1，兼容纯 mise.toml） */
export function presetImport(path: string): Promise<PresetFile> {
  return invoke("preset_import", { path });
}

/** 通用错误信息提取 */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
