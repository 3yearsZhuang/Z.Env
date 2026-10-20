// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Navigate } from "./lib/nav";
import App from "./App";
vi.mock("./logger", () => ({ initLogger: vi.fn(), logInfo: vi.fn() }));
vi.mock("./components/TaskPanel", () => ({ default: () => null }));
vi.mock("./components/Dashboard", () => ({
  default: ({ onNavigate }: { onNavigate: Navigate }) => (
    <button onClick={() => onNavigate("software")}>旧软件跳转</button>
  ),
}));
vi.mock("./components/EnvironmentView", () => ({
  default: ({ compatibilityRequest }: { compatibilityRequest: number }) => (
    <div>统一工具页面，兼容意图 {compatibilityRequest}</div>
  ),
}));
beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() })),
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
it("主导航收敛为五个入口，旧软件意图转入本机工具页而非打开第二份列表", async () => {
  render(<App />);
  for (const name of ["系统概览", "本机工具", "项目配置", "服务与缓存", "设置"])
    expect(screen.getByRole("button", { name })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "支持列表" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "旧软件跳转" }));
  await screen.findByText("统一工具页面，兼容意图 1");
  expect(screen.getByRole("button", { name: "本机工具" }).getAttribute("aria-current")).toBe(
    "page",
  );
});
