import type { InventorySnapshot } from "../api/inventory";
import type { CatalogTool } from "../api/toolCatalog";
export interface CatalogReadState {
  snapshot: InventorySnapshot | null;
  loading: boolean;
  error: string | null;
}
export interface CatalogEvidence {
  state: "observed" | "notObserved" | "unknown";
  count: number;
  reason: string;
}
/** 目录定义不是机器状态。只有已接入工具自身的来源覆盖可以支持不存在的结论。 */
export function catalogEvidence(tool: CatalogTool, read: CatalogReadState): CatalogEvidence {
  const channels = tool.channels.filter((c) => c.inventory.state === "integrated");
  if (!channels.length)
    return { state: "unknown", count: 0, reason: "尚未接入统一来源，安装状态未知" };
  const id = tool.definition.toolId;
  const matching =
    read.snapshot?.installations.filter(
      (i) => i.toolId === id && i.providerPackage.providerId !== "path",
    ) ?? [];
  const fresh = matching.filter(
    (i) =>
      !i.stale &&
      channels.some(
        (c) =>
          c.providerId === i.providerPackage.providerId &&
          c.packages.some((p) => p.platform === i.providerPackage.platform),
      ) &&
      read.snapshot?.providers.some(
        (p) =>
          p.providerId === i.providerPackage.providerId &&
          !p.stale &&
          p.coverage.toolIds.includes(id) &&
          (i.health === "healthy" ||
            (p.status === "ok" && !p.error && p.coverage.complete && !p.coverage.truncated)),
      ),
  );
  if (read.loading || read.error || !read.snapshot)
    return {
      state: "unknown",
      count: matching.length,
      reason: matching.length ? "保留上次实例记录，当前状态待刷新" : "尚无当前读取结果",
    };
  if (fresh.length)
    return {
      state: "observed",
      count: fresh.length,
      reason: channels.every((c) =>
        read.snapshot?.providers.some(
          (p) =>
            p.providerId === c.providerId &&
            p.status === "ok" &&
            !p.error &&
            !p.stale &&
            p.coverage.complete &&
            !p.coverage.truncated &&
            p.coverage.toolIds.includes(id),
        ),
      )
        ? "已观察安装目录；健康、归属和可用操作请查看实例证据"
        : "存在本次已验证实例；部分来源未完整读取，不能推断其余安装或操作权限",
    };
  const complete =
    matching.length === 0 &&
    channels.every((c) =>
      read.snapshot?.providers.some(
        (p) =>
          p.providerId === c.providerId &&
          p.status === "ok" &&
          !p.stale &&
          !p.error &&
          p.coverage.complete &&
          !p.coverage.truncated &&
          p.coverage.toolIds.includes(id),
      ),
    );
  return complete
    ? {
        state: "notObserved",
        count: 0,
        reason: "已接入来源完整读取，未观察到安装目录；不代表系统中绝对不存在",
      }
    : {
        state: "unknown",
        count: matching.length,
        reason: "来源不完整或记录已过期，不能据此断言未安装",
      };
}
