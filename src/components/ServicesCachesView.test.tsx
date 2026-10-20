// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cacheClean, cacheList, serviceAction, serviceList, type CacheInfo } from "../api";
import ServicesCachesView from "./ServicesCachesView";

vi.mock("../api", () => ({
  serviceList: vi.fn(),
  serviceAction: vi.fn(),
  cacheList: vi.fn(),
  cacheClean: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

const npmCache: CacheInfo = {
  id: "npm",
  name: "npm",
  path: "/Users/developer/.npm",
  exists: true,
  sizeBytes: 1024 ** 3,
  approx: false,
  cleanable: true,
};

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(serviceList).mockResolvedValue({
    services: [{ name: "redis", state: "started", manager: "brew", pid: 123 }],
    notes: [],
  });
  vi.mocked(cacheList).mockResolvedValue([npmCache]);
  vi.mocked(cacheClean).mockResolvedValue("已清理 npm 缓存");
  vi.mocked(serviceAction).mockResolvedValue("已完成");
});

afterEach(cleanup);

it("缓存读取失败独立显示错误并可重试，不影响已读取的服务", async () => {
  vi.mocked(cacheList).mockRejectedValueOnce(new Error("缓存目录暂时无法读取"));
  const user = userEvent.setup();
  render(<ServicesCachesView />);

  expect(await screen.findByRole("button", { name: "重启 redis" })).toBeTruthy();
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    expect.stringContaining("缓存目录暂时无法读取"),
  );
  expect(screen.queryByText("暂无开发缓存")).toBeNull();

  await user.click(screen.getByRole("button", { name: "重试缓存" }));
  expect(await screen.findByRole("button", { name: "清理 npm 缓存" })).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(cacheList).toHaveBeenCalledTimes(2);
  expect(serviceList).toHaveBeenCalledTimes(1);
});

it("刷新失败保留原有缓存数据并说明数据尚未更新", async () => {
  const user = userEvent.setup();
  render(<ServicesCachesView />);
  await screen.findByRole("button", { name: "清理 npm 缓存" });
  let rejectRefresh!: (reason: Error) => void;
  vi.mocked(cacheList).mockImplementationOnce(
    () =>
      new Promise((_resolve, reject) => {
        rejectRefresh = reject;
      }),
  );

  await user.click(screen.getByRole("button", { name: "刷新缓存" }));
  expect(screen.getByText(npmCache.path)).toBeTruthy();
  expect(screen.queryByText("正在统计开发缓存")).toBeNull();
  expect(screen.getByRole("button", { name: "清理 npm 缓存" })).toHaveProperty("disabled", true);

  await act(async () => rejectRefresh(new Error("读取超时")));
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    expect.stringContaining("当前展示上次读取的结果"),
  );
  expect(screen.getByText(npmCache.path)).toBeTruthy();
  expect(screen.getByRole("button", { name: "清理 npm 缓存" })).toHaveProperty("disabled", false);
});

it("清理确认展示体积与官方命令，取消后不会调用清理且恢复触发按钮焦点", async () => {
  const user = userEvent.setup();
  render(<ServicesCachesView />);
  const trigger = await screen.findByRole("button", { name: "清理 npm 缓存" });

  await user.click(trigger);
  const dialog = screen.getByRole("dialog", { name: "清理 npm 缓存" });
  expect(within(dialog).getByText("1.00 GB")).toBeTruthy();
  expect(within(dialog).getByText("npm cache clean --force")).toBeTruthy();
  expect(within(dialog).getByText(npmCache.path)).toBeTruthy();
  expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "取消" }));

  await user.click(within(dialog).getByRole("button", { name: "取消" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(cacheClean).not.toHaveBeenCalled();
  await waitFor(() => expect(document.activeElement).toBe(trigger));
});

it("确认清理期间阻止重复提交和关闭，完成后重新读取缓存", async () => {
  let finishClean!: (message: string) => void;
  vi.mocked(cacheClean).mockImplementation(
    () =>
      new Promise((resolve) => {
        finishClean = resolve;
      }),
  );
  const user = userEvent.setup();
  render(<ServicesCachesView />);
  await user.click(await screen.findByRole("button", { name: "清理 npm 缓存" }));
  await user.dblClick(screen.getByRole("button", { name: "确认清理" }));

  expect(cacheClean).toHaveBeenCalledExactlyOnceWith("npm");
  expect(screen.getByRole("button", { name: "取消" })).toHaveProperty("disabled", true);
  expect(screen.getByRole("button", { name: "清理中…" })).toHaveProperty("disabled", true);
  await user.keyboard("{Escape}");
  expect(screen.getByRole("dialog")).toBeTruthy();

  vi.mocked(cacheList).mockResolvedValue([{ ...npmCache, sizeBytes: 0 }]);
  await act(async () => finishClean("已清理 npm 缓存"));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(cacheList).toHaveBeenCalledTimes(2);
  expect(screen.getByText("npm 的清理命令已完成。")).toBeTruthy();
  expect(within(screen.getByRole("list", { name: "开发缓存列表" })).getByText("0 B")).toBeTruthy();
  await waitFor(() => {
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "清理 npm 缓存" }));
  });
});

it("清理失败保留确认上下文，可以重试并再次提交相同缓存", async () => {
  vi.mocked(cacheClean).mockRejectedValueOnce(new Error("npm 正在被另一个进程使用"));
  const user = userEvent.setup();
  render(<ServicesCachesView />);
  await user.click(await screen.findByRole("button", { name: "清理 npm 缓存" }));
  await user.click(screen.getByRole("button", { name: "确认清理" }));

  const dialog = screen.getByRole("dialog", { name: "清理 npm 缓存" });
  expect(await within(dialog).findByRole("alert")).toHaveProperty(
    "textContent",
    expect.stringContaining("npm 正在被另一个进程使用"),
  );
  expect(cacheList).toHaveBeenCalledTimes(1);
  await user.click(within(dialog).getByRole("button", { name: "重试清理" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(cacheClean).toHaveBeenCalledTimes(2);
  expect(vi.mocked(cacheClean).mock.calls).toEqual([["npm"], ["npm"]]);
});

it("服务操作失败保留原状态与可重试按钮，成功后刷新服务列表", async () => {
  vi.mocked(serviceAction).mockRejectedValueOnce(new Error("权限不足"));
  const user = userEvent.setup();
  render(<ServicesCachesView />);
  await user.click(await screen.findByRole("button", { name: "停止 redis" }));
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    expect.stringContaining("redis 停止失败：权限不足"),
  );
  expect(screen.getByText("运行中")).toBeTruthy();
  expect(screen.getByRole("button", { name: "清理 npm 缓存" })).toHaveProperty("disabled", false);

  vi.mocked(serviceList).mockResolvedValue({
    services: [{ name: "redis", state: "stopped", manager: "brew", pid: null }],
    notes: [],
  });
  await user.click(screen.getByRole("button", { name: "停止 redis" }));
  expect(await screen.findByRole("button", { name: "启动 redis" })).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(serviceAction).toHaveBeenNthCalledWith(2, "brew", "redis", "stop");
  expect(serviceList).toHaveBeenCalledTimes(2);
});
