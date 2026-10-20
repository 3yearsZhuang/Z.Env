import { useCallback, useEffect, useRef, useState } from "react";
import { inventoryCancel } from "../api/inventoryQuery";
import { projectEnvironment, type ProjectEnvironment } from "../api/projectEnvironment";
import { useInventory } from "./useInventory";
let sequence = 0;
/** 项目核对绑定到磁盘路径和清单身份；换项目、切页或取消后，迟到响应不得恢复旧的成功状态。 */
export function useProjectEnvironment(path: string, active: boolean, refreshKey = 0) {
  const inventory = useInventory(active && Boolean(path));
  const [report, setReport] = useState<{ path: string; value: ProjectEnvironment } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const epoch = useRef(0);
  const current = useRef<{
    id: string;
    epoch: number;
    started: boolean;
    cancelling: boolean;
    cancelAccepted: boolean;
  } | null>(null);
  const flight = useRef<Promise<ProjectEnvironment> | null>(null);
  const enabled = useRef(false);
  const snapshotId = inventory.snapshot?.requestId;
  const run = useCallback(
    async (probe: boolean) => {
      if (!enabled.current || !snapshotId || !path || current.current) return;
      const request = {
        id: `project-${Date.now().toString(36)}-${++sequence}`,
        epoch: epoch.current,
        started: false,
        cancelling: false,
        cancelAccepted: false,
      };
      current.current = request;
      setLoading(true);
      setError(null);
      setCancelling(false);
      const valid = () =>
        enabled.current && current.current === request && epoch.current === request.epoch;
      try {
        if (flight.current) await flight.current.catch(() => undefined);
        if (!valid()) return;
        request.started = true;
        const promise = projectEnvironment({
          requestId: request.id,
          snapshotRequestId: snapshotId,
          path,
          probe,
        });
        flight.current = promise;
        let next: ProjectEnvironment;
        try {
          next = await promise;
        } finally {
          if (flight.current === promise) flight.current = null;
        }
        if (!valid()) return;
        if (request.cancelAccepted) throw new Error("项目核对已取消，旧的成功结果不再适用");
        if (next.requestId !== request.id || next.snapshotRequestId !== snapshotId)
          throw new Error("项目响应与当前核对身份不一致，请重试");
        setError(null);
        setReport({ path, value: next });
      } catch (reason) {
        if (valid()) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        if (valid()) {
          current.current = null;
          setLoading(false);
          setCancelling(false);
        }
      }
    },
    [path, snapshotId],
  );
  useEffect(() => {
    enabled.current = active && Boolean(path);
    if (enabled.current && snapshotId && !inventory.loading && !inventory.error) void run(false);
    return () => {
      enabled.current = false;
      epoch.current++;
      const request = current.current;
      current.current = null;
      setLoading(false);
      setCancelling(false);
      if (request?.started && !request.cancelling)
        void inventoryCancel(request.id).catch(() => undefined);
    };
  }, [active, path, snapshotId, inventory.loading, inventory.error, run]);
  const lastRefresh = useRef(refreshKey);
  useEffect(() => {
    if (lastRefresh.current !== refreshKey) {
      lastRefresh.current = refreshKey;
      if (active) void inventory.refresh();
    }
  }, [refreshKey, active, inventory.refresh]);
  async function cancel() {
    const request = current.current;
    if (!request) {
      await inventory.cancel();
      return;
    }
    if (request.cancelling) return;
    request.cancelling = true;
    setCancelling(true);
    if (!request.started) {
      current.current = null;
      setLoading(false);
      setCancelling(false);
      setError("项目核对已取消");
      return;
    }
    try {
      const accepted = await inventoryCancel(request.id);
      if (current.current !== request) return;
      request.cancelAccepted = accepted;
      if (!accepted) {
        request.cancelling = false;
        setCancelling(false);
        setError("取消请求未被接受，仍在等待原核对结果");
      }
    } catch (reason) {
      if (current.current === request) {
        request.cancelling = false;
        setCancelling(false);
        setError(String(reason));
      }
    }
  }
  const stale =
    !active ||
    !report ||
    report.path !== path ||
    report.value.snapshotRequestId !== snapshotId ||
    loading ||
    inventory.loading ||
    Boolean(error || inventory.error);
  return {
    report: report?.value ?? null,
    snapshotId,
    loading: loading || inventory.loading,
    cancelling: cancelling || inventory.cancelling,
    error: error ?? inventory.error,
    stale,
    probe: () => run(true),
    refresh: inventory.refresh,
    cancel,
  };
}
