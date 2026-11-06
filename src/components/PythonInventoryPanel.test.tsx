// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixture from "../../fixtures/python-inventory/contract-v1.json";
import { useInventory, type InventoryState } from "../lib/useInventory";
import PythonInventoryPanel from "./PythonInventoryPanel";
import { pythonBindings } from "../api/pythonProject";
vi.mock("../lib/useInventory", () => ({ useInventory: vi.fn() }));
vi.mock("../api/pythonProject", () => ({ pythonBindings: vi.fn() }));
vi.mock("./OperationPlanDialog", () => ({
  default: (p: {
    toolId: string;
    snapshotRequestId: string;
    installationId: string | null;
    initialAction?: string;
    initialBindingId?: string;
    initialProviderId?: string;
    initialPackageId?: string;
    openerElement?: HTMLElement | null;
    onFinished: () => void;
    onClose: () => void;
  }) => (
    <div role="dialog" aria-label="测试计划">
      {p.toolId} · {p.snapshotRequestId} · {p.installationId ?? "install"} ·{" "}
      {p.initialAction ?? "default"} · {p.initialBindingId ?? "no-binding"} ·{" "}
      {p.initialProviderId ?? "mise"} · {p.initialPackageId ?? "python"}
      <span data-testid="opener">{p.openerElement?.id}</span>
      <button onClick={p.onFinished}>完成并刷新</button>
      <button onClick={p.onClose}>关闭测试计划</button>
    </div>
  ),
}));
let state: InventoryState;
const props = { active: true, focusRequest: 0, refreshRequest: 0, onReadState: vi.fn() };
beforeEach(() => {
  vi.resetAllMocks();
  state = {
    snapshot: structuredClone(fixture) as InventoryState["snapshot"],
    loading: false,
    waiting: false,
    cancelling: false,
    error: null,
    ready: true,
    refresh: vi.fn(),
    cancel: vi.fn(),
  };
  vi.mocked(useInventory).mockImplementation(() => state);
  vi.mocked(pythonBindings).mockResolvedValue([]);
});
afterEach(cleanup);
it("Python 读取不复用 Node 范围，来源和隔离探针分开，绑定与安装入口显式", async () => {
  render(<PythonInventoryPanel {...props} />);
  expect(useInventory).toHaveBeenCalledWith(true, "python");
  expect(screen.getByText("3 条来源记录 · 2 条通过隔离探针")).toBeTruthy();
  expect(
    screen.getByText(
      /mise、uv、pyenv 精确安装与 Homebrew core formula 安装\/卸载以计划中的能力、归属和影响为准/,
    ),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^执行/ })).toBeNull();
  expect(screen.getByRole("button", { name: "预览 Python 安装计划" })).toBeTruthy();
  const bind = screen.getAllByRole("button", { name: "绑定此实例到项目" });
  expect(bind.length).toBeGreaterThan(0);
  fireEvent.click(bind[0]);
  const dialog = await screen.findByRole("dialog", { name: "测试计划" });
  expect(dialog.textContent).toContain("bind");
  fireEvent.click(screen.getByRole("button", { name: "关闭测试计划" }));
  fireEvent.click(screen.getByRole("button", { name: "重试 pyenv Python 来源" }));
  expect(state.refresh).toHaveBeenCalledWith("pyenv");
});
it("来源失败空结果不能被报告为未安装，过期结果不显示已验证", () => {
  state.snapshot!.installations = [];
  state.snapshot!.providers[0].coverage.complete = false;
  const { rerender } = render(<PythonInventoryPanel {...props} />);
  expect(screen.getByText(/不能判定未安装/)).toBeTruthy();
  state.snapshot = structuredClone(fixture) as InventoryState["snapshot"];
  state.error = "扫描失败";
  rerender(<PythonInventoryPanel {...props} />);
  expect(screen.getByRole("alert").textContent).toContain("扫描失败");
  expect(screen.queryByText(/已验证 CPython/)).toBeNull();
  expect(screen.getAllByText(/上次记录 · 待核对/)).toHaveLength(3);
});
it("取消期间保留旧证据并禁用重复取消与刷新", () => {
  state.loading = true;
  state.cancelling = true;
  render(<PythonInventoryPanel {...props} />);
  expect(
    (screen.getByRole("button", { name: "Python 取消中…" }) as HTMLButtonElement).disabled,
  ).toBe(true);
  expect(
    (screen.getByRole("button", { name: "刷新 Python 来源" }) as HTMLButtonElement).disabled,
  ).toBe(true);
  expect(props.onReadState).toHaveBeenCalledWith(expect.objectContaining({ loading: true }));
});

