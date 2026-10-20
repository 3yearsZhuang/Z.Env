// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixture from "../../fixtures/project-environment/contract-v1.json";
import { projectEnvironment, type ProjectEnvironment } from "../api/projectEnvironment";
import { inventoryCancel } from "../api/inventoryQuery";
import { useInventory } from "./useInventory";
import { useProjectEnvironment } from "./useProjectEnvironment";
vi.mock("./useInventory", () => ({ useInventory: vi.fn() }));
vi.mock("../api/inventoryQuery", () => ({ inventoryCancel: vi.fn() }));
vi.mock("../api/projectEnvironment", () => ({ projectEnvironment: vi.fn() }));
const scan = {
  snapshot: { requestId: "inventory-contract" },
  loading: false,
  cancelling: false,
  error: null,
  refresh: vi.fn(),
  cancel: vi.fn(),
};
function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function response(requestId: string): ProjectEnvironment {
  return { ...fixture.report, requestId } as ProjectEnvironment;
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(useInventory).mockReturnValue(scan as unknown as ReturnType<typeof useInventory>);
  vi.mocked(inventoryCancel).mockResolvedValue(true);
  vi.mocked(projectEnvironment).mockImplementation(async (r) => response(r.requestId));
});
afterEach(cleanup);
it("元数据读取不执行上下文探测，显式探测保持独立请求身份", async () => {
  const { result } = renderHook(() => useProjectEnvironment("/a", true));
  await waitFor(() => expect(result.current.stale).toBe(false));
  expect(projectEnvironment).toHaveBeenLastCalledWith(
    expect.objectContaining({ path: "/a", probe: false }),
  );
  await act(() => result.current.probe());
  expect(projectEnvironment).toHaveBeenLastCalledWith(expect.objectContaining({ probe: true }));
  expect(result.current.report?.applicationPath.requirementMatch).toBe("mismatch");
});
it("切换项目后丢弃迟到成功，等待原读取退出再查询新项目", async () => {
  const old = pending<ProjectEnvironment>();
  vi.mocked(projectEnvironment).mockReturnValueOnce(old.promise);
  const { result, rerender } = renderHook(({ path }) => useProjectEnvironment(path, true), {
    initialProps: { path: "/a" },
  });
  const id = vi.mocked(projectEnvironment).mock.calls[0][0].requestId;
  rerender({ path: "/b" });
  expect(inventoryCancel).toHaveBeenCalledWith(id);
  expect(projectEnvironment).toHaveBeenCalledTimes(1);
  expect(result.current.stale).toBe(true);
  await act(async () => old.resolve(response(id)));
  await waitFor(() => expect(projectEnvironment).toHaveBeenCalledTimes(2));
  expect(projectEnvironment).toHaveBeenLastCalledWith(expect.objectContaining({ path: "/b" }));
  await waitFor(() => expect(result.current.stale).toBe(false));
  expect(result.current.report?.requestId).not.toBe(id);
});
it("取消被接受后迟到成功不能恢复旧结果的有效状态", async () => {
  const { result } = renderHook(() => useProjectEnvironment("/a", true));
  await waitFor(() => expect(result.current.stale).toBe(false));
  const oldReport = result.current.report;
  const probe = pending<ProjectEnvironment>();
  vi.mocked(projectEnvironment).mockReturnValueOnce(probe.promise);
  let work!: Promise<void>;
  act(() => {
    work = result.current.probe();
  });
  const id =
    vi.mocked(projectEnvironment).mock.calls[vi.mocked(projectEnvironment).mock.calls.length - 1][0]
      .requestId;
  await act(() => result.current.cancel());
  expect(result.current.cancelling).toBe(true);
  await act(async () => {
    probe.resolve(response(id));
    await work;
  });
  expect(result.current.error).toContain("已取消");
  expect(result.current.stale).toBe(true);
  expect(result.current.report).toBe(oldReport);
});
it("拒绝取消恢复取消入口，原成功回复可清除临时错误", async () => {
  const probe = pending<ProjectEnvironment>();
  vi.mocked(projectEnvironment).mockReturnValueOnce(probe.promise);
  vi.mocked(inventoryCancel).mockResolvedValue(false);
  const { result } = renderHook(() => useProjectEnvironment("/a", true));
  const id = vi.mocked(projectEnvironment).mock.calls[0][0].requestId;
  await act(() => result.current.cancel());
  expect(result.current.cancelling).toBe(false);
  expect(result.current.loading).toBe(true);
  expect(result.current.error).toContain("未被接受");
  await act(async () => probe.resolve(response(id)));
  expect(result.current.error).toBeNull();
  expect(result.current.stale).toBe(false);
});
it("身份不符和清单刷新均撤销旧结果的有效状态", async () => {
  const { result, rerender } = renderHook(({ active }) => useProjectEnvironment("/a", active), {
    initialProps: { active: true },
  });
  await waitFor(() => expect(result.current.stale).toBe(false));
  vi.mocked(projectEnvironment).mockResolvedValueOnce(response("wrong"));
  await act(() => result.current.probe());
  expect(result.current.error).toContain("身份不一致");
  expect(result.current.stale).toBe(true);
  rerender({ active: false });
  expect(result.current.stale).toBe(true);
});
