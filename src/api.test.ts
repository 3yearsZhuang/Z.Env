import { describe, expect, it, vi } from "vitest";

// mock Tauri IPC：单元测试只验证纯函数，不触真实后端
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

import { errorMessage } from "./api";

describe("errorMessage", () => {
  it("透传字符串错误", () => {
    expect(errorMessage("boom")).toBe("boom");
  });

  it("提取 Error 对象消息", () => {
    expect(errorMessage(new Error("bad"))).toBe("bad");
  });

  it("其他类型字符串化兜底", () => {
    expect(errorMessage(42)).toBe("42");
  });
});