it("Python 详情仅显式打开自身计划，完成后刷新自身，离开详情保留弹窗", async () => {
  const { rerender } = render(<PythonInventoryPanel {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "预览 Python 安装计划" }));
  expect((await screen.findByRole("dialog", { name: "测试计划" })).textContent).toContain(
    "python · " + state.snapshot!.requestId + " · install",
  );
  expect(
    (screen.getByRole("button", { name: "刷新 Python 来源" }) as HTMLButtonElement).disabled,
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "完成并刷新" }));
  expect(state.refresh).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "关闭测试计划" }));
  const verify = screen.getAllByRole("button", { name: "预览重新验证此 Python 实例" });
  const healthy = state.snapshot!.installations.find((i) => i.health === "healthy")!;
  fireEvent.click(verify[state.snapshot!.installations.indexOf(healthy)]);
  expect((await screen.findByRole("dialog")).textContent).toContain(healthy.installationId);
  rerender(<PythonInventoryPanel {...props} active={false} />);
  expect(screen.getByRole("dialog").textContent).toContain(healthy.installationId);
});
it("未知 PATH、过期及异常实例没有重新验证入口能力", () => {
  state.snapshot!.installations.forEach((i) => {
    i.health = "unverified";
  });
  const { rerender } = render(<PythonInventoryPanel {...props} />);
  expect(
    screen
      .getAllByRole("button", { name: "预览重新验证此 Python 实例" })
      .every((b) => (b as HTMLButtonElement).disabled),
  ).toBe(true);
  state.snapshot!.installations.forEach((i) => {
    i.health = "healthy";
    i.stale = true;
  });
  rerender(<PythonInventoryPanel {...props} />);
  expect(
    screen
      .getAllByRole("button", { name: "预览重新验证此 Python 实例" })
      .every((b) => (b as HTMLButtonElement).disabled),
  ).toBe(true);
});

it("已有 Python 绑定时可打开解除绑定计划", async () => {
  vi.mocked(pythonBindings).mockResolvedValue([
    {
      bindingId: "python-binding-1",
      installationId: "instance-1",
      version: "3.13.15",
      locator: "/fixture/python",
      configPath: "/work/project/mise.toml",
      scope: "project",
      state: "bound",
    },
  ]);
  render(<PythonInventoryPanel {...props} />);
  const button = await screen.findByRole("button", { name: "解除绑定" });
  fireEvent.click(button);
  const dialog = await screen.findByRole("dialog", { name: "测试计划" });
  expect(dialog.textContent).toContain("unbind");
  expect(dialog.textContent).toContain("python-binding-1");
});

function pythonRead(requestId: string) {
  state = {
    ...state,
    snapshot: {
      ...(structuredClone(fixture) as NonNullable<InventoryState["snapshot"]>),
      requestId,
    },
    loading: false,
    waiting: false,
    cancelling: false,
    error: null,
    ready: true,
  };
}

