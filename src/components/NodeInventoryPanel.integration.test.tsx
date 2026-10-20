// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixture from "../../fixtures/inventory/contract-v1.json";
import type { InventorySnapshot } from "../api/inventory";
import { inventoryCancel, inventorySnapshot } from "../api/inventoryQuery";
import NodeInventoryPanel from "./NodeInventoryPanel";

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

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(inventoryCancel).mockResolvedValue(true);
});

afterEach(cleanup);

describe("清单组件与真实会话状态协作", () => {
  it("扫描期间新增刷新意图再取消，不会在取消完成后自动发起第二次扫描", async () => {
    const user = userEvent.setup();
    const pending = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot).mockReturnValue(pending.promise);
    const { rerender } = render(<NodeInventoryPanel refreshRequest={0} />);
    await waitFor(() => expect(inventorySnapshot).toHaveBeenCalledTimes(1));

    rerender(<NodeInventoryPanel refreshRequest={1} />);
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "取消刷新" }));
    expect(inventoryCancel).toHaveBeenCalledExactlyOnceWith(
      vi.mocked(inventorySnapshot).mock.calls[0][0],
    );
    expect(screen.getByRole("button", { name: "取消中…" })).toHaveProperty("disabled", true);

    // 取消期间的安装完成通知也应由下一次明确刷新处理。
    rerender(<NodeInventoryPanel refreshRequest={2} />);
    await act(async () => pending.reject(new Error("工具清单刷新已取消")));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "刷新全部 Node.js 来源" })).toHaveProperty(
        "disabled",
        false,
      ),
    );
    expect(screen.getByRole("alert").textContent).toContain("工具清单刷新已取消");
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("status")).toBeNull();

    vi.mocked(inventorySnapshot).mockImplementation(async (id) => snapshot(id));
    await user.click(screen.getByRole("button", { name: "刷新全部 Node.js 来源" }));
    await waitFor(() => expect(inventorySnapshot).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("list", { name: "Node.js 安装实例" }).children).toHaveLength(4);
  });

  it("等待旧扫描退出时收到新意图，由接续扫描覆盖，不再追加一次读取", async () => {
    const previous = deferred<InventorySnapshot>();
    const next = deferred<InventorySnapshot>();
    vi.mocked(inventorySnapshot)
      .mockReturnValueOnce(previous.promise)
      .mockReturnValueOnce(next.promise);
    const { rerender } = render(<NodeInventoryPanel active refreshRequest={0} />);
    await waitFor(() => expect(inventorySnapshot).toHaveBeenCalledTimes(1));
    const previousId = vi.mocked(inventorySnapshot).mock.calls[0][0];

    rerender(<NodeInventoryPanel active={false} refreshRequest={0} />);
    rerender(<NodeInventoryPanel active refreshRequest={0} />);
    expect(screen.getByRole("status").textContent).toContain("正在等待上次读取结束");
    rerender(<NodeInventoryPanel active refreshRequest={1} />);
    expect(inventorySnapshot).toHaveBeenCalledTimes(1);

    await act(async () => previous.resolve(snapshot(previousId)));
    await waitFor(() => expect(inventorySnapshot).toHaveBeenCalledTimes(2));
    const nextId = vi.mocked(inventorySnapshot).mock.calls[1][0];
    expect(nextId).not.toBe(previousId);
    await act(async () => next.resolve(snapshot(nextId)));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "刷新全部 Node.js 来源" })).toHaveProperty(
        "disabled",
        false,
      ),
    );
    expect(inventorySnapshot).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("list", { name: "Node.js 安装实例" }).children).toHaveLength(4);
  });
});
