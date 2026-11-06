// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import MachinePresetSection from "./MachinePresetSection";
import { presetParseTools, recipePreview } from "../api";

vi.mock("../api", () => ({
  errorMessage: (e: unknown) => String(e),
  listTools: vi.fn().mockResolvedValue([]),
  presetParseTools: vi.fn(),
  recipePreview: vi.fn(),
}));
vi.mock("../lib/useUserPresets", () => ({
  useUserPresets: () => ({
    userPresets: [
      {
        name: "项目配方",
        content: '[tools]\npython = "3.13.15"\nnode = "22"\n',
        description: "测试",
      },
    ],
    persist: vi.fn(),
    remove: vi.fn(),
    remember: vi.fn(),
  }),
}));
vi.mock("../lib/presetFileIO", () => ({ exportPresetFile: vi.fn() }));

function api() {
  return {
    installLog: [],
    setInstallLog: vi.fn(),
    installBusy: false,
    logRef: { current: null },
    install: vi.fn().mockResolvedValue(2),
  } as never;
}

beforeEach(() => {
  vi.mocked(presetParseTools).mockReset();
  vi.mocked(recipePreview).mockReset();
});

it("预设弹窗显示只读配方检查计数且不阻塞安装", async () => {
  vi.mocked(presetParseTools).mockResolvedValue([
    { name: "python", version: "3.13.15" },
    { name: "node", version: "22" },
  ]);
  vi.mocked(recipePreview).mockResolvedValue({
    schemaVersion: 2,
    snapshotRequestId: null,
    snapshotBindings: [
      { tool: "python", requestId: "s", generation: "2" },
      { tool: "node", requestId: "n", generation: "1" },
    ],
    executable: false,
    issues: ["node：22 无法只读判断"],
    steps: [
      {
        tool: "python",
        request: "3.13.15",
        disposition: "reuse",
        channel: "mise",
        installationId: "p",
        version: "3.13.15",
        reason: "已有",
        planReady: false,
      },
      {
        tool: "node",
        request: "22",
        disposition: "unknown",
        channel: null,
        installationId: null,
        version: null,
        reason: "范围",
        planReady: false,
      },
    ],
  });
  render(<MachinePresetSection installApi={api()} />);
  fireEvent.click(screen.getByRole("button", { name: /项目配方/ }));
  await waitFor(() => expect(screen.getByRole("status")).toBeTruthy());
  expect(screen.getByRole("status").textContent).toContain("1 项可复用");
  expect(screen.getByRole("status").textContent).toContain("1 项需人工确认");
});

it("配方检查失败时仍允许原有整机安装流程", async () => {
  vi.mocked(presetParseTools).mockResolvedValue([{ name: "python", version: "3.13.15" }]);
  vi.mocked(recipePreview).mockRejectedValue("boom");
  render(<MachinePresetSection installApi={api()} />);
  fireEvent.click(screen.getByRole("button", { name: /项目配方/ }));
  await waitFor(() => expect(screen.getByText(/共 1 个工具/)).toBeTruthy());
  expect(screen.queryByRole("status")).toBeNull();
});
