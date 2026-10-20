// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { operationCancel, operationList, type Operation } from "../api";
import TaskPanel from "./TaskPanel";

vi.mock("../api", () => ({
  operationCancel: vi.fn(),
  operationList: vi.fn(),
  errorMessage: (error: unknown) => String(error),
}));

const running: Operation = {
  id: "install-node",
  label: "安装 node",
  status: "running",
  lines: ["正在下载 node"],
  cancelRequested: false,
};

beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  vi.mocked(operationList).mockResolvedValue([running]);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

async function openPanel() {
  await act(async () => {
    render(<TaskPanel />);
  });
  fireEvent.click(screen.getByText("安装 node进行中…"));
}

describe("全局任务反馈", () => {
  it("取消立即禁用，旧轮询结果不会允许重复取消，终态由后端确认", async () => {
    let finish!: () => void;
    vi.mocked(operationCancel).mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));
    await openPanel();
    const button = screen.getByRole("button", { name: "取消任务：安装 node" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(operationCancel).toHaveBeenCalledTimes(1);
    await act(async () => finish());
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect((button as HTMLButtonElement).disabled).toBe(true);
    vi.mocked(operationList).mockResolvedValue([{ ...running, status: "cancelled" }]);
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(screen.queryByRole("button", { name: "取消任务：安装 node" })).toBeNull();
    expect(screen.getByText("已取消")).toBeTruthy();
  });

  it("取消失败说明所属任务并允许重试", async () => {
    vi.mocked(operationCancel).mockRejectedValueOnce("连接超时").mockResolvedValueOnce(undefined);
    await openPanel();
    const button = screen.getByRole("button", { name: "取消任务：安装 node" });
    await act(async () => fireEvent.click(button));
    expect(screen.getByRole("alert").textContent).toContain("无法取消「安装 node」：连接超时");
    expect((button as HTMLButtonElement).disabled).toBe(false);
    await act(async () => fireEvent.click(button));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(operationCancel).toHaveBeenCalledTimes(2);
  });

  it("轮询失败保留最近任务并说明数据过期，连接恢复后移除提示", async () => {
    await openPanel();
    vi.mocked(operationList).mockRejectedValueOnce("离线");
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(screen.getByRole("status").textContent).toContain("上次获取的结果");
    expect(screen.getByRole("region", { name: "安装 node" })).toBeTruthy();
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("重启后的步骤记录", () => {
  it("独立显示补偿完成与外部修改冲突，不把补偿结果当作原任务成功", async () => {
    vi.mocked(operationList).mockResolvedValue([
      {
        ...running,
        id: "bind-cancelled",
        label: "Node 绑定",
        status: "cancelled",
        compensation: "completed",
        steps: [{ kind: "compensate", status: "completed" }],
      },
      {
        ...running,
        id: "bind-conflict",
        label: "Node 绑定冲突",
        status: "partial",
        compensation: "conflict",
        steps: [{ kind: "compensate", status: "failed" }],
      },
    ]);
    await act(async () => {
      render(<TaskPanel />);
    });
    fireEvent.click(screen.getByText("最近任务"));
    expect(screen.getByText("本次补偿已完成，请刷新确认原来源状态。")).toBeTruthy();
    expect(screen.getByText(/补偿遇到外部修改，未覆盖冲突内容/)).toBeTruthy();
    expect(screen.getByText("补偿本次变更 · 已完成")).toBeTruthy();
    expect(screen.getByText("补偿本次变更 · 未完成")).toBeTruthy();
    expect(screen.queryByText("已完成")).toBeNull();
  });
  it("中断任务明确未重放，展示完成和跳过的步骤且不能取消", async () => {
    vi.mocked(operationList).mockResolvedValue([
      {
        ...running,
        status: "interrupted",
        steps: [
          { kind: "command", status: "completed" },
          { kind: "verify", status: "skipped" },
        ],
      },
    ]);
    await act(async () => {
      render(<TaskPanel />);
    });
    fireEvent.click(screen.getByText("最近任务"));
    expect(screen.getByText("已中断，未重放")).toBeTruthy();
    expect(screen.getByRole("list", { name: "安装 node的执行步骤" }).textContent).toContain(
      "执行变更 · 已完成",
    );
    expect(screen.getByText("确认结果 · 已跳过")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /取消任务/ })).toBeNull();
  });
});
