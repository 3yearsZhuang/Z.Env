// @vitest-environment jsdom
import { useEffect } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  detectToolSources,
  doctorFix,
  doctorRun,
  envCenterList,
  getEnvInfo,
  listTools,
  managedReconcile,
} from "../api";
import EnvironmentView from "./EnvironmentView";
import type {
  InventoryInstallRequest,
  InventoryInstallTarget,
  InventoryTool,
} from "../lib/inventoryNavigation";

vi.mock("../api", () => ({
  detectSystemVersions: vi.fn(async () => []),
  detectToolSources: vi.fn(),
  doctorFix: vi.fn(),
  doctorRun: vi.fn(),
  envCenterList: vi.fn(),
  getEnvInfo: vi.fn(),
  listTools: vi.fn(),
  managedAdopt: vi.fn(),
  managedReconcile: vi.fn(),
  uninstallSystemPackage: vi.fn(),
  uninstallVersion: vi.fn(),
  useVersion: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));
vi.mock("../lib/useMachineInstall", () => ({
  useMachineInstall: () => ({ installBusy: false }),
}));
const probe = vi.hoisted(() => ({
  nodeInstallations: 0,
  nodeHandled: undefined as ((id: number) => void) | undefined,
  dialogClaim: undefined as ((toolId: InventoryTool) => number | null) | undefined,
  dialogRelease: undefined as ((toolId: InventoryTool, token: number) => void) | undefined,
}));
vi.mock("./NodeInventoryPanel", () => ({
  default: ({
    active,
    focusRequest,
    refreshRequest,
    onReadState,
    installRequest,
    onInstallRequestHandled,
    planDialogOwner,
    claimPlanDialog,
    releasePlanDialog,
  }: {
    active: boolean;
    focusRequest: number;
    refreshRequest: number;
    onReadState?: (state: unknown) => void;
    installRequest?: InventoryInstallRequest;
    onInstallRequestHandled?: (id: number) => void;
    planDialogOwner?: InventoryTool | null;
    claimPlanDialog?: (toolId: InventoryTool) => number | null;
    releasePlanDialog?: (toolId: InventoryTool, token: number) => void;
  }) => {
    probe.nodeHandled = onInstallRequestHandled;
    probe.dialogClaim = claimPlanDialog;
    probe.dialogRelease = releasePlanDialog;
    useEffect(() => {
      onReadState?.({
        snapshot: { installations: Array.from({ length: probe.nodeInstallations }, () => ({})) },
        loading: false,
        error: null,
      });
    }, [onReadState]);
    return (
      <section
        aria-label="Node 只读清单"
        data-active={String(active)}
        data-focus={focusRequest}
        data-refresh={refreshRequest}
        data-plan={
          installRequest
            ? `${installRequest.requestId}:${installRequest.providerId}:${installRequest.packageId}`
            : ""
        }
        data-opener={installRequest?.opener?.textContent ?? ""}
        data-dialog-owner={planDialogOwner ?? ""}
      >
        外部 Node 实例
      </section>
    );
  },
}));
vi.mock("./PythonInventoryPanel", () => ({
  default: ({
    active,
    installRequest,
  }: {
    active: boolean;
    installRequest?: InventoryInstallRequest;
  }) => (
    <section
      aria-label="Python 只读清单"
      data-active={String(active)}
      data-plan={
        installRequest
          ? `${installRequest.requestId}:${installRequest.providerId}:${installRequest.packageId}`
          : ""
      }
    >
      Python 来源记录
      <input aria-label="Python 清单会话" defaultValue="已有会话" />
    </section>
  ),
}));
vi.mock("./SoftwareView", () => ({
  default: ({
    active,
    onShowInventory,
    onPreviewInstall,
  }: {
    active: boolean;
    onShowInventory: (tool: InventoryTool) => void;
    onPreviewInstall: (target: InventoryInstallTarget, opener?: HTMLElement) => void;
  }) => (
    <section aria-label="兼容软件列表" data-active={String(active)}>
      <input aria-label="兼容渠道会话" defaultValue="旧会话" />
      <button onClick={() => onShowInventory("python")}>查看 Python 实例</button>
      <button
        onClick={(event) =>
          onPreviewInstall(
            { toolId: "node", providerId: "brew", packageId: "node@22" },
            event.currentTarget,
          )
        }
      >
        预览 Node brew
      </button>
      <button
        onClick={(event) =>
          onPreviewInstall(
            { toolId: "python", providerId: "brew", packageId: "python@3.13" },
            event.currentTarget,
          )
        }
      >
        预览 Python brew
      </button>
    </section>
  ),
}));
vi.mock("./InstallDialog", () => ({ default: () => null }));
vi.mock("./EnvPanel", () => ({ default: () => <div>环境变量内容</div> }));
vi.mock("./MachinePresetSection", () => ({ default: () => null }));
vi.mock("./SnapshotPanel", () => ({
  default: ({ onRestored }: { onRestored: () => void }) => (
    <button onClick={onRestored}>模拟重建完成</button>
  ),
}));

beforeEach(() => {
  vi.resetAllMocks();
  probe.nodeInstallations = 0;
  probe.nodeHandled = undefined;
  probe.dialogClaim = undefined;
  probe.dialogRelease = undefined;
  vi.mocked(listTools).mockResolvedValue([]);
  vi.mocked(detectToolSources).mockResolvedValue([]);
  vi.mocked(getEnvInfo).mockResolvedValue({
    mise_version: null,
    git_version: null,
    os: { name: "macOS", version: "test", arch: "arm64" },
    pkg: [],
  });
  vi.mocked(envCenterList).mockResolvedValue({
    configPath: "/fixture/config.toml",
    exists: false,
    entries: [],
    conflicts: [],
    systemEnv: [],
  });
  vi.mocked(doctorRun).mockResolvedValue([
    {
      id: "managed-health",
      title: "托管接入",
      level: "warn",
      detail: "待显式处理",
      hint: "",
      fixable: true,
    },
  ]);
  vi.mocked(doctorFix).mockResolvedValue("已修复");
});
afterEach(cleanup);

describe("本机工具页的读取边界", () => {
  it("先展示独立 Node 清单，不自动读取管理列表或触发修复", async () => {
    render(<EnvironmentView />);
    expect(screen.getByRole("region", { name: "Node 只读清单" }).getAttribute("data-active")).toBe(
      "true",
    );
    await waitFor(() => expect(envCenterList).toHaveBeenCalledTimes(1));
    expect(listTools).not.toHaveBeenCalled();
    expect(detectToolSources).not.toHaveBeenCalled();
    expect(doctorRun).not.toHaveBeenCalled();
    expect(managedReconcile).not.toHaveBeenCalled();
    expect(doctorFix).not.toHaveBeenCalled();
  });

  it("环境健康独立成区块，修复仅在明确展开并点击后执行", async () => {
    const user = userEvent.setup();
    vi.mocked(listTools).mockRejectedValue(new Error("mise 不可用"));
    vi.mocked(detectToolSources).mockResolvedValue([
      { tool: "python", version: "3.12.1", manager: "pyenv", path: "/fixture/pyenv/3.12.1" },
    ]);
    render(<EnvironmentView />);

    // 旧版折叠区仍保留外部运行时
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    expect(await screen.findByText("python")).toBeTruthy();
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(managedReconcile).not.toHaveBeenCalled();
    expect(doctorFix).not.toHaveBeenCalled();

    // 健康巡检不再埋在旧版折叠区里，而是清单下方的独立区块
    await user.click(screen.getByText(/环境健康巡检/));
    await user.click(await screen.findByRole("button", { name: "一键修复" }));
    await waitFor(() => expect(doctorFix).toHaveBeenCalledExactlyOnceWith("managed-health"));
    await waitFor(() =>
      expect(
        screen.getByRole("region", { name: "Node 只读清单" }).getAttribute("data-refresh"),
      ).toBe("1"),
    );
    expect(managedReconcile).not.toHaveBeenCalled();
    expect(screen.getByRole("region", { name: "Node 只读清单" })).toBeTruthy();
  });

  it("Node 导航意图切回运行时，普通切页不覆盖已选择的环境变量区块", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<EnvironmentView nodeDetailsRequest={0} />);
    await user.click(screen.getByRole("tab", { name: "全局变量" }));
    expect(
      screen
        .getByRole("region", { name: "Node 只读清单", hidden: true })
        .getAttribute("data-active"),
    ).toBe("false");
    rerender(<EnvironmentView nodeDetailsRequest={1} />);
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "安装实例" }).getAttribute("aria-selected")).toBe(
        "true",
      ),
    );
    await user.click(screen.getByRole("tab", { name: "全局变量" }));
    rerender(<EnvironmentView active={false} nodeDetailsRequest={1} />);
    rerender(<EnvironmentView active nodeDetailsRequest={1} />);
    expect(screen.getByRole("tab", { name: "全局变量" }).getAttribute("aria-selected")).toBe(
      "true",
    );
  });

  it("环境重建完成为 Node 清单提供刷新意图", async () => {
    const user = userEvent.setup();
    render(<EnvironmentView />);
    await user.click(screen.getByRole("tab", { name: "预设与快照" }));
    await user.click(screen.getByRole("button", { name: "模拟重建完成" }));
    await user.click(screen.getByRole("tab", { name: "安装实例" }));
    expect(screen.getByRole("region", { name: "Node 只读清单" }).getAttribute("data-refresh")).toBe(
      "1",
    );
  });

  it("其他页面完成安装时也通知当前 Node 清单", () => {
    const { rerender } = render(<EnvironmentView inventoryChange={0} />);
    rerender(<EnvironmentView inventoryChange={1} />);
    expect(screen.getByRole("region", { name: "Node 只读清单" }).getAttribute("data-refresh")).toBe(
      "1",
    );
  });
  it("兼容渠道只在显式展开后加载，收起或切页不丢失会话", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<EnvironmentView />);
    expect(screen.queryByRole("region", { name: "兼容软件列表" })).toBeNull();
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    const session = await screen.findByRole("textbox", { name: "兼容渠道会话" });
    await user.clear(session);
    await user.type(session, "正在核对的内容");
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    await waitFor(() =>
      expect(
        screen
          .getByRole("region", { name: "兼容软件列表", hidden: true })
          .getAttribute("data-active"),
      ).toBe("false"),
    );
    rerender(<EnvironmentView active={false} />);
    rerender(<EnvironmentView />);
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    expect((session as HTMLInputElement).value).toBe("正在核对的内容");
  });
  it("旧软件导航意图打开同页兼容入口，普通切页保留区块选择", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<EnvironmentView compatibilityRequest={0} />);
    await user.click(screen.getByRole("tab", { name: "全局变量" }));
    rerender(<EnvironmentView compatibilityRequest={1} />);
    await screen.findByRole("region", { name: "兼容软件列表" });
    expect(screen.getByRole("tab", { name: "安装实例" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    await user.click(screen.getByRole("tab", { name: "全局变量" }));
    rerender(<EnvironmentView active={false} compatibilityRequest={1} />);
    rerender(<EnvironmentView compatibilityRequest={1} />);
    expect(screen.getByRole("tab", { name: "全局变量" }).getAttribute("aria-selected")).toBe(
      "true",
    );
  });
  it("Python 清单首次显式打开才挂载，切回 Node 保留两个独立读取会话", async () => {
    const user = userEvent.setup();
    render(<EnvironmentView />);
    expect(screen.queryByRole("region", { name: "Python 只读清单" })).toBeNull();
    // 新的运行时切换器是第一入口，不需要先展开能力台账
    await user.click(screen.getByRole("button", { name: /Python/ }));
    const python = await screen.findByRole("region", { name: "Python 只读清单" });
    expect(python.getAttribute("data-active")).toBe("true");
    expect(
      screen
        .getByRole("region", { name: "Node 只读清单", hidden: true })
        .getAttribute("data-active"),
    ).toBe("false");
    await user.click(screen.getByRole("button", { name: /Node.js/ }));
    expect(python.getAttribute("data-active")).toBe("false");
    expect(screen.getByRole("region", { name: "Node 只读清单" }).getAttribute("data-active")).toBe(
      "true",
    );
  });
});

