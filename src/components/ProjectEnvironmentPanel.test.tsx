// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixture from "../../fixtures/project-environment/contract-v1.json";
import { useProjectEnvironment } from "../lib/useProjectEnvironment";
import ProjectEnvironmentPanel from "./ProjectEnvironmentPanel";
vi.mock("../lib/useProjectEnvironment", () => ({ useProjectEnvironment: vi.fn() }));
vi.mock("./OperationPlanDialog", () => ({
  default: (props: {
    initialAction: string;
    initialBinding?: { scope: string; projectPath: string };
    onFinished: () => void;
  }) => (
    <div role="dialog">
      <p>
        {props.initialAction} · {props.initialBinding?.scope} · {props.initialBinding?.projectPath}
      </p>
      <button onClick={props.onFinished}>完成模拟操作</button>
    </div>
  ),
}));
const props = {
  path: "/fixture/project",
  dirty: false,
  busy: false,
  refreshKey: 0,
  onChanged: vi.fn(),
};
function state() {
  return {
    report: structuredClone(fixture.report),
    snapshotId: "inventory-contract",
    loading: false,
    cancelling: false,
    error: null,
    stale: false,
    probe: vi.fn(),
    refresh: vi.fn(),
    cancel: vi.fn(),
  };
}
let current: ReturnType<typeof state>;
beforeEach(() => {
  vi.resetAllMocks();
  current = state();
  vi.mocked(useProjectEnvironment).mockImplementation(
    () => current as ReturnType<typeof useProjectEnvironment>,
  );
});
afterEach(cleanup);
it("匹配候选不会遮盖应用 PATH 不匹配，终端始终保留独立未知状态", () => {
  render(<ProjectEnvironmentPanel {...props} />);
  expect(
    within(screen.getByRole("region", { name: "应用继承 PATH" })).getByText(
      "版本不匹配，不能确认环境满足",
    ),
  ).toBeTruthy();
  expect(
    within(screen.getByRole("region", { name: "项目受控上下文" })).getByText(/22.1.0 · 满足声明/),
  ).toBeTruthy();
  expect(
    within(screen.getByRole("region", { name: "用户终端与 IDE" })).getByText("实际解析未知"),
  ).toBeTruthy();
  expect(current.probe).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "验证受控上下文" }));
  expect(current.probe).toHaveBeenCalledOnce();
});
it("项目绑定复用计划弹窗并预选项目作用域，完成后刷新及使编辑内容失效", async () => {
  render(<ProjectEnvironmentPanel {...props} />);
  fireEvent.click(screen.getAllByRole("button", { name: "预览绑定此实例到项目" })[0]);
  const dialog = await screen.findByRole("dialog");
  expect(dialog.textContent).toContain("bind · project · /fixture/project");
  fireEvent.click(within(dialog).getByRole("button", { name: "完成模拟操作" }));
  expect(props.onChanged).toHaveBeenCalledOnce();
  expect(current.refresh).toHaveBeenCalledOnce();
});
it("旧结果不能显示当前成功，也不能用于发起操作计划", () => {
  current.stale = true;
  render(<ProjectEnvironmentPanel {...props} />);
  expect(screen.queryByText(/已核对 22/)).toBeNull();
  expect(screen.getAllByText("上次结果 · 待重新核对")).toHaveLength(3);
  expect(
    (screen.getAllByRole("button", { name: "预览绑定此实例到项目" })[0] as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    (screen.getByRole("button", { name: "预览 Node 安装计划" }) as HTMLButtonElement).disabled,
  ).toBe(true);
});
it("草稿禁止绑定，来源不完整不能显示确定缺失", () => {
  current.report.candidates = [];
  current.report.inventoryComplete = false;
  render(<ProjectEnvironmentPanel {...props} dirty />);
  expect(screen.getByText(/不能据此断言没有安装/)).toBeTruthy();
  expect(screen.getByText(/草稿尚未保存/)).toBeTruthy();
  expect(
    (screen.getByRole("button", { name: "预览 Node 安装计划" }) as HTMLButtonElement).disabled,
  ).toBe(true);
});