it("Python 安装请求等待成功重试，保持渠道与原生包，并只回执一次", async () => {
  state = { ...state, loading: true, waiting: true, ready: false };
  const handled = vi.fn();
  const request = {
    requestId: 201,
    providerId: "brew" as const,
    packageId: "homebrew/core/python@3.13",
  };
  const { rerender } = render(
    <PythonInventoryPanel {...props} installRequest={request} onInstallRequestHandled={handled} />,
  );
  expect(state.refresh).not.toHaveBeenCalled();
  state = { ...state, loading: false, waiting: false, error: "读取失败" };
  rerender(
    <PythonInventoryPanel {...props} installRequest={request} onInstallRequestHandled={handled} />,
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(state.refresh).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "刷新 Python 来源" }));
  expect(state.refresh).toHaveBeenCalledOnce();
  pythonRead("python-retried");
  rerender(
    <PythonInventoryPanel {...props} installRequest={request} onInstallRequestHandled={handled} />,
  );
  expect((await screen.findByRole("dialog")).textContent).toContain(
    "python · python-retried · install · install · no-binding · brew · homebrew/core/python@3.13",
  );
  expect(handled).toHaveBeenCalledExactlyOnceWith(201);
  rerender(<PythonInventoryPanel {...props} onInstallRequestHandled={handled} />);
  expect(screen.getByRole("dialog")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "关闭测试计划" }));
  pythonRead("python-next-read");
  rerender(
    <PythonInventoryPanel {...props} installRequest={request} onInstallRequestHandled={handled} />,
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("Python 排队新意图不替换弹窗，父级撤销后不回头打开旧意图", async () => {
  const { rerender } = render(<PythonInventoryPanel {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "预览 Python 安装计划" }));
  await screen.findByRole("dialog");
  const request = { requestId: 202, providerId: "uv" as const, packageId: "python" };
  rerender(<PythonInventoryPanel {...props} active={false} installRequest={request} />);
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(screen.getByRole("dialog").textContent).not.toContain(" · uv · ");
  rerender(<PythonInventoryPanel {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "关闭测试计划" }));
  pythonRead("python-after-revocation");
  rerender(<PythonInventoryPanel {...props} />);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(state.refresh).not.toHaveBeenCalled();
});

it("Python 取消安装导航请求按 ID 回执，扫描继续且迟到结果不打开", () => {
  state = { ...state, loading: true, cancelling: true, ready: false };
  const handled = vi.fn();
  const request = { requestId: 203, providerId: "pyenv" as const, packageId: "python" };
  const { rerender } = render(
    <PythonInventoryPanel {...props} installRequest={request} onInstallRequestHandled={handled} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "取消安装计划请求" }));
  expect(handled).toHaveBeenCalledExactlyOnceWith(203);
  expect(state.cancel).not.toHaveBeenCalled();
  pythonRead("python-after-navigation-cancel");
  rerender(
    <PythonInventoryPanel {...props} installRequest={request} onInstallRequestHandled={handled} />,
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

it.each(["预览 Python 安装计划", "绑定此实例到项目", "预览重新验证此 Python 实例", "解除绑定"])(
  "Python %s 入口必须先取得共享弹窗占用",
  async (label) => {
    const claim = vi.fn(() => null);
    vi.mocked(pythonBindings).mockResolvedValue([
      {
        bindingId: "shared-binding",
        installationId: "instance",
        version: "3.13.15",
        locator: "/fixture/python",
        configPath: "/project/mise.toml",
        scope: "project",
        state: "bound",
      },
    ]);
    const { container } = render(<PythonInventoryPanel {...props} claimPlanDialog={claim} />);
    if (label === "绑定此实例到项目" || label === "预览重新验证此 Python 实例")
      fireEvent.click(container.querySelector(".python-instance-list summary")!);
    const button =
      label === "绑定此实例到项目" || label === "预览重新验证此 Python 实例"
        ? within(container.querySelector(".python-instance-list details")!).getByRole("button", {
            name: label,
          })
        : await screen.findByRole("button", { name: label });
    fireEvent.click(button);
    expect(claim).toHaveBeenCalledExactlyOnceWith("python");
    expect(screen.queryByRole("dialog")).toBeNull();
  },
);

it("Python 已请求读取后遇到外部弹窗，关闭后重新读取，不沿用旧刷新标志卡住", async () => {
  const request = { requestId: 204, providerId: "uv" as const, packageId: "python" };
  const { rerender } = render(<PythonInventoryPanel {...props} installRequest={request} />);
  expect(state.refresh).toHaveBeenCalledOnce();
  state = { ...state, loading: true, ready: false };
  rerender(<PythonInventoryPanel {...props} installRequest={request} planDialogOwner="node" />);
  pythonRead("python-completed-while-node-dialog");
  rerender(<PythonInventoryPanel {...props} installRequest={request} planDialogOwner="node" />);
  expect(screen.queryByRole("dialog")).toBeNull();
  rerender(<PythonInventoryPanel {...props} installRequest={request} />);
  expect(state.refresh).toHaveBeenCalledTimes(2);
  pythonRead("python-completed-after-node-dialog");
  rerender(<PythonInventoryPanel {...props} installRequest={request} />);
  expect((await screen.findByRole("dialog")).textContent).toContain(
    "python-completed-after-node-dialog",
  );
});