describe("本机工具页的简明性", () => {
  it("切换卡只说清状态：未读取、实例数量、读取失败三种文案", async () => {
    const user = userEvent.setup();
    probe.nodeInstallations = 3;
    render(<EnvironmentView />);

    const nodeCard = screen.getByRole("button", { name: /Node\.js/ });
    expect(nodeCard.textContent).toContain("3 个安装实例");
    // Python 未展开过，不谎报数量
    expect(screen.getByRole("button", { name: /Python/ }).textContent).toContain("点击查看实例");

    // 切换卡不再罗列来源清单（来源细节留给下方清单面板）
    expect(nodeCard.textContent).not.toContain("mise / nvm / Homebrew");
    await user.click(screen.getByRole("tab", { name: "全局变量" }));
    await user.click(screen.getByRole("tab", { name: "安装实例" }));
    expect(screen.getByRole("button", { name: /Node\.js/ }).textContent).toContain("3 个安装实例");
  });

  it("环境健康是独立区块，展开才巡检", async () => {
    const user = userEvent.setup();
    render(<EnvironmentView />);
    expect(doctorRun).not.toHaveBeenCalled();

    await user.click(screen.getByText(/环境健康巡检/));
    await waitFor(() => expect(doctorRun).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("托管接入 · 注意")).toBeTruthy();
  });

  it("旧版入口合并为一个折叠区，内含运行时管理与系统软件两段", async () => {
    const user = userEvent.setup();
    render(<EnvironmentView />);
    expect(screen.getAllByText(/其他工具与旧版渠道/)).toHaveLength(1);

    await user.click(screen.getByText(/其他工具与旧版渠道/));
    expect(await screen.findByText("其他运行时与版本管理")).toBeTruthy();
    expect(screen.getByText("系统软件与兼容渠道")).toBeTruthy();
    expect(screen.getByRole("region", { name: "兼容软件列表" })).toBeTruthy();
  });

  it("环境变量冲突在区块标签上计数，不再重复一条横幅", async () => {
    vi.mocked(envCenterList).mockResolvedValue({
      configPath: "/fixture/config.toml",
      exists: true,
      entries: [],
      conflicts: [
        { key: "HTTP_PROXY", miseValue: "a", systemValue: "b" },
        { key: "PATH", miseValue: "c", systemValue: "d" },
      ],
      systemEnv: [],
    });
    const user = userEvent.setup();
    render(<EnvironmentView />);

    const varsTab = await screen.findByRole("tab", { name: /全局变量/ });
    await waitFor(() => expect(varsTab.textContent).toContain("2"));
    expect(screen.queryByText(/个环境变量同名冲突/)).toBeNull();
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    expect(screen.queryByText(/本机已装 0 个运行时/)).toBeNull();
  });
});

