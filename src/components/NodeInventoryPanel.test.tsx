// @vitest-environment jsdom
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import sharedFixture from "../../fixtures/inventory/contract-v1.json";
import type { InventorySnapshot, ProviderId } from "../api/inventory";
import { useInventory, type InventoryState } from "../lib/useInventory";
import NodeInventoryPanel from "./NodeInventoryPanel";

vi.mock("../lib/useInventory", () => ({ useInventory: vi.fn() }));

const refresh = vi.fn<(providerId?: ProviderId) => Promise<void>>();
const cancel = vi.fn<() => Promise<void>>();
let state: InventoryState;

function snapshot(): InventorySnapshot {
  return structuredClone(sharedFixture.expectedSnapshot) as InventorySnapshot;
}

function source(name: string) {
  return within(screen.getByRole("list", { name: "Node.js 来源状态" })).getByRole("listitem", {
    name: `${name} 来源`,
  });
}

function instance(provider: string, architecture: string) {
  return screen.getByLabelText(new RegExp(`^${provider} .* ${architecture}，目录`));
}

beforeEach(() => {
  vi.resetAllMocks();
  refresh.mockResolvedValue(undefined);
  cancel.mockResolvedValue(undefined);
  state = {
    snapshot: snapshot(),
    loading: false,
    waiting: false,
    cancelling: false,
    error: null,
    refresh,
    cancel,
  };
  vi.mocked(useInventory).mockImplementation(() => state);
});

afterEach(cleanup);

