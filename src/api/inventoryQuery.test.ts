import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { inventoryCancel, inventorySnapshot } from "./inventoryQuery";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.resetAllMocks());

describe("inventory 查询协议", () => {
  it("全量刷新显式传递空来源，来源重试携带同一请求身份", async () => {
    vi.mocked(invoke).mockResolvedValue({ requestId: "full" });
    expect(await inventorySnapshot("full")).toEqual({ requestId: "full" });
    expect(invoke).toHaveBeenLastCalledWith("inventory_snapshot", {
      requestId: "full",
      providerId: null,
    });
    await inventorySnapshot("brew-retry", "brew");
    expect(invoke).toHaveBeenLastCalledWith("inventory_snapshot", {
      requestId: "brew-retry",
      providerId: "brew",
    });
  });

  it("取消仅作用于指定请求并保留后端是否接受的结果", async () => {
    vi.mocked(invoke).mockResolvedValue(false);
    expect(await inventoryCancel("request-a")).toBe(false);
    expect(invoke).toHaveBeenCalledWith("inventory_cancel", { requestId: "request-a" });
  });
});