describe("卡片导航与单份安装意图", () => {
  it("鼠标点击未改变焦点时也保存实际计划按钮", async () => {
    const user = userEvent.setup();
    render(<EnvironmentView />);
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    const opener = await screen.findByRole("button", { name: "预览 Node brew" });
    const input = screen.getByRole("textbox", { name: "兼容渠道会话" });
    input.focus();
    fireEvent.click(opener);
    expect(document.activeElement).toBe(input);
    expect(screen.getByRole("region", { name: "Node 只读清单" }).getAttribute("data-opener")).toBe(
      "预览 Node brew",
    );
  });
  it("旧列表失败仍可进入 Python 实例，切回后保留已经访问的清单会话", async () => {
    const user = userEvent.setup();
    vi.mocked(listTools).mockRejectedValue("旧 mise 读取失败");
    render(<EnvironmentView />);
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    await user.click(await screen.findByRole("button", { name: "查看 Python 实例" }));
    const python = await screen.findByRole("region", { name: "Python 只读清单" });
    expect(python.getAttribute("data-active")).toBe("true");
    const session = screen.getByRole("textbox", { name: "Python 清单会话" });
    await user.clear(session);
    await user.type(session, "待核对实例");
    await user.click(screen.getByRole("button", { name: /Node\.js/ }));
    expect(python.getAttribute("data-active")).toBe("false");
    await user.click(screen.getByRole("button", { name: "查看 Python 实例" }));
    expect(screen.getByRole("region", { name: "Python 只读清单" })).toBe(python);
    expect((session as HTMLInputElement).value).toBe("待核对实例");
  });

  it("跨工具计划保留原生包身份，迟到的旧请求不能清空新请求", async () => {
    const user = userEvent.setup();
    render(<EnvironmentView />);
    await user.click(screen.getByText(/其他工具与旧版渠道/));
    const opener = await screen.findByRole("button", { name: "预览 Node brew" });
    opener.focus();
    await user.keyboard("{Enter}");
    const node = screen.getByRole("region", { name: "Node 只读清单" });
    expect(node.getAttribute("data-plan")).toBe("1:brew:node@22");
    expect(node.getAttribute("data-opener")).toBe("预览 Node brew");
    const oldHandled = probe.nodeHandled!;
    await user.click(screen.getByRole("button", { name: "预览 Python brew" }));
    const python = await screen.findByRole("region", { name: "Python 只读清单" });
    expect(python.getAttribute("data-plan")).toBe("2:brew:python@3.13");
    expect(node.getAttribute("data-plan")).toBe("");
    act(() => oldHandled(1));
    expect(python.getAttribute("data-plan")).toBe("2:brew:python@3.13");
    await user.click(screen.getByRole("button", { name: /Node\.js/ }));
    expect(python.getAttribute("data-plan")).toBe("");
    expect(screen.getAllByRole("region", { name: "Node 只读清单" })).toHaveLength(1);
  });
});

it("跨工具同步 claim 只能成功一次，迟到的旧 token 不能释放新弹窗", () => {
  render(<EnvironmentView />);
  let first!: number | null;
  let rejected!: number | null;
  act(() => {
    first = probe.dialogClaim!("node");
    rejected = probe.dialogClaim!("python");
  });
  expect(first).not.toBeNull();
  expect(rejected).toBeNull();
  const node = screen.getByRole("region", { name: "Node 只读清单" });
  expect(node.getAttribute("data-dialog-owner")).toBe("node");
  act(() => probe.dialogRelease!("python", first!));
  expect(node.getAttribute("data-dialog-owner")).toBe("node");
  act(() => probe.dialogRelease!("node", first!));
  expect(node.getAttribute("data-dialog-owner")).toBe("");
  let second!: number | null;
  act(() => {
    second = probe.dialogClaim!("node");
  });
  expect(second).not.toBe(first);
  act(() => probe.dialogRelease!("node", first!));
  expect(node.getAttribute("data-dialog-owner")).toBe("node");
  act(() => probe.dialogRelease!("node", second!));
  expect(node.getAttribute("data-dialog-owner")).toBe("");
});
