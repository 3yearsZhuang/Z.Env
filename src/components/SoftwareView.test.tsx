// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
import { toolCatalog } from "../api/toolCatalog";
import { clearEnvInfoCache } from "../lib/envInfo";
import { clearToolCatalogCache } from "../lib/toolCatalogCache";
import type { ToolCatalog } from "../api/toolCatalog";
import SoftwareView from "./SoftwareView";
import type { InventoryInstallTarget } from "../lib/inventoryNavigation";

vi.mock("../api/toolCatalog", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api/toolCatalog")>()),
  toolCatalog: vi.fn(),
}));
vi.mock("../api", () => ({
  detectSystemInstalled: vi.fn(),
  getEnvInfo: vi.fn(),
  installSystemPackage: vi.fn(),
  listTools: vi.fn(),
  onSysInstallProgress: vi.fn(),
  packageCatalog: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));
vi.mock("../data/catalog", () => ({
  CAT_KEYS_ALL: ["语言", "编辑器"],
  KNOWN_RUNTIMES: ["node", "python", "ruby"],
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

const nodePythonPackages: ProviderPackage[] = [
  ...packages,
  { tool: "python", manager: "brew", id: "python@3.13", kind: "formula", platforms: ["macos"] },
];

const catalog: ToolCatalog = {
  schemaVersion: 1,
  platform: "macos",
  coverageNote: "首批 7 项工具定义",
  tools: [
    {
      definition: {
        toolId: "node",
        displayName: "Node.js",
        aliases: ["nodejs"],
        executableNames: ["node"],
      },
      category: "runtime",
      description: "JavaScript 运行时",
      documentationUrl: "https://nodejs.org/en",
      projectCheck: { state: "integrated", reason: "已接入项目核对" },
      channels: [
        {
          providerId: "brew",
          packages: [],
          inventory: { state: "integrated", reason: "安装收据与目录" },
          installPlan: { state: "integrated", reason: "可生成安装计划" },
          uninstallPlan: { state: "integrated", reason: "可生成卸载计划" },
          bindingPlan: { state: "integrated", reason: "托管模式可绑定" },
        },
        {
          providerId: "nvm",
          packages: [],
          inventory: { state: "integrated", reason: "版本目录" },
          installPlan: { state: "notIntegrated", reason: "nvm 不提供安装计划，请用 nvm 自己安装" },
          uninstallPlan: { state: "notIntegrated", reason: "nvm 不提供卸载计划" },
          bindingPlan: { state: "notIntegrated", reason: "nvm 只提供只读观测" },
        },
      ],
    },
  ],
};

const previewCatalog: ToolCatalog = {
  ...catalog,
  tools: (["node", "python"] as const).map((toolId) => ({
    ...catalog.tools[0],
    definition: {
      toolId,
      displayName: toolId === "node" ? "Node.js" : "Python",
      aliases: toolId === "node" ? ["nodejs"] : [],
      executableNames: [toolId],
    },
    channels: (toolId === "node"
      ? (["mise", "brew"] as const)
      : (["mise", "brew", "pyenv", "uv"] as const)
    ).map((providerId) => ({
      ...catalog.tools[0].channels[0],
      providerId,
      packages: [
        {
          providerId,
          packageId:
            providerId === "brew" ? (toolId === "node" ? "node@22" : "python@3.13") : toolId,
          kind: providerId === "brew" ? ("formula" as const) : ("runtime" as const),
          platform: "macos" as const,
        },
      ],
    })),
  })),
};

beforeEach(() => {
  clearEnvInfoCache();
  clearToolCatalogCache();
  vi.resetAllMocks();
  vi.mocked(toolCatalog).mockResolvedValue(catalog);
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
      (
        screen.getByRole("button", {
          name: "winget 安装 visual-studio-code",
        }) as HTMLButtonElement
      ).disabled,
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
    expect(
      within(screen.getByRole("article", { name: "node" })).getByText("查看本机实例"),
    ).toBeTruthy();
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
      (screen.getByRole("button", { name: "查看 node 的本机实例" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (
        screen.getByRole("button", {
          name: "查看 visual-studio-code 的安装渠道",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "brew 预览计划 node" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "复制 node 的 brew 安装命令" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    await user.click(screen.getByRole("button", { name: "brew 预览计划 node" }));
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
      within(screen.getByRole("article", { name: "ruby" })).getByText("状态未知"),
    ).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "查看 ruby 的运行时版本" }) as HTMLButtonElement)
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
        name: "ruby",
        versions: [{ version: "3.4.0", installed: true }],
        active_versions: ["3.4.0"],
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
    expect(within(screen.getByRole("article", { name: "ruby" })).getByText("已安装")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "查看 ruby 的运行时版本" }));
    expect(screen.getByRole("dialog", { name: "运行时安装" }).textContent).toContain("3.4.0");
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
    expect(screen.getAllByRole("article")).toHaveLength(4);
  });
});

describe("Node 与 Python 卡片的清单和安装预览", () => {
  it("目录迟到时先就绪的 Node/Python 包渠道保持禁用，目录到达后仅走原生包计划", async () => {
    const user = userEvent.setup();
    const preview = vi.fn();
    let resolveCatalog!: (value: ToolCatalog) => void;
    vi.mocked(toolCatalog).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveCatalog = resolve;
        }),
    );
    vi.mocked(packageCatalog).mockResolvedValue(nodePythonPackages);
    render(<SoftwareView onPreviewInstall={preview} />);
    await ready();
    expect(detectSystemInstalled).toHaveBeenCalledWith("brew");

    for (const toolId of ["node", "python"] as const) {
      const button = screen.getByRole("button", { name: `brew 能力读取中 ${toolId}` });
      expect((button as HTMLButtonElement).disabled).toBe(true);
      await user.click(button);
    }
    expect(installSystemPackage).not.toHaveBeenCalled();
    expect(preview).not.toHaveBeenCalled();

    await act(async () => resolveCatalog(previewCatalog));
    for (const toolId of ["node", "python"] as const) {
      await user.click(screen.getByRole("button", { name: `brew 预览计划 ${toolId}` }));
    }
    expect(preview.mock.calls).toEqual([
      [
        { toolId: "node", providerId: "brew", packageId: "node@22" },
        screen.getByRole("button", { name: "brew 预览计划 node" }),
      ],
      [
        { toolId: "python", providerId: "brew", packageId: "python@3.13" },
        screen.getByRole("button", { name: "brew 预览计划 python" }),
      ],
    ]);
    expect(installSystemPackage).not.toHaveBeenCalled();
  });

  it.each(["tool", "channel"] as const)(
    "已读取目录缺少 Node %s 时能力仍未知，不推断为未接入而直接安装",
    async (missing) => {
      const user = userEvent.setup();
      vi.mocked(toolCatalog).mockResolvedValue({
        ...previewCatalog,
        tools:
          missing === "tool"
            ? previewCatalog.tools.filter((tool) => tool.definition.toolId !== "node")
            : previewCatalog.tools.map((tool) =>
                tool.definition.toolId === "node"
                  ? {
                      ...tool,
                      channels: tool.channels.filter((channel) => channel.providerId !== "brew"),
                    }
                  : tool,
              ),
      });
      render(<SoftwareView onPreviewInstall={vi.fn()} />);
      await ready();
      const button = screen.getByRole("button", { name: "brew 能力未知 node" });
      expect((button as HTMLButtonElement).disabled).toBe(true);
      await user.click(button);
      expect(installSystemPackage).not.toHaveBeenCalled();
    },
  );

  it.each(["notIntegrated", "unsupported"] as const)(
    "目录明确标注 %s 的来源保留兼容安装并传递原生包 ID",
    async (state) => {
      const user = userEvent.setup();
      const preview = vi.fn();
      vi.mocked(toolCatalog).mockResolvedValue({
        ...catalog,
        tools: catalog.tools.map((tool) => ({
          ...tool,
          channels: tool.channels.map((channel) => ({
            ...channel,
            installPlan: { state, reason: "此来源未接入安装计划" },
          })),
        })),
      });
      render(<SoftwareView onPreviewInstall={preview} />);
      await ready();
      await user.click(screen.getByRole("button", { name: "brew 安装 node" }));
      await waitFor(() => expect(installSystemPackage).toHaveBeenCalledTimes(1));
      expect(installSystemPackage).toHaveBeenCalledWith(
        "brew",
        "node@22",
        "formula",
        expect.any(String),
      );
      expect(preview).not.toHaveBeenCalled();
    },
  );

  it("旧 mise 查询失败时，Node 和 Python 仍对称进入清单", async () => {
    const user = userEvent.setup();
    const showInventory = vi.fn();
    vi.mocked(listTools).mockRejectedValue(new Error("mise unavailable"));
    render(<SoftwareView onShowInventory={showInventory} />);
    await screen.findByText("运行时安装状态检测失败");

    for (const toolId of ["node", "python"] as const) {
      const button = screen.getByRole("button", { name: `查看 ${toolId} 的本机实例` });
      expect((button as HTMLButtonElement).disabled).toBe(false);
      button.focus();
      await user.keyboard("{Enter}");
    }
    expect(showInventory.mock.calls).toEqual([["node"], ["python"]]);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(installSystemPackage).not.toHaveBeenCalled();
  });

  it("默认 mise 安装预览不依赖旧版本列表和系统渠道检测", async () => {
    const user = userEvent.setup();
    const preview = vi.fn();
    vi.mocked(toolCatalog).mockResolvedValue(previewCatalog);
    vi.mocked(listTools).mockRejectedValue(new Error("mise unavailable"));
    vi.mocked(getEnvInfo).mockRejectedValue(new Error("环境检测失败"));
    vi.mocked(packageCatalog).mockRejectedValue(new Error("旧包目录不可用"));
    render(<SoftwareView onPreviewInstall={preview} />);

    for (const toolId of ["node", "python"] as const) {
      const button = await screen.findByRole("button", { name: `mise 预览计划 ${toolId}` });
      expect((button as HTMLButtonElement).disabled).toBe(false);
      await user.click(button);
    }
    expect(preview.mock.calls).toEqual([
      [
        { toolId: "node", providerId: "mise", packageId: "node" },
        screen.getByRole("button", { name: "mise 预览计划 node" }),
      ],
      [
        { toolId: "python", providerId: "mise", packageId: "python" },
        screen.getByRole("button", { name: "mise 预览计划 python" }),
      ],
    ]);
    expect(installSystemPackage).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it.each(["installed", "failed"] as const)(
    "brew 旧检测为 %s 时仍可预览，保留 Node/Python 原生包 ID",
    async (state) => {
      const user = userEvent.setup();
      const preview = vi.fn();
      vi.mocked(toolCatalog).mockResolvedValue(previewCatalog);
      vi.mocked(packageCatalog).mockResolvedValue(nodePythonPackages);
      vi.mocked(detectSystemInstalled).mockImplementation(async (manager) => {
        if (manager !== "brew") return [];
        if (state === "failed") throw new Error("brew 检测失败");
        return ["node@22", "python@3.13"];
      });
      render(<SoftwareView onPreviewInstall={preview} />);
      await ready();

      for (const toolId of ["node", "python"] as const) {
        const button = screen.getByRole("button", { name: `brew 预览计划 ${toolId}` });
        expect((button as HTMLButtonElement).disabled).toBe(false);
        await user.click(button);
      }
      expect(preview.mock.calls).toEqual([
        [
          { toolId: "node", providerId: "brew", packageId: "node@22" },
          screen.getByRole("button", { name: "brew 预览计划 node" }),
        ],
        [
          { toolId: "python", providerId: "brew", packageId: "python@3.13" },
          screen.getByRole("button", { name: "brew 预览计划 python" }),
        ],
      ]);
      expect(installSystemPackage).not.toHaveBeenCalled();
    },
  );

  it("pyenv 和 uv 使用目录声明的来源及包身份，展开预览不会执行安装", async () => {
    const user = userEvent.setup();
    const preview = vi.fn();
    vi.mocked(toolCatalog).mockResolvedValue(previewCatalog);
    render(<SoftwareView onPreviewInstall={preview} />);
    await ready();
    const python = screen.getByRole("article", { name: "python" });
    await user.click(within(python).getByText("来源与能力"));
    for (const providerId of ["pyenv", "uv"] as const) {
      await user.click(
        within(python).getByRole("button", { name: `${providerId} 预览计划 python python` }),
      );
    }
    expect(preview.mock.calls).toEqual([
      [
        { toolId: "python", providerId: "pyenv", packageId: "python" },
        screen.getByRole("button", { name: "pyenv 预览计划 python python" }),
      ],
      [
        { toolId: "python", providerId: "uv", packageId: "python" },
        screen.getByRole("button", { name: "uv 预览计划 python python" }),
      ],
    ]);
    expect(installSystemPackage).not.toHaveBeenCalled();
  });

  it("复制、清单和预览是独立按钮，复制不会触发另两个操作", async () => {
    const user = userEvent.setup();
    const clipboard = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    const showInventory = vi.fn();
    const preview = vi.fn();
    const { container } = render(
      <SoftwareView onShowInventory={showInventory} onPreviewInstall={preview} />,
    );
    await ready();
    await user.click(screen.getByRole("button", { name: "复制 node 的 brew 安装命令" }));
    expect(clipboard).toHaveBeenCalledWith("brew install --formula node@22");
    expect(showInventory).not.toHaveBeenCalled();
    expect(preview).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "查看 node 的本机实例" }));
    expect(showInventory).toHaveBeenCalledExactlyOnceWith("node");
    expect(preview).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "brew 预览计划 node" }));
    expect(preview).toHaveBeenCalledExactlyOnceWith(
      {
        toolId: "node",
        providerId: "brew",
        packageId: "node@22",
      } satisfies InventoryInstallTarget,
      screen.getByRole("button", { name: "brew 预览计划 node" }),
    );
    expect(showInventory).toHaveBeenCalledTimes(1);
    expect(installSystemPackage).not.toHaveBeenCalled();
    expect(container.querySelector("button button")).toBeNull();
  });

  it("未聚焦的鼠标点击把各来源的实际计划按钮透传为 opener，不依赖 activeElement", async () => {
    const preview = vi.fn();
    vi.mocked(toolCatalog).mockResolvedValue(previewCatalog);
    vi.mocked(packageCatalog).mockResolvedValue(nodePythonPackages);
    render(<SoftwareView onPreviewInstall={preview} />);
    await ready();
    const python = screen.getByRole("article", { name: "python" });
    fireEvent.click(within(python).getByText("来源与能力"));
    const actions: { label: string; target: InventoryInstallTarget }[] = [
      {
        label: "mise 预览计划 node",
        target: { toolId: "node", providerId: "mise", packageId: "node" },
      },
      {
        label: "mise 预览计划 python",
        target: { toolId: "python", providerId: "mise", packageId: "python" },
      },
      {
        label: "brew 预览计划 node",
        target: { toolId: "node", providerId: "brew", packageId: "node@22" },
      },
      {
        label: "brew 预览计划 python",
        target: { toolId: "python", providerId: "brew", packageId: "python@3.13" },
      },
      {
        label: "pyenv 预览计划 python python",
        target: { toolId: "python", providerId: "pyenv", packageId: "python" },
      },
      {
        label: "uv 预览计划 python python",
        target: { toolId: "python", providerId: "uv", packageId: "python" },
      },
    ];
    for (const { label, target } of actions) {
      const button = screen.getByRole("button", { name: label });
      expect(document.activeElement).toBe(document.body);
      // 图标点击会冒泡，currentTarget 必须仍是按钮，而不是 svg 或先前焦点。
      fireEvent.click(button.querySelector("svg") ?? button);
      expect(preview).toHaveBeenLastCalledWith(target, button);
      expect(document.activeElement).toBe(document.body);
    }
    expect(preview).toHaveBeenCalledTimes(actions.length);
    expect(installSystemPackage).not.toHaveBeenCalled();
  });

  it("缺少清单或计划回调时禁用入口，绝不回退为旧版安装", async () => {
    const user = userEvent.setup();
    vi.mocked(toolCatalog).mockResolvedValue(previewCatalog);
    render(<SoftwareView />);
    await ready();
    const python = screen.getByRole("article", { name: "python" });
    await user.click(within(python).getByText("来源与能力"));
    for (const label of [
      "查看 node 的本机实例",
      "查看 python 的本机实例",
      "mise 预览计划 node",
      "mise 预览计划 python",
      "brew 预览计划 node",
      "pyenv 预览计划 python python",
      "uv 预览计划 python python",
    ]) {
      const button = screen.getByRole("button", { name: label });
      expect((button as HTMLButtonElement).disabled).toBe(true);
      await user.click(button);
    }
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(installSystemPackage).not.toHaveBeenCalled();
  });

  it("未接入 mise 安装计划时不展示默认预览入口", async () => {
    vi.mocked(toolCatalog).mockResolvedValue({
      ...previewCatalog,
      tools: previewCatalog.tools.map((tool) => ({
        ...tool,
        channels: tool.channels.map((channel) => ({
          ...channel,
          installPlan: { state: "notIntegrated", reason: "只读来源" },
        })),
      })),
    });
    render(<SoftwareView onPreviewInstall={vi.fn()} />);
    await ready();
    expect(screen.queryByRole("button", { name: "mise 预览计划 node" })).toBeNull();
    expect(screen.queryByRole("button", { name: "mise 预览计划 python" })).toBeNull();
    expect(
      (screen.getByRole("button", { name: "brew 安装 node" }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });
});

describe("统一目录能力标注", () => {
  const nodePackages: ProviderPackage[] = [
    { tool: "node", manager: "brew", id: "node@22", kind: "formula", platforms: ["macos"] },
    {
      tool: "node",
      manager: "winget",
      id: "OpenJS.NodeJS",
      kind: "package",
      platforms: ["windows"],
    },
  ];

  it("目录收录的工具标「统一目录」，渠道行给出能否走计划", async () => {
    vi.mocked(packageCatalog).mockResolvedValue(nodePackages);
    render(<SoftwareView />);
    await screen.findByRole("button", { name: "brew 预览计划 node" });

    expect(screen.getByText("统一目录")).toBeTruthy();
    expect(screen.getByText("可计划安装")).toBeTruthy();
    // 未登记来源的能力仍未知，不能推断为未接入并直接写入。
    const wingetEntry = screen.getByRole("button", { name: "winget 能力未知 node" });
    expect((wingetEntry as HTMLButtonElement).disabled).toBe(true);
    expect(wingetEntry.getAttribute("title")).toContain("winget");

    // 不在统一目录里的工具只保留旧版渠道视图，不冒充已接入
    const vscode = screen.getByRole("article", { name: "visual-studio-code" });
    expect(within(vscode).queryByText("统一目录")).toBeNull();
    expect(within(vscode).queryByText("可计划安装")).toBeNull();
  });

  it("目录里只读的渠道标注「只读，未接入计划」并带出原因", async () => {
    vi.mocked(packageCatalog).mockResolvedValue([nodePackages[0]]);
    vi.mocked(toolCatalog).mockResolvedValue({
      ...catalog,
      tools: [
        {
          ...catalog.tools[0],
          channels: [
            {
              ...catalog.tools[0].channels[0],
              installPlan: { state: "notIntegrated", reason: "brew 渠道尚未接入安装计划" },
            },
          ],
        },
      ],
    });
    render(<SoftwareView />);
    await screen.findByRole("button", { name: "brew 安装 node" });

    const label = screen.getByText("只读，未接入计划");
    expect(label.getAttribute("title")).toContain("brew 渠道尚未接入安装计划");
  });

  it.each(["目录 IPC 不可用", "工具目录格式不受支持或能力记录不完整"])(
    "%s 时 Node/Python 渠道禁用，独立重试后恢复原生包计划且其他旧渠道可用",
    async (message) => {
      const user = userEvent.setup();
      const preview = vi.fn();
      vi.mocked(toolCatalog).mockRejectedValueOnce(new Error(message));
      vi.mocked(toolCatalog).mockResolvedValue(previewCatalog);
      vi.mocked(packageCatalog).mockResolvedValue(nodePythonPackages);
      const { unmount } = render(<SoftwareView onPreviewInstall={preview} />);
      await ready();

      const alert = await screen.findByRole("alert");
      expect(within(alert).getByText("工具能力读取失败")).toBeTruthy();
      expect(within(alert).getByText(message)).toBeTruthy();
      expect(screen.queryByText("统一目录")).toBeNull();
      for (const toolId of ["node", "python"] as const) {
        const button = screen.getByRole("button", { name: `brew 能力未知 ${toolId}` });
        expect((button as HTMLButtonElement).disabled).toBe(true);
        await user.click(button);
      }
      expect(preview).not.toHaveBeenCalled();
      expect(installSystemPackage).not.toHaveBeenCalled();

      await user.click(within(alert).getByRole("button", { name: "重试" }));
      expect(
        await within(screen.getByRole("article", { name: "node" })).findByText("统一目录"),
      ).toBeTruthy();
      for (const toolId of ["node", "python"] as const) {
        await user.click(screen.getByRole("button", { name: `brew 预览计划 ${toolId}` }));
      }
      expect(preview.mock.calls).toEqual([
        [
          { toolId: "node", providerId: "brew", packageId: "node@22" },
          screen.getByRole("button", { name: "brew 预览计划 node" }),
        ],
        [
          { toolId: "python", providerId: "brew", packageId: "python@3.13" },
          screen.getByRole("button", { name: "brew 预览计划 python" }),
        ],
      ]);
      expect(screen.queryByRole("alert")).toBeNull();
      expect(toolCatalog).toHaveBeenCalledTimes(2);
      expect(packageCatalog).toHaveBeenCalledTimes(1);
      expect(listTools).toHaveBeenCalledTimes(1);
      expect(getEnvInfo).toHaveBeenCalledTimes(1);

      // 成功目录在会话内复用：再次挂载不再读取 IPC。
      unmount();
      render(<SoftwareView />);
      expect(
        await within(screen.getByRole("article", { name: "node" })).findByText("统一目录"),
      ).toBeTruthy();
      expect(toolCatalog).toHaveBeenCalledTimes(2);
      expect(installSystemPackage).not.toHaveBeenCalled();
    },
  );

  it("目录首次失败后，刷新状态也会重新读取并恢复能力", async () => {
    const user = userEvent.setup();
    vi.mocked(toolCatalog).mockRejectedValueOnce(new Error("目录 IPC 不可用"));
    render(<SoftwareView />);
    await ready();
    expect(await screen.findByRole("alert")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "刷新状态" }));
    expect(await screen.findByText("统一目录")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toolCatalog).toHaveBeenCalledTimes(2);
    expect(installSystemPackage).not.toHaveBeenCalled();
  });

  it("来源与能力保留非系统包渠道、项目核对及清单安装卸载绑定状态和原因", async () => {
    const user = userEvent.setup();
    const python = {
      ...catalog.tools[0],
      definition: {
        toolId: "python",
        displayName: "Python",
        aliases: [],
        executableNames: ["python", "python3"],
      },
      projectCheck: { state: "integrated" as const, reason: "按 Python 项目声明核对候选" },
      channels: (["mise", "brew", "pyenv", "uv", "path"] as const).map((providerId) => ({
        providerId,
        packages: [],
        inventory: { state: "integrated" as const, reason: `${providerId} 清单读取保留文件证据` },
        installPlan: {
          state: providerId === "path" ? ("unsupported" as const) : ("integrated" as const),
          reason: `${providerId} 安装范围须预览核对`,
        },
        uninstallPlan: {
          state: providerId === "path" ? ("notIntegrated" as const) : ("integrated" as const),
          reason: `${providerId} 卸载须核对精确归属`,
        },
        bindingPlan: {
          state: providerId === "mise" ? ("integrated" as const) : ("notIntegrated" as const),
          reason: `${providerId} 绑定范围按作用域判断`,
        },
      })),
    };
    vi.mocked(toolCatalog).mockResolvedValue({ ...catalog, tools: [...catalog.tools, python] });
    render(<SoftwareView />);
    await ready();

    const card = screen.getByRole("article", { name: "python" });
    await user.click(await within(card).findByText("来源与能力"));
    expect(within(card).getByText("项目核对")).toBeTruthy();
    expect(within(card).getByText(python.projectCheck.reason)).toBeTruthy();
    for (const channel of python.channels) {
      for (const support of [
        channel.inventory,
        channel.installPlan,
        channel.uninstallPlan,
        channel.bindingPlan,
      ]) {
        expect(within(card).getByText(support.reason)).toBeTruthy();
      }
    }
    for (const label of ["清单", "安装", "卸载", "绑定"]) {
      expect(within(card).getAllByText(label)).toHaveLength(5);
    }
    for (const label of ["已接入", "未接入", "不支持"]) {
      expect(within(card).getAllByText(label).length).toBeGreaterThan(0);
    }
    expect(within(card).queryByRole("button", { name: "mise 安装 python" })).toBeNull();
    expect(installSystemPackage).not.toHaveBeenCalled();

    // Node 独有的 nvm 渠道也不依赖旧系统包映射才能显示。
    const node = screen.getByRole("article", { name: "node" });
    await user.click(within(node).getByText("来源与能力"));
    expect(within(node).getByText("nvm 不提供安装计划，请用 nvm 自己安装")).toBeTruthy();
    expect(within(node).getByText("nvm 不提供卸载计划")).toBeTruthy();
    expect(within(node).getByText("nvm 只提供只读观测")).toBeTruthy();
  });
});
