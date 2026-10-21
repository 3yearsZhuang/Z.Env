// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixture from "../../fixtures/python-inventory/contract-v1.json";
import { useInventory, type InventoryState } from "../lib/useInventory";
import PythonInventoryPanel from "./PythonInventoryPanel";
vi.mock("../lib/useInventory", () => ({ useInventory: vi.fn() }));
vi.mock("./OperationPlanDialog", () => ({
  default: (p: {
    toolId: string;
    snapshotRequestId: string;
    installationId: string | null;
    initialAction?: string;
    onFinished: () => void;
    onClose: () => void;
  }) => (
    <div role="dialog" aria-label="测试计划">
      {p.toolId} · {p.snapshotRequestId} · {p.installationId ?? "install"} ·{" "}
      {p.initialAction ?? "default"}
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
    refresh: vi.fn(),
    cancel: vi.fn(),
  };
  vi.mocked(useInventory).mockImplementation(() => state);
});
afterEach(cleanup);
it("Python 读取不复用 Node 范围，来源和隔离探针分开，绑定与安装入口显式", async () => {
  render(<PythonInventoryPanel {...props} />);
  expect(useInventory).toHaveBeenCalledWith(true, "python");
  expect(screen.getByText("3 条来源记录 · 2 条通过隔离探针")).toBeTruthy();
  expect(screen.getByText(/mise 与 uv 托管安装\/卸载以计划中的能力、归属和影响为准/)).toBeTruthy();
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

it("Python 详情仅显式打开自身计划，完成后刷新自身，离开详情关闭弹窗", async () => {
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
  expect(screen.queryByRole("dialog")).toBeNull();
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
