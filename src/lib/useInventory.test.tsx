// @vitest-environment jsdom
import { StrictMode } from "react";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixture from "../../fixtures/inventory/contract-v1.json";
import type { InventorySnapshot } from "../api/inventory";
import { inventoryCancel, inventorySnapshot } from "../api/inventoryQuery";
import { useInventory } from "./useInventory";

vi.mock("../api/inventoryQuery", () => ({
  inventorySnapshot: vi.fn(),
  inventoryCancel: vi.fn(),
}));

function snapshot(requestId: string): InventorySnapshot {
  return { ...(fixture.expectedSnapshot as InventorySnapshot), requestId };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function singleSlotBackend() {
  type Scan = { id: string; result: ReturnType<typeof deferred<InventorySnapshot>> };
  const scans: Scan[] = [];
  let active: Scan | null = null;
  vi.mocked(inventorySnapshot).mockImplementation((id) => {
    if (active) return Promise.reject(new Error("已有扫描正在进行"));
    const scan = { id, result: deferred<InventorySnapshot>() };
    active = scan;
    scans.push(scan);
    return scan.result.promise.finally(() => {
      if (active === scan) active = null;
    });
  });
  // 接受取消只发出信号；由测试模拟工作线程稍后退出，之后才能释放槽。
  vi.mocked(inventoryCancel).mockImplementation(async (id) => active?.id === id);
  return scans;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(inventorySnapshot).mockImplementation(async (id) => snapshot(id));
  vi.mocked(inventoryCancel).mockResolvedValue(true);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("只读工具清单会话", () => {
  it("首次进入扫描所有来源，mise 不可用仍保留其他来源的安装", async () => {
    vi.mocked(inventorySnapshot).mockImplementation(async (id) => {
      const next = snapshot(id);
      return {
        ...next,
        providers: next.providers.map((report) =>
          report.providerId === "mise" ? { ...report, status: "unavailable", error: null } : report,
        ),
      };
    });
    const { result } = renderHook(() => useInventory(true));
    expect(result.current.loading).toBe(true);
    expect(result.current.waiting).toBe(false);
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(inventorySnapshot).toHaveBeenCalledExactlyOnceWith(expect.any(String), undefined);
    expect(result.current.snapshot?.providers[0].status).toBe("unavailable");
    expect(result.current.snapshot?.installations).toHaveLength(4);
    expect(result.current.error).toBeNull();
  });

  it("全量刷新失败保留旧快照并将每个来源和安装标为过期", async () => {
    const { result } = renderHook(() => useInventory(true));
    await waitFor(() => expect(result.current.loading).toBe(false));
    const previous = result.current.snapshot!;
    vi.mocked(inventorySnapshot).mockRejectedValueOnce(new Error("后端连接断开"));
    await act(() => result.current.refresh());
    expect(result.current.snapshot?.requestId).toBe(previous.requestId);
    expect(result.current.snapshot?.installations.map((item) => item.installationId)).toEqual(
      previous.installations.map((item) => item.installationId),
    );
    expect(result.current.snapshot?.providers.every((item) => item.stale)).toBe(true);
    expect(result.current.snapshot?.installations.every((item) => item.stale)).toBe(true);
    expect(result.current.snapshot?.aliases).toEqual([]);
    expect(previous.aliases).toHaveLength(1);
    expect(previous.providers.every((item) => !item.stale)).toBe(true);
    expect(previous.installations.every((item) => !item.stale)).toBe(true);
    expect(result.current.error).toBe("后端连接断开");
    expect(result.current.loading).toBe(false);
    expect(result.current.waiting).toBe(false);
  });

  it("来源重试传递来源身份，同步连续点击不会创建并发请求", async () => {
    const { result } = renderHook(() => useInventory(true));
    await waitFor(() => expect(result.current.loading).toBe(false));
    const pending = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(pending.promise);
    let refreshing!: Promise<void>;
    act(() => {
      refreshing = result.current.refresh("brew");
      void result.current.refresh("nvm");
      void result.current.refresh();
    });
    expect(inventorySnapshot).toHaveBeenCalledTimes(2);
    const id = vi.mocked(inventorySnapshot).mock.calls[1][0];
    expect(inventorySnapshot).toHaveBeenLastCalledWith(id, "brew");
    expect(result.current.loading).toBe(true);
    await act(async () => {
      pending.resolve(snapshot(id));
      await refreshing;
    });
    expect(result.current.snapshot?.requestId).toBe(id);
    expect(result.current.loading).toBe(false);
  });

  it("取消确认后等待扫描终止，重复取消不会重发或提前解锁刷新", async () => {
    const pending = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(pending.promise);
    const cancelReply = deferred<boolean>();
    vi.mocked(inventoryCancel).mockReturnValueOnce(cancelReply.promise);
    const { result } = renderHook(() => useInventory(true));
    const id = vi.mocked(inventorySnapshot).mock.calls[0][0];
    let cancelling!: Promise<void>;
    act(() => {
      cancelling = result.current.cancel();
      void result.current.cancel();
    });
    expect(result.current.cancelling).toBe(true);
    expect(inventoryCancel).toHaveBeenCalledExactlyOnceWith(id);
    await act(async () => {
      cancelReply.resolve(true);
      await cancelling;
      await result.current.cancel();
      await result.current.refresh();
    });
    expect(result.current.cancelling).toBe(true);
    expect(result.current.loading).toBe(true);
    expect(inventoryCancel).toHaveBeenCalledTimes(1);
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    await act(async () => {
      pending.reject("工具清单刷新已取消");
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.cancelling).toBe(false);
    expect(result.current.error).toBe("工具清单刷新已取消");
  });

  it("取消失败显示错误，继续等待原扫描并允许重试取消", async () => {
    const pending = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(pending.promise);
    vi.mocked(inventoryCancel).mockRejectedValueOnce(new Error("取消通道不可用"));
    const { result } = renderHook(() => useInventory(true));
    await act(() => result.current.cancel());
    expect(result.current.cancelling).toBe(false);
    expect(result.current.loading).toBe(true);
    expect(result.current.error).toBe("取消通道不可用");
    await act(() => result.current.cancel());
    expect(inventoryCancel).toHaveBeenCalledTimes(2);
    expect(result.current.cancelling).toBe(true);
    expect(result.current.error).toBeNull();
    await act(async () => pending.reject("工具清单刷新已取消"));
    expect(result.current.loading).toBe(false);
  });

  it("取消未被接受时继续等待结果并恢复取消入口", async () => {
    const pending = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(pending.promise);
    vi.mocked(inventoryCancel).mockResolvedValueOnce(false);
    const { result } = renderHook(() => useInventory(true));
    const id = vi.mocked(inventorySnapshot).mock.calls[0][0];
    await act(() => result.current.cancel());
    expect(result.current.loading).toBe(true);
    expect(result.current.cancelling).toBe(false);
    expect(result.current.error).toContain("未被接受");
    await act(async () => pending.resolve(snapshot(id)));
    expect(result.current.snapshot?.requestId).toBe(id);
    expect(result.current.loading).toBe(false);
    expect(result.current.cancelling).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("扫描先完成时，迟到的取消错误不能污染已完成结果", async () => {
    const pending = deferred<InventorySnapshot>();
    const cancelReply = deferred<boolean>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(pending.promise);
    vi.mocked(inventoryCancel).mockReturnValueOnce(cancelReply.promise);
    const { result } = renderHook(() => useInventory(true));
    const id = vi.mocked(inventorySnapshot).mock.calls[0][0];
    act(() => void result.current.cancel());
    await act(async () => pending.resolve(snapshot(id)));
    await act(async () => cancelReply.reject(new Error("过期取消错误")));
    expect(result.current.snapshot?.requestId).toBe(id);
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(result.current.cancelling).toBe(false);
  });

  it("连续切换等待旧线程释放单槽，只为最新激活周期接续扫描", async () => {
    const scans = singleSlotBackend();
    const { result, rerender } = renderHook(({ active }) => useInventory(active), {
      initialProps: { active: true },
    });
    const oldId = scans[0].id;
    rerender({ active: false });
    expect(inventoryCancel).toHaveBeenCalledExactlyOnceWith(oldId);
    expect(result.current.loading).toBe(false);
    expect(result.current.waiting).toBe(false);
    await act(() => result.current.refresh());
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    rerender({ active: true });
    rerender({ active: false });
    rerender({ active: true });
    await act(() => result.current.refresh());
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    expect(inventoryCancel).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(true);
    expect(result.current.waiting).toBe(true);
    await act(async () => scans[0].result.resolve({ ...snapshot(oldId), observedAt: 100 }));
    expect(result.current.snapshot).toBeNull();
    expect(inventorySnapshot).toHaveBeenCalledTimes(2);
    expect(scans).toHaveLength(2);
    expect(result.current.waiting).toBe(false);
    const nextId = scans[1].id;
    expect(nextId).not.toBe(oldId);
    await act(async () => scans[1].result.resolve({ ...snapshot(nextId), observedAt: 200 }));
    expect(result.current.snapshot?.requestId).toBe(nextId);
    expect(result.current.snapshot?.observedAt).toBe(200);
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(result.current.waiting).toBe(false);
  });

  it("旧请求的错误和取消回复不能修改重新激活中的会话", async () => {
    const old = deferred<InventorySnapshot>();
    const next = deferred<InventorySnapshot>();
    const cancelReply = deferred<boolean>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
    vi.mocked(inventoryCancel).mockReturnValueOnce(cancelReply.promise);
    const { result, rerender } = renderHook(({ active }) => useInventory(active), {
      initialProps: { active: true },
    });
    act(() => void result.current.cancel());
    rerender({ active: false });
    expect(result.current.cancelling).toBe(false);
    rerender({ active: true });
    await act(async () => {
      old.reject(new Error("旧扫描失败"));
      cancelReply.reject(new Error("旧取消失败"));
    });
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(true);
    expect(result.current.cancelling).toBe(false);
    expect(inventoryCancel).toHaveBeenCalledTimes(1);
    const nextId = vi.mocked(inventorySnapshot).mock.calls[1][0];
    await act(async () => next.resolve(snapshot(nextId)));
  });

  it("StrictMode 等待取消线程退出后才接续查询，避免撞上后端单槽", async () => {
    const scans = singleSlotBackend();
    const { result } = renderHook(() => useInventory(true), { wrapper: StrictMode });
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    expect(inventoryCancel).toHaveBeenCalledExactlyOnceWith(scans[0].id);
    expect(result.current.loading).toBe(true);
    expect(result.current.waiting).toBe(true);
    await act(async () => Promise.resolve());
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    await act(async () => scans[0].result.reject("工具清单刷新已取消"));
    expect(scans).toHaveLength(2);
    expect(inventorySnapshot).toHaveBeenCalledTimes(2);
    expect(scans[0].id).not.toBe(scans[1].id);
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(true);
    expect(result.current.waiting).toBe(false);
    await act(async () => scans[1].result.resolve(snapshot(scans[1].id)));
    expect(result.current.snapshot?.requestId).toBe(scans[1].id);
    expect(result.current.loading).toBe(false);
  });

  it("其他会话占用后端时报告忙碌，等待用户重试而不自动循环", async () => {
    vi.mocked(inventorySnapshot).mockRejectedValueOnce(new Error("已有扫描正在进行"));
    const { result } = renderHook(() => useInventory(true));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toBe("已有扫描正在进行");
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    await act(() => result.current.refresh());
    expect(result.current.snapshot?.requestId).toBe(vi.mocked(inventorySnapshot).mock.calls[1][0]);
    expect(result.current.error).toBeNull();
  });

  it("等待旧线程退出时可直接取消接续扫描，不发出多余后端请求", async () => {
    const old = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(old.promise);
    const { result, rerender } = renderHook(({ active }) => useInventory(active), {
      initialProps: { active: true },
    });
    rerender({ active: false });
    rerender({ active: true });
    expect(result.current.loading).toBe(true);
    expect(result.current.waiting).toBe(true);
    await act(() => result.current.cancel());
    expect(result.current.loading).toBe(false);
    expect(result.current.waiting).toBe(false);
    expect(result.current.cancelling).toBe(false);
    expect(result.current.error).toBe("工具清单刷新已取消");
    expect(inventoryCancel).toHaveBeenCalledTimes(1);
    await act(async () => old.reject("工具清单刷新已取消"));
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    await act(() => result.current.refresh());
    expect(inventorySnapshot).toHaveBeenCalledTimes(2);
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  it("卸载只取消仍在运行的请求，迟到回复不会再启动扫描", async () => {
    const pending = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot).mockReturnValueOnce(pending.promise);
    const { unmount } = renderHook(() => useInventory(true));
    const id = vi.mocked(inventorySnapshot).mock.calls[0][0];
    unmount();
    expect(inventoryCancel).toHaveBeenCalledExactlyOnceWith(id);
    await act(async () => pending.resolve(snapshot(id)));
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
  });

  it("错误请求身份的快照被拒绝，浏览器无 UUID 时使用独立 ASCII 身份", async () => {
    vi.stubGlobal("crypto", {});
    vi.mocked(inventorySnapshot).mockResolvedValueOnce(snapshot("other-request"));
    const { result } = renderHook(() => useInventory(true));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.snapshot).toBeNull();
    expect(result.current.error).toContain("不匹配");
    await act(() => result.current.refresh());
    const ids = vi.mocked(inventorySnapshot).mock.calls.map(([id]) => id);
    expect(ids[0]).toMatch(/^[A-Za-z0-9_-]{1,128}$/);
    expect(new Set(ids).size).toBe(2);
    expect(result.current.snapshot?.requestId).toBe(ids[1]);
  });
});

it("Python 显式指定工具范围，切换工具等待旧读取退出并拒绝跨工具响应", async () => {
  const pythonFixture = (await import("../../fixtures/python-inventory/contract-v1.json")).default;
  const pending = deferred<InventorySnapshot>();
  vi.mocked(inventorySnapshot)
    .mockReturnValueOnce(pending.promise)
    .mockImplementation(async (id) => ({ ...pythonFixture, requestId: id }) as InventorySnapshot);
  const { result, rerender } = renderHook(({ tool }) => useInventory(true, tool), {
    initialProps: { tool: "node" as "node" | "python" },
  });
  const oldId = vi.mocked(inventorySnapshot).mock.calls[0][0];
  rerender({ tool: "python" });
  expect(inventoryCancel).toHaveBeenCalledWith(oldId);
  expect(inventorySnapshot).toHaveBeenCalledTimes(1);
  await act(async () => pending.resolve(snapshot(oldId)));
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(inventorySnapshot).toHaveBeenLastCalledWith(expect.any(String), undefined, "python");
  expect(result.current.snapshot?.installations.every((i) => i.toolId === "python")).toBe(true);
  vi.mocked(inventorySnapshot).mockImplementationOnce(async (id) => snapshot(id));
  await act(() => result.current.refresh());
  expect(result.current.error).toContain("工具范围");
});
it("接受取消后迟到成功不能发布为新鲜清单", async () => {
  const pending = deferred<InventorySnapshot>();
  vi.mocked(inventorySnapshot).mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useInventory(true));
  const id = vi.mocked(inventorySnapshot).mock.calls[0][0];
  await act(() => result.current.cancel());
  await act(async () => pending.resolve(snapshot(id)));
  expect(result.current.snapshot).toBeNull();
  expect(result.current.error).toContain("已取消");
});
