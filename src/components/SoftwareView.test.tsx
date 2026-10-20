// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  detectSystemInstalled,
  getEnvInfo,
  installSystemPackage,
  listTools,
  onSysInstallProgress,
  packageCatalog,
  type ProviderPackage,
  type ToolInfo,
} from "../api";
import SoftwareView from "./SoftwareView";

vi.mock("../api", () => ({
  detectSystemInstalled: vi.fn(),
  getEnvInfo: vi.fn(),
  installSystemPackage: vi.fn(),
  listTools: vi.fn(),
  onSysInstallProgress: vi.fn(),
  packageCatalog: vi.fn(),
  errorMessage: (error: unknown) => String(error),
}));
vi.mock("../data/catalog", () => ({
  CAT_KEYS_ALL: ["语言", "编辑器"],
  KNOWN_RUNTIMES: ["node"],
  SOFTWARE: [
    { name: "visual-studio-code", desc: "代码编辑器", url: "https://code.visualstudio.com" },
  ],
  TOOL_ICON_PATHS: {},
  categoryOf: (name: string) => (name === "node" ? "语言" : "编辑器"),
}));
vi.mock("./InstallDialog", () => ({
  default: ({ tool }: { tool: ToolInfo }) => (
    <div role="dialog" aria-label="运行时安装">
      {tool.name} {tool.versions.map((version) => version.version).join(", ")}
    </div>
  ),
}));

const packages: ProviderPackage[] = [
  { tool: "node", manager: "brew", id: "node@22", kind: "formula", platforms: ["macos"] },
  {
    tool: "visual-studio-code",
    manager: "winget",
    id: "Microsoft.VisualStudioCode",
    kind: "package",
    platforms: ["windows"],
  },
];

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(packageCatalog).mockResolvedValue(packages);
  vi.mocked(listTools).mockResolvedValue([]);
  vi.mocked(getEnvInfo).mockResolvedValue({
    mise_version: "2026.10.0",
    git_version: "2.50.0",
    os: { name: "Windows", version: "11", arch: "x64" },
    pkg: [
      { name: "brew", available: true, version: "5" },
      { name: "winget", available: true, version: "1" },
    ],
  });
  vi.mocked(detectSystemInstalled).mockResolvedValue([]);
  vi.mocked(installSystemPackage).mockResolvedValue("installed");
  vi.mocked(onSysInstallProgress).mockResolvedValue(vi.fn());
});
afterEach(cleanup);

async function ready() {
  await screen.findByRole("button", { name: "winget 安装 visual-studio-code" });
  await waitFor(() =>
    expect(
      (screen.getByRole("button", { name: "brew 安装 node" }) as HTMLButtonElement).disabled,
    ).toBe(false),
  );
}

describe("支持列表的安装与状态反馈", () => {
  it("mise 查询失败仍可从 Node 入口进入本机实例详情，不调用安装", async () => {
    const user = userEvent.setup();
    const showNode = vi.fn();
    vi.mocked(listTools).mockRejectedValue(new Error("mise unavailable"));
    render(<SoftwareView onShowNode={showNode} />);
    await screen.findByText("运行时安装状态检测失败");
    const entry = screen.getByRole("button", { name: "查看 node 的本机实例" });
    expect((entry as HTMLButtonElement).disabled).toBe(false);
    entry.focus();
    await user.keyboard("{Enter}");
    expect(showNode).toHaveBeenCalledTimes(1);
    expect(installSystemPackage).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByText("查看本机实例")).toBeTruthy();
  });

  it("复制按钮可用键盘触发，保留规范包 ID 且不触发卡片或安装", async () => {
    const user = userEvent.setup();
    const clipboard = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    const { container } = render(<SoftwareView />);
    await ready();
    const copy = screen.getByRole("button", { name: "复制 visual-studio-code 的 winget 安装命令" });
    copy.focus();
    await user.keyboard("{Enter}");
    expect(clipboard).toHaveBeenCalledWith(
      "winget install --id Microsoft.VisualStudioCode --exact",
    );
    expect(await screen.findByText("已复制 visual-studio-code 的 winget 安装命令")).toBeTruthy();
    expect(installSystemPackage).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(container.querySelector("button button")).toBeNull();
  });

  it("安装只提交一次规范包 ID，任务期间禁用冲突入口并在结束后重新检测", async () => {
    const user = userEvent.setup();
    const changed = vi.fn();
    let finish!: (value: string) => void;
    const unlisten = vi.fn();
    vi.mocked(onSysInstallProgress).mockResolvedValue(unlisten);
    vi.mocked(installSystemPackage).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const { rerender } = render(<SoftwareView onInventoryChanged={changed} />);
    await ready();
    screen.getByRole("button", { name: "winget 安装 visual-studio-code" }).focus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(installSystemPackage).toHaveBeenCalledTimes(1));
    expect(installSystemPackage).toHaveBeenCalledWith(
      "winget",
      "Microsoft.VisualStudioCode",
      "package",
      expect.any(String),
    );
    expect(
      (screen.getByRole("button", { name: "查看 node 的运行时版本" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(
      (
        screen.getByRole("button", {
          name: "查看 visual-studio-code 的安装渠道",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "brew 安装 node" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "复制 node 的 brew 安装命令" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    await user.click(screen.getByRole("button", { name: "brew 安装 node" }));
    expect(installSystemPackage).toHaveBeenCalledTimes(1);
    const jobId = vi.mocked(installSystemPackage).mock.calls[0][3];
    act(() =>
      vi.mocked(onSysInstallProgress).mock.calls[0][0]({
        jobId,
        manager: "winget",
        name: "Microsoft.VisualStudioCode",
        line: "正在下载 VS Code",
      }),
    );
    expect(screen.getByText("正在下载 VS Code")).toBeTruthy();
    vi.mocked(detectSystemInstalled).mockImplementation(async (manager) =>
      manager === "winget" ? ["microsoft.visualstudiocode"] : [],
    );
    expect(changed).not.toHaveBeenCalled();
    rerender(<SoftwareView active={false} onInventoryChanged={changed} />);
    await act(async () => finish("installed"));
    expect(changed).toHaveBeenCalledTimes(1);
    rerender(<SoftwareView active onInventoryChanged={changed} />);
    const installed = await screen.findByRole("button", {
      name: "winget 已安装 visual-studio-code",
    });
    expect((installed as HTMLButtonElement).disabled).toBe(true);
    expect(unlisten).toHaveBeenCalledOnce();
    expect(listTools).toHaveBeenCalledTimes(2);
  });

  it("检测失败显示未知状态并允许独立重试，恢复后将真实版本传给安装弹窗", async () => {
    const user = userEvent.setup();
    vi.mocked(listTools).mockRejectedValue(new Error("mise 不可用"));
    vi.mocked(detectSystemInstalled).mockImplementation(async (manager) => {
      if (manager === "winget") throw new Error("winget 检测超时");
      return [];
    });
    render(<SoftwareView />);
    const runtimeError = await screen.findByText("运行时安装状态检测失败");
    const systemError = await screen.findByText("winget 安装状态检测失败");
    expect(
      within(screen.getByRole("article", { name: "node" })).getByText("状态未知"),
    ).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "查看 node 的运行时版本" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(
      (
        screen.getByRole("button", {
          name: "winget 检测失败 visual-studio-code",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    vi.mocked(listTools).mockResolvedValue([
      {
        name: "node",
        versions: [{ version: "22.10.0", installed: true }],
        active_versions: ["22.10.0"],
      },
    ]);
    vi.mocked(detectSystemInstalled).mockResolvedValue([]);
    await user.click(
      within(runtimeError.closest('[role="alert"]') as HTMLElement).getByRole("button", {
        name: "重试",
      }),
    );
    await user.click(
      within(systemError.closest('[role="alert"]') as HTMLElement).getByRole("button", {
        name: "重试",
      }),
    );
    await ready();
    expect(within(screen.getByRole("article", { name: "node" })).getByText("已安装")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "查看 node 的运行时版本" }));
    expect(screen.getByRole("dialog", { name: "运行时安装" }).textContent).toContain("22.10.0");
  });

  it("安装渠道加载失败可在弹窗重试，官方下载始终可访问", async () => {
    const user = userEvent.setup();
    vi.mocked(packageCatalog).mockRejectedValueOnce(new Error("目录不可用"));
    render(<SoftwareView />);
    await screen.findByText("安装渠道加载失败");
    await user.click(screen.getByRole("button", { name: "查看 visual-studio-code 的安装渠道" }));
    const dialog = screen.getByRole("dialog", { name: "visual-studio-code" });
    expect(within(dialog).getByRole("link", { name: "前往官方下载" }).getAttribute("href")).toBe(
      "https://code.visualstudio.com",
    );
    expect(within(dialog).getByText("安装渠道暂不可用")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "重试" }));
    expect(
      await within(dialog).findByRole("button", {
        name: "复制 visual-studio-code 的 winget 安装命令",
      }),
    ).toBeTruthy();
  });

  it("包管理器检测失败不会伪装成没有渠道，重试后恢复安装入口", async () => {
    const user = userEvent.setup();
    vi.mocked(getEnvInfo).mockRejectedValueOnce(new Error("环境检测失败"));
    render(<SoftwareView />);
    const failure = await screen.findByText("包管理器检测失败");
    expect(screen.queryByRole("button", { name: "winget 安装 visual-studio-code" })).toBeNull();
    await user.click(
      within(failure.closest('[role="alert"]') as HTMLElement).getByRole("button", {
        name: "重试",
      }),
    );
    await ready();
    expect(screen.queryByText("包管理器检测失败")).toBeNull();
  });

  it("剪贴板拒绝时显示可手动复制的完整命令", async () => {
    const user = userEvent.setup();
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
    render(<SoftwareView />);
    await ready();
    await user.click(screen.getByRole("button", { name: "复制 node 的 brew 安装命令" }));
    expect(await screen.findByText("复制失败，请手动选择下方命令复制")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("brew install --formula node@22");
    expect(installSystemPackage).not.toHaveBeenCalled();
  });

  it("支持空白归一化和多个关键词，空态可一次清除搜索与类型筛选", async () => {
    const user = userEvent.setup();
    render(<SoftwareView />);
    await ready();
    const search = screen.getByRole("searchbox", { name: "搜索支持的软件或运行时" });
    await user.type(search, "  MICROSOFT   编辑器  ");
    expect(screen.getAllByRole("article")).toHaveLength(1);
    expect(screen.getByRole("article", { name: "visual-studio-code" })).toBeTruthy();
    await user.selectOptions(screen.getByRole("combobox", { name: "工具类型" }), "runtime");
    expect(screen.getByText("没有找到匹配的工具")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "清除筛选" }));
    expect((search as HTMLInputElement).value).toBe("");
    expect((screen.getByRole("combobox", { name: "工具类型" }) as HTMLSelectElement).value).toBe(
      "all",
    );
    expect(screen.getAllByRole("article")).toHaveLength(2);
  });
});
