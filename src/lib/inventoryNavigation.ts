/** 卡片导航只描述工具与原生渠道身份；快照和可执行能力由清单/计划重新读取。 */
export type InventoryTool = "node" | "python";
export type InstallProvider = "mise" | "brew" | "pyenv" | "uv";

export interface InventoryInstallTarget {
  toolId: InventoryTool;
  providerId: InstallProvider;
  packageId: string;
}

export interface InventoryInstallRequest {
  requestId: number;
  providerId: InstallProvider;
  packageId: string;
  /** 跨面板导航前记录的触发按钮，关闭计划时用于归还焦点。 */
  opener?: HTMLElement | null;
}
