import { invoke } from "@tauri-apps/api/core";
import type { InventorySnapshot, ProviderId } from "./inventory";
import { inventoryRead } from "./inventoryRead";

/** 只读扫描；省略来源时刷新全部来源，指定来源时保留其他来源的缓存。 */
export function inventorySnapshot(
  requestId: string,
  providerId?: ProviderId,
  toolId?: "node" | "python",
): Promise<InventorySnapshot> {
  return inventoryRead(() =>
    invoke("inventory_snapshot", {
      requestId,
      providerId: providerId ?? null,
      ...(toolId && toolId !== "node" ? { toolId } : {}),
    }),
  );
}

/** 请求取消对应扫描；最终结果仍由 inventorySnapshot 的 Promise 返回。 */
export function inventoryCancel(requestId: string): Promise<boolean> {
  return invoke("inventory_cancel", { requestId });
}