describe("Node.js 多来源只读清单", () => {
  it("mise 不可用时仍展示外部安装与独立来源状态，错误只报告一次", () => {
    const mise = state.snapshot!.providers.find((report) => report.providerId === "mise")!;
    mise.status = "unavailable";
    mise.error = { code: "unavailable", message: "未找到 mise 可执行文件", retryable: true };
    render(<NodeInventoryPanel />);

    expect(screen.getByRole("region", { name: "Node.js 本机清单" })).toBeTruthy();
    expect(screen.getByRole("list", { name: "Node.js 安装实例" }).children).toHaveLength(4);
    expect(within(source("mise")).getByText("未知 · 来源不可用")).toBeTruthy();
    expect(within(source("mise")).getByText("安装情况未知")).toBeTruthy();
    expect(within(source("Homebrew")).getByText("2 条来源记录")).toBeTruthy();
    expect(within(source("Homebrew")).getByText("读取完成")).toBeTruthy();
    expect(screen.getAllByText("mise：未找到 mise 可执行文件")).toHaveLength(1);
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByText(/不代表终端或项目正在使用它/)).toBeTruthy();
    expect(screen.queryByText(/本机还没有/)).toBeNull();
    expect(screen.queryByRole("button", { name: /安装|卸载|接管|设为/ })).toBeNull();
  });

  it("重试只传递对应来源，全量刷新不附带来源", async () => {
    const user = userEvent.setup();
    render(<NodeInventoryPanel />);
    await user.click(screen.getByRole("button", { name: "重试 mise 来源" }));
    expect(refresh).toHaveBeenNthCalledWith(1, "mise");
    await user.click(screen.getByRole("button", { name: "刷新 Homebrew 来源" }));
    expect(refresh).toHaveBeenNthCalledWith(2, "brew");
    await user.click(screen.getByRole("button", { name: "刷新全部 Node.js 来源" }));
    expect(refresh).toHaveBeenNthCalledWith(3);
    expect(cancel).not.toHaveBeenCalled();
  });

  it("同版本不同目录各有详情；文件关联链接展开并聚焦目标，保留独立归属", async () => {
    const user = userEvent.setup();
    const { container } = render(<NodeInventoryPanel />);
    const arm = instance("Homebrew", "arm64");
    const intel = instance("Homebrew", "x86_64");
    const path = instance("PATH", "arm64");
    expect(arm.parentElement).not.toBe(intel.parentElement);
    expect(arm.textContent).toContain("/opt/homebrew/Cellar/node@22/22.11.0");
    expect(intel.textContent).toContain("/usr/local/Cellar/node@22/22.11.0");
    expect(path.textContent).toContain("仅观测");

    await user.click(path);
    const link = screen.getByRole("link", { name: /查看 Homebrew · node@22 · 22.11.0 · arm64/ });
    expect(link.getAttribute("href")).toBe(`#${arm.id}`);
    expect(arm.id).toMatch(/^[a-zA-Z0-9_-]+$/);
    expect(screen.getByText(/文件关联不会改变各自归属/)).toBeTruthy();
    await user.click(link);
    expect((arm.parentElement as HTMLDetailsElement).open).toBe(true);
    expect(document.activeElement).toBe(arm);
    expect(path.textContent).toContain("仅观测");
    expect(path.textContent).not.toContain("已确认归属");
    expect(arm.textContent).toContain("已确认归属");
    expect(container.querySelectorAll(".node-inventory-instance")).toHaveLength(4);
  });

  it("来源失败、未支持和不完整均保留未知，只有完整的空来源可说未发现", () => {
    state.snapshot!.installations = [];
    state.snapshot!.aliases = [];
    state.snapshot!.providers = state.snapshot!.providers.map((report) => ({
      ...report,
      error: null,
    }));
    state.snapshot!.providers.find((report) => report.providerId === "mise")!.status =
      "unavailable";
    state.snapshot!.providers.find((report) => report.providerId === "nvm")!.status = "unsupported";
    state.snapshot!.providers.find((report) => report.providerId === "path")!.coverage.truncated =
      true;
    render(<NodeInventoryPanel />);

    expect(within(source("mise")).getByText("安装情况未知")).toBeTruthy();
    expect(within(source("Homebrew")).getByText("未发现 Node.js 记录")).toBeTruthy();
    expect(within(source("nvm")).getByText("当前平台不支持")).toBeTruthy();
    expect(within(source("nvm")).getByText("此发现能力目前仅支持 macOS")).toBeTruthy();
    expect(within(source("PATH")).getByText("结果已截断")).toBeTruthy();
    expect(within(source("PATH")).getByText("安装情况未知")).toBeTruthy();
    expect(screen.getByText(/尚无可展示的 Node.js 安装记录；部分来源情况未知/)).toBeTruthy();
    expect(screen.queryByText("当前来源范围内未发现 Node.js 安装记录。")).toBeNull();
  });

  it("全部来源完整读取为空时，说明未发现的范围", () => {
    state.snapshot!.installations = [];
    state.snapshot!.aliases = [];
    state.snapshot!.providers = state.snapshot!.providers.map((report) => ({
      ...report,
      status: "ok",
      error: null,
      coverage: { toolIds: ["node"], complete: true, truncated: false },
    }));
    render(<NodeInventoryPanel />);
    expect(screen.getByText("当前来源范围内未发现 Node.js 安装记录。")).toBeTruthy();
    expect(screen.getAllByText("未发现 Node.js 记录")).toHaveLength(4);
  });

  it("成功状态附带错误时仍视为未知，不据空清单推断未发现", () => {
    state.snapshot!.installations = [];
    state.snapshot!.aliases = [];
    state.snapshot!.providers = state.snapshot!.providers.map((report) => ({
      ...report,
      status: "ok",
      error:
        report.providerId === "brew"
          ? { code: "partialRead", message: "部分安装目录读取失败", retryable: true }
          : null,
      coverage: { toolIds: ["node"], complete: true, truncated: false },
    }));
    render(<NodeInventoryPanel />);

    expect(within(source("Homebrew")).getByText("读取未完整")).toBeTruthy();
    expect(within(source("Homebrew")).getByText("安装情况未知")).toBeTruthy();
    expect(within(source("Homebrew")).queryByText("未发现 Node.js 记录")).toBeNull();
    expect(
      within(source("Homebrew")).getByRole("button", { name: "重试 Homebrew 来源" }),
    ).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("部分安装目录读取失败");
    expect(screen.getByText(/尚无可展示的 Node.js 安装记录；部分来源情况未知/)).toBeTruthy();
    expect(screen.queryByText("当前来源范围内未发现 Node.js 安装记录。")).toBeNull();
  });

  it("损坏与未知版本、架构、路径分别呈现，详情保留全部证据及过期标记", async () => {
    const user = userEvent.setup();
    const nvm = state.snapshot!.installations.find(
      (item) => item.providerPackage.providerId === "nvm",
    )!;
    nvm.stale = true;
    nvm.evidence.push({ kind: "directoryEntry", path: null, detail: null });
    render(<NodeInventoryPanel />);
    const summary = instance("nvm", "架构未知");
    expect(summary.textContent).toContain("版本未知");
    expect(summary.textContent).toContain("异常");
    expect(summary.textContent).toContain("归属未知");
    expect(summary.textContent).toContain("上次结果");
    await user.click(summary);
    const body = within(summary.parentElement!);
    expect(body.getByText("此记录来自上次结果，最新状态待确认。")).toBeTruthy();
    expect(body.getByText("解析后文件路径").nextElementSibling?.textContent).toBe("未知");
    expect(body.getByText("原始文件路径").nextElementSibling?.textContent).toBe(nvm.executablePath);
    expect(body.getByText("executableProbe")).toBeTruthy();
    expect(body.getByText("Executable could not be probed")).toBeTruthy();
    expect(body.getByText("directoryEntry")).toBeTruthy();
    expect(body.getAllByText("未提供")).toHaveLength(2);
  });

  it("未知 PATH 记录明确只保留文件证据，未把目录名当成版本", async () => {
    const user = userEvent.setup();
    const path = state.snapshot!.installations.find(
      (item) => item.providerPackage.providerId === "path",
    )!;
    path.health = "unverified";
    path.version = null;
    path.architecture = null;
    render(<NodeInventoryPanel />);
    const summary = instance("PATH", "架构未知");
    await user.click(summary);
    expect(summary.textContent).toContain("版本未知");
    expect(summary.textContent).toContain("未验证");
    expect(screen.getByText("仅保留文件证据；未自动执行未知来源的 PATH 文件。")).toBeTruthy();
    expect(within(summary.parentElement!).getByText("pathLookup")).toBeTruthy();
    expect(
      within(summary.parentElement!).getByText("First matching executable on PATH"),
    ).toBeTruthy();
  });

  it("全量查询失败仍保留所有安装记录并提示上次结果，可重新刷新", () => {
    state.error = "读取连接已断开";
    render(<NodeInventoryPanel />);
    expect(screen.getByRole("alert").textContent).toContain("当前展示上次读取的结果");
    expect(screen.getByRole("list", { name: "Node.js 安装实例" }).children).toHaveLength(4);
    expect(screen.getAllByText("上次结果")).toHaveLength(4);
    expect(screen.getAllByText("上次结果 · 待刷新")).toHaveLength(4);
    expect(screen.getByRole("button", { name: "刷新全部 Node.js 来源" })).toHaveProperty(
      "disabled",
      false,
    );
  });

  it("读取中保留快照并允许取消，等待取消确认时阻止重复操作", async () => {
    const user = userEvent.setup();
    state.loading = true;
    const { rerender } = render(<NodeInventoryPanel />);
    expect(screen.getByRole("list", { name: "Node.js 安装实例" }).children).toHaveLength(4);
    expect(screen.getByRole("status").textContent).toContain("正在读取各来源");
    expect(screen.getByRole("button", { name: "刷新全部 Node.js 来源" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(screen.getByRole("button", { name: "重试 mise 来源" })).toHaveProperty("disabled", true);
    await user.click(screen.getByRole("button", { name: "取消刷新" }));
    expect(cancel).toHaveBeenCalledTimes(1);

    state = { ...state, cancelling: true };
    rerender(<NodeInventoryPanel />);
    const cancelling = screen.getByRole("button", { name: "取消中…" });
    expect(cancelling).toHaveProperty("disabled", true);
    expect(screen.getByRole("status").textContent).toContain("等待本次读取结束");
    await user.click(cancelling);
    expect(cancel).toHaveBeenCalledTimes(1);
  });

  it("无快照查询失败不会显示未安装结论", () => {
    state = { ...state, snapshot: null, error: "连接失败" };
    render(<NodeInventoryPanel />);
    expect(screen.getByRole("alert").textContent).toContain("连接失败");
    expect(screen.getByText(/尚无可展示的 Node.js 安装记录/)).toBeTruthy();
    expect(screen.getAllByText("安装情况未知")).toHaveLength(4);
    expect(screen.queryByText(/未发现 Node.js 记录/)).toBeNull();
  });

  it("安装记录可从键盘到达，使用原生详情元素展开与折叠", async () => {
    const user = userEvent.setup();
    render(<NodeInventoryPanel />);
    const summary = instance("Homebrew", "arm64");
    const details = summary.parentElement as HTMLDetailsElement;
    screen.getByRole("button", { name: "刷新全部 Node.js 来源" }).focus();
    for (let index = 0; index < 5; index += 1) await user.tab();
    expect(document.activeElement).toBe(summary);
    expect(summary.tagName).toBe("SUMMARY");
    expect(details.open).toBe(false);
    // jsdom 未实现 summary 的键盘默认行为；真实浏览器另验 Enter 与空格。
    await user.click(summary);
    expect(details.open).toBe(true);
    await user.click(summary);
    expect(details.open).toBe(false);
  });

  it("默认挂载不抢焦点；详情导航意图激活时聚焦标题且同一意图只处理一次", () => {
    const { rerender } = render(<NodeInventoryPanel active={false} />);
    const heading = screen.getByRole("heading", { name: "Node.js 本机清单" });
    expect(document.activeElement).not.toBe(heading);
    rerender(<NodeInventoryPanel active={false} focusRequest={1} />);
    expect(document.activeElement).not.toBe(heading);
    rerender(<NodeInventoryPanel active focusRequest={1} />);
    expect(document.activeElement).toBe(heading);
    const button = screen.getByRole("button", { name: "刷新全部 Node.js 来源" });
    button.focus();
    rerender(<NodeInventoryPanel active focusRequest={1} />);
    expect(document.activeElement).toBe(button);
    expect(useInventory).toHaveBeenLastCalledWith(true);
  });

  it("已有操作完成后的刷新意图在本次读取结束后合并执行，挂载不额外扫描", () => {
    state.loading = true;
    const { rerender } = render(<NodeInventoryPanel refreshRequest={4} />);
    expect(refresh).not.toHaveBeenCalled();
    rerender(<NodeInventoryPanel refreshRequest={5} />);
    rerender(<NodeInventoryPanel refreshRequest={6} />);
    expect(refresh).not.toHaveBeenCalled();
    state = { ...state, loading: false };
    rerender(<NodeInventoryPanel refreshRequest={6} />);
    expect(refresh).toHaveBeenCalledExactlyOnceWith();
    rerender(<NodeInventoryPanel refreshRequest={6} />);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("隐藏时延迟刷新意图，恢复激活后只执行最新一次", () => {
    const { rerender } = render(<NodeInventoryPanel active={false} refreshRequest={0} />);
    rerender(<NodeInventoryPanel active={false} refreshRequest={1} />);
    expect(refresh).not.toHaveBeenCalled();

    rerender(<NodeInventoryPanel active refreshRequest={2} />);
    expect(refresh).toHaveBeenCalledExactlyOnceWith();
    rerender(<NodeInventoryPanel active refreshRequest={2} />);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("取消期间丢弃刷新意图，取消结束后不自动重启", () => {
    state = { ...state, loading: true, cancelling: true };
    const { rerender } = render(<NodeInventoryPanel refreshRequest={0} />);
    rerender(<NodeInventoryPanel refreshRequest={1} />);
    expect(refresh).not.toHaveBeenCalled();
    state = { ...state, loading: false, cancelling: false };
    rerender(<NodeInventoryPanel refreshRequest={1} />);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("等待旧扫描退出时消费刷新意图，接续扫描完成后不重复读取", () => {
    state = { ...state, loading: true, waiting: true };
    const { rerender } = render(<NodeInventoryPanel refreshRequest={0} />);
    rerender(<NodeInventoryPanel refreshRequest={1} />);
    expect(screen.getByRole("status").textContent).toContain("正在等待上次读取结束");
    state = { ...state, loading: true, waiting: false };
    rerender(<NodeInventoryPanel refreshRequest={1} />);
    state = { ...state, loading: false };
    rerender(<NodeInventoryPanel refreshRequest={1} />);
    expect(refresh).not.toHaveBeenCalled();
  });
});
