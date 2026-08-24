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
export function installVersionStreaming(
  tool: string,
  version: string
): Promise<string> {
  return invoke("install_version_streaming", { tool, version });
}

/** 订阅安装进度事件，返回取消监听的函数 */
export function onInstallProgress(
  cb: (payload: InstallProgressPayload) => void
): Promise<UnlistenFn> {
  return listen<InstallProgressPayload>("mise:install-progress", (e) =>
    cb(e.payload)
  );
}

/** 卸载指定版本 */
export function uninstallVersion(
  tool: string,
  version: string
): Promise<string> {
  return invoke("uninstall_version", { tool, version });
}

/** 切换版本（global 控制是否写入全局配置） */
export function useVersion(
  tool: string,
  version: string,
  global: boolean
): Promise<string> {
  return invoke("use_version", { tool, version, global });
}

/** 接管：把已存在的外部环境目录链接为 mise 版本（不重新下载） */
export function linkVersion(
  tool: string,
  version: string,
  path: string
): Promise<string> {
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
export function writeProjectConfig(
  path: string,
  content: string
): Promise<string> {
  return invoke("write_project_config", { path, content });
}

/** 写入配置并在项目内一键安装全套环境 */
export function installAllProject(
  path: string,
  content: string
): Promise<string> {
  return invoke("install_all_project", { path, content });
}

/** 通用错误信息提取 */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}