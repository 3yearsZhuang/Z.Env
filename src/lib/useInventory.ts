import { useCallback, useEffect, useRef, useState } from "react";
import type { InventorySnapshot, ProviderId } from "../api/inventory";
import { inventoryCancel, inventorySnapshot } from "../api/inventoryQuery";

export interface InventoryState {
  snapshot: InventorySnapshot | null;
  loading: boolean;
  /** 等待旧扫描退出；接续扫描尚未进入后端。 */
  waiting: boolean;
  cancelling: boolean;
  error: string | null;
  refresh: (providerId?: ProviderId) => Promise<void>;
  cancel: () => Promise<void>;
}

interface Request {
  id: string;
  generation: number;
  started: boolean;
  cancelInFlight: boolean;
  cancelAccepted: boolean;
}

let fallbackSequence = 0;

function requestId(): string {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  fallbackSequence += 1;
  return `inventory-${Date.now().toString(36)}-${fallbackSequence.toString(36)}`;
}

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

function staleSnapshot(snapshot: InventorySnapshot | null): InventorySnapshot | null {
  if (!snapshot) return null;
  return {
    ...snapshot,
    providers: snapshot.providers.map((report) => ({ ...report, stale: true })),
    installations: snapshot.installations.map((installation) => ({ ...installation, stale: true })),
    aliases: [],
  };
}

/** 仅在页签激活时扫描一次；异步回复只属于创建它的激活周期。 */
export function useInventory(active: boolean, toolId: "node" | "python" = "node"): InventoryState {
  const [snapshot, setSnapshot] = useState<InventorySnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [waiting, setWaiting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const enabled = useRef(false);
  const generation = useRef(0);
  const current = useRef<Request | null>(null);
  const inFlight = useRef<Promise<InventorySnapshot> | null>(null);

  const isCurrent = useCallback(
    (request: Request) =>
      enabled.current && current.current === request && generation.current === request.generation,
    [],
  );

  const refresh = useCallback(
    async (providerId?: ProviderId) => {
      // 等待之前就占用当前请求，连续点击不能创建重叠扫描。
      if (!enabled.current || current.current) return;
      const request: Request = {
        id: requestId(),
        generation: generation.current,
        started: false,
        cancelInFlight: false,
        cancelAccepted: false,
      };
      current.current = request;
      const previous = inFlight.current;
      setLoading(true);
      setWaiting(Boolean(previous));
      setCancelling(false);
      setError(null);
      try {
        if (previous) {
          try {
            // 后端在旧扫描实际退出后才释放槽；等待原 Promise，无需轮询重试。
            await previous;
          } catch {
            // 旧扫描的错误属于旧激活周期，不影响接续扫描。
          }
        }
        if (!isCurrent(request)) return;
        setWaiting(false);
        request.started = true;
        const query =
          toolId === "node"
            ? inventorySnapshot(request.id, providerId)
            : inventorySnapshot(request.id, providerId, toolId);
        inFlight.current = query;
        let next: InventorySnapshot;
        try {
          next = await query;
        } finally {
          if (inFlight.current === query) inFlight.current = null;
        }
        if (!isCurrent(request)) return;
        if (request.cancelAccepted) throw new Error("工具清单刷新已取消，迟到结果不再适用");
        if (next.requestId !== request.id) throw new Error("清单响应与当前扫描不匹配，请重试。");
        if (
          toolId === "python" &&
          (next.installations.some((i) => i.toolId !== "python") ||
            !next.providers.length ||
            next.providers.some((p) => !p.coverage.toolIds.includes("python")))
        )
          throw new Error("清单工具范围与 Python 请求不匹配");
        setSnapshot(next);
        setError(null);
      } catch (reason) {
        if (!isCurrent(request)) return;
        setSnapshot(staleSnapshot);
        setError(errorMessage(reason));
      } finally {
        if (isCurrent(request)) {
          current.current = null;
          setLoading(false);
          setWaiting(false);
          setCancelling(false);
        }
      }
    },
    [isCurrent, toolId],
  );

  const cancel = useCallback(async () => {
    const request = current.current;
    if (!request || !isCurrent(request) || request.cancelInFlight || request.cancelAccepted) return;
    if (!request.started) {
      // 接续扫描尚未进入后端，直接结束本次等待；旧扫描仍按原有取消请求退出。
      current.current = null;
      setLoading(false);
      setWaiting(false);
      setCancelling(false);
      setSnapshot(staleSnapshot);
      setError("工具清单刷新已取消");
      return;
    }
    request.cancelInFlight = true;
    setCancelling(true);
    setError(null);
    try {
      const accepted = await inventoryCancel(request.id);
      if (!isCurrent(request)) return;
      request.cancelInFlight = false;
      request.cancelAccepted = accepted;
      if (!accepted) {
        setCancelling(false);
        setError("取消请求未被接受，请等待扫描结果或重试。");
      }
    } catch (reason) {
      if (!isCurrent(request)) return;
      request.cancelInFlight = false;
      setCancelling(false);
      setError(errorMessage(reason));
    }
  }, [isCurrent]);

  useEffect(() => {
    enabled.current = active;
    if (active) {
      void refresh();
    } else {
      setLoading(false);
      setWaiting(false);
      setCancelling(false);
    }
    return () => {
      enabled.current = false;
      generation.current += 1;
      const request = current.current;
      current.current = null;
      if (request?.started && !request.cancelInFlight && !request.cancelAccepted) {
        void inventoryCancel(request.id).catch(() => {
          // 页面已经切走，迟到的取消错误不能修改下一次激活的状态。
        });
      }
    };
  }, [active, refresh]);

  return { snapshot, loading, waiting, cancelling, error, refresh, cancel };
}
