// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixture from "../../fixtures/tool-catalog/contract-v1.json";
import { toolCatalog, validateToolCatalog } from "../api/toolCatalog";
import ToolCatalogPanel from "./ToolCatalogPanel";
vi.mock("../api/toolCatalog", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api/toolCatalog")>()),
  toolCatalog: vi.fn(),
}));
const props = {
  active: true,
  read: { snapshot: null, loading: false, error: null },
  onShowNode: vi.fn(),
  onShowCompatibility: vi.fn(),
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(toolCatalog).mockResolvedValue(validateToolCatalog(fixture[0]));
});
afterEach(cleanup);
it("目录默认显示未知而非未安装，仅用户点击才打开已有 Node 或兼容入口", async () => {
  render(<ToolCatalogPanel {...props} />);
  await screen.findByRole("heading", { name: "Python 语言与运行时" });
  expect(props.onShowNode).not.toHaveBeenCalled();
  expect(props.onShowCompatibility).not.toHaveBeenCalled();
  expect(screen.getAllByText("安装状态未知")).toHaveLength(7);
  fireEvent.click(screen.getByRole("button", { name: "查看 Node 本机实例与计划" }));
  expect(props.onShowNode).toHaveBeenCalledOnce();
  fireEvent.click(screen.getAllByRole("button", { name: "查看兼容渠道与手动入口" })[0]);
  expect(props.onShowCompatibility).toHaveBeenCalledOnce();
});
it("搜索命令名不改变工具身份，空结果可清除并且不宣称未安装", async () => {
  render(<ToolCatalogPanel {...props} />);
  await screen.findByRole("heading", { name: "Python 语言与运行时" });
  fireEvent.change(screen.getByRole("searchbox", { name: "搜索工具" }), {
    target: { value: "javac" },
  });
  expect(screen.getByRole("heading", { name: "Java 语言与运行时" })).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "Node.js 语言与运行时" })).toBeNull();
  fireEvent.change(screen.getByRole("combobox", { name: "实例证据" }), {
    target: { value: "observed" },
  });
  expect(screen.getByText(/这不表示系统未安装/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "清除筛选" }));
  expect(screen.getByText("显示 7 / 7 项工具定义")).toBeTruthy();
});
it("能力详情逐来源给出原因，nvm 不获得安装或卸载计划", async () => {
  render(<ToolCatalogPanel {...props} />);
  await screen.findByText("显示 7 / 7 项工具定义");
  fireEvent.click(screen.getByText("Node.js 接入范围"));
  const nvm = screen.getByRole("region", { name: "Node.js nvm 接入范围" });
  expect(within(nvm).getByText("安装计划：当前不支持")).toBeTruthy();
  expect(within(nvm).getByText("不从目录观测推导卸载权限")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "执行安装" })).toBeNull();
});
it("刷新失败保留旧目录并停用入口，重试恢复；隐藏期间迟到结果不能改写新会话", async () => {
  const { rerender } = render(<ToolCatalogPanel {...props} />);
  await screen.findByText("显示 7 / 7 项工具定义");
  vi.mocked(toolCatalog).mockRejectedValueOnce(new Error("offline"));
  fireEvent.click(screen.getByRole("button", { name: "重新读取目录" }));
  expect((await screen.findByRole("alert")).textContent).toContain("offline");
  expect(
    (screen.getByRole("button", { name: "查看 Node 本机实例与计划" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  let finish!: (v: ReturnType<typeof validateToolCatalog>) => void;
  vi.mocked(toolCatalog).mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "重新读取目录" }));
  rerender(<ToolCatalogPanel {...props} active={false} />);
  rerender(<ToolCatalogPanel {...props} />);
  await screen.findByRole("button", { name: "查看 Node 本机实例与计划" });
  await act(async () => finish(validateToolCatalog(fixture[1])));
  expect(screen.getByText(/当前平台：macOS/)).toBeTruthy();
});
it("其他平台展示不支持原因，不提供 Node 统一流程入口", async () => {
  vi.mocked(toolCatalog).mockResolvedValueOnce(validateToolCatalog(fixture[1]));
  render(<ToolCatalogPanel {...props} />);
  await screen.findByText(/当前平台：Windows/);
  expect(screen.queryByRole("button", { name: "查看 Node 本机实例与计划" })).toBeNull();
  fireEvent.click(screen.getByText("Node.js 接入范围"));
  expect(screen.getAllByText("当前平台尚未接入统一 Node 流程").length).toBeGreaterThan(0);
});
it("Python 使用自己的覆盖与实例记录，不借用 Node 记录；点击才进入 Python 清单", async () => {
  const pythonFixture = (await import("../../fixtures/python-inventory/contract-v1.json")).default;
  const onShowPython = vi.fn();
  render(
    <ToolCatalogPanel
      {...props}
      onShowPython={onShowPython}
      pythonRead={{
        snapshot: pythonFixture as unknown as import("../api/inventory").InventorySnapshot,
        loading: false,
        error: null,
      }}
    />,
  );
  const button = await screen.findByRole("button", { name: "查看 Python 本机实例" });
  expect(onShowPython).not.toHaveBeenCalled();
  expect(screen.getByText("已观察到 2 个实例")).toBeTruthy();
  fireEvent.click(button);
  expect(onShowPython).toHaveBeenCalledOnce();
});
