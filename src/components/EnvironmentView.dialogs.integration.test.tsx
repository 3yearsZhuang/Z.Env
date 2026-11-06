// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import nodeFixture from "../../fixtures/inventory/contract-v1.json";
import pythonFixture from "../../fixtures/python-inventory/contract-v1.json";
import type { InventorySnapshot } from "../api/inventory";
import { useInventory, type InventoryState } from "../lib/useInventory";
import type { InventoryInstallTarget, InventoryTool } from "../lib/inventoryNavigation";
import EnvironmentView from "./EnvironmentView";

vi.mock("../api", () => ({
  detectSystemVersions: vi.fn(async () => []),
  detectToolSources: vi.fn(async () => []),
  doctorFix: vi.fn(),
  doctorRun: vi.fn(),
  envCenterList: vi.fn(async () => ({
    configPath: "/fixture/config.toml",
    exists: false,
    entries: [],
    conflicts: [],
    systemEnv: [],
  })),
  getEnvInfo: vi.fn(async () => ({
    mise_version: null,
    git_version: null,
    os: { name: "macOS", version: "test", arch: "arm64" },
    pkg: [],
  })),
  listTools: vi.fn(async () => []),
  managedAdopt: vi.fn(),
  managedReconcile: vi.fn(),
  uninstallSystemPackage: vi.fn(),
  uninstallVersion: vi.fn(),
  useVersion: vi.fn(),
  operationCancel: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));
vi.mock("../lib/useMachineInstall", () => ({ useMachineInstall: () => ({ installBusy: false }) }));
vi.mock("../lib/useInventory", () => ({ useInventory: vi.fn() }));
vi.mock("../api/pythonProject", () => ({ pythonBindings: vi.fn(async () => []) }));
vi.mock("../api/operationPlan", () => ({
  operationBindings: vi.fn(async () => []),
  operationExecute: vi.fn(),
}));
vi.mock("./SoftwareView", () => ({
  default: ({
    onPreviewInstall,
  }: {
    onPreviewInstall: (target: InventoryInstallTarget, opener: HTMLElement) => void;
  }) => (
    <div>
      {(["node", "python"] as const).map((toolId) => (
        <button
          key={toolId}
          onClick={(event) =>
            onPreviewInstall(
              {
                toolId,
                providerId: "brew",
                packageId: toolId === "node" ? "node@22" : "python@3.13",
              },
              event.currentTarget,
            )
          }
        >
          预览 {toolId} 渠道
        </button>
      ))}
      {(["node", "python"] as const).map((toolId) => (
        <button
          key={toolId}
          onClick={(event) =>
            onPreviewInstall(
              {
                toolId,
                providerId: "brew",
                packageId: toolId === "node" ? "node@24" : "python@3.14",
              },
              event.currentTarget,
            )
          }
        >
          预览 {toolId} 更新渠道
        </button>
      ))}
    </div>
  ),
}));
vi.mock("./InstallDialog", () => ({ default: () => null }));
vi.mock("./EnvPanel", () => ({ default: () => null }));
vi.mock("./MachinePresetSection", () => ({ default: () => null }));
vi.mock("./SnapshotPanel", () => ({ default: () => null }));

let states: Record<InventoryTool, InventoryState>;
beforeEach(() => {
  states = {
    node: {
      snapshot: structuredClone(nodeFixture.expectedSnapshot) as InventorySnapshot,
      ready: true,
      loading: false,
      waiting: false,
      cancelling: false,
      error: null,
      refresh: vi.fn(async () => undefined),
      cancel: vi.fn(async () => undefined),
    },
    python: {
      snapshot: structuredClone(pythonFixture) as InventorySnapshot,
      ready: true,
      loading: false,
      waiting: false,
      cancelling: false,
      error: null,
      refresh: vi.fn(async () => undefined),
      cancel: vi.fn(async () => undefined),
    },
  };
  vi.mocked(useInventory).mockImplementation((_active, toolId = "node") => states[toolId]);
});
afterEach(async () => {
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 0));
});

function publish(toolId: InventoryTool, requestId: string) {
  states[toolId] = { ...states[toolId], snapshot: { ...states[toolId].snapshot!, requestId } };
}

it.each([
  ["node", "python"],
  ["python", "node"],
] as const)(
  "%s 计划打开时 %s 程序导航只排队，关闭后读取并打开最新请求",
  async (firstTool, nextTool) => {
    const user = userEvent.setup();
    const { rerender } = render(<EnvironmentView />);
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    await screen.findByText("预览 node 渠道");
    if (firstTool === "python")
      await user.click(screen.getByRole("button", { name: /Python.*点击查看实例/ }));
    const manual = firstTool === "node" ? "预览 Node 计划" : "预览 Python 安装计划";
    await user.click(await screen.findByRole("button", { name: manual }));
    const title = firstTool === "node" ? "Node.js 操作计划" : "Python 操作计划";
    await screen.findByRole("dialog", { name: title });
    const original = document.querySelector('[data-slot="dialog-content"]');
    fireEvent.click(screen.getByText(`预览 ${nextTool} 渠道`, { selector: "button" }));
    fireEvent.click(screen.getByText(`预览 ${nextTool} 更新渠道`, { selector: "button" }));
    publish(nextTool, `${nextTool}-while-other-dialog`);
    rerender(<EnvironmentView />);
    expect(document.querySelectorAll('[data-slot="dialog-content"]')).toHaveLength(1);
    expect(document.querySelector('[data-slot="dialog-content"]')).toBe(original);
    expect(states[nextTool].refresh).not.toHaveBeenCalled();
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(document.querySelectorAll('[data-slot="dialog-content"]')).toHaveLength(0),
    );
    await waitFor(() => expect(states[nextTool].refresh).toHaveBeenCalledOnce());
    publish(nextTool, `${nextTool}-after-dialog`);
    rerender(<EnvironmentView />);
    const nextTitle = nextTool === "node" ? "Node.js 操作计划" : "Python 操作计划";
    await screen.findByRole("dialog", { name: nextTitle });
    expect(document.querySelectorAll('[data-slot="dialog-content"]')).toHaveLength(1);
    expect(
      (
        screen.getByRole("textbox", {
          name: nextTool === "node" ? "Node formula" : "Python formula",
        }) as HTMLInputElement
      ).value,
    ).toBe(nextTool === "node" ? "node@24" : "python@3.14");
    expect(document.querySelector('[data-slot="dialog-content"]')).not.toBe(original);
  },
);
