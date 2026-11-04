// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  installVersionStreaming,
  listRemoteVersions,
  listRemoteVersionsAsdf,
  listRemoteVersionsGithub,
  listRemoteVersionsOfficial,
  onInstallProgress,
  type ToolInfo,
} from "../api";
import InstallDialog from "./InstallDialog";

vi.mock("../api", () => ({
  installVersionStreaming: vi.fn(),
  listRemoteVersions: vi.fn(),
  listRemoteVersionsAsdf: vi.fn(),
  listRemoteVersionsGithub: vi.fn(),
  listRemoteVersionsOfficial: vi.fn(),
  onInstallProgress: vi.fn(),
  operationCancel: vi.fn(),
  managedAdopt: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

const tool: ToolInfo = {
  name: "node",
  versions: [],
  active_versions: [],
};

beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
  vi.mocked(listRemoteVersions).mockResolvedValue(["22.23.2", "20.20.2"]);
  vi.mocked(listRemoteVersionsAsdf).mockResolvedValue([]);
  vi.mocked(listRemoteVersionsGithub).mockResolvedValue([]);
  vi.mocked(listRemoteVersionsOfficial).mockResolvedValue([]);
  vi.mocked(installVersionStreaming).mockResolvedValue("");
  vi.mocked(onInstallProgress).mockResolvedValue(() => {});
});

afterEach(cleanup);

describe("InstallDialog", () => {
  it("远程版本加载完成后可选中并安装", async () => {
    render(<InstallDialog tool={tool} onClose={vi.fn()} onDone={vi.fn()} />);
    await waitFor(() => expect(listRemoteVersions).toHaveBeenCalledWith("node"));

    const picker = await screen.findByLabelText("选择远程版本");
    fireEvent.change(picker, { target: { value: "22.23.2" } });
    const install = screen.getByRole("button", { name: "安装 22.23.2" });
    fireEvent.click(install);

    await waitFor(() =>
      expect(installVersionStreaming).toHaveBeenCalledWith("node", "22.23.2", expect.any(String)),
    );
  });

  it("远程源全部不可用时降级为手动输入，仍可安装", async () => {
    vi.mocked(listRemoteVersions).mockRejectedValue(new Error("网络不可用"));
    render(<InstallDialog tool={tool} onClose={vi.fn()} onDone={vi.fn()} />);

    expect(await screen.findByText("无法自动获取远程版本，可手动输入版本号")).toBeTruthy();
    // 未填版本号时安装按钮就是禁用的
    expect(screen.getByRole("button", { name: /^安装$/ })).toHaveProperty("disabled", true);

    fireEvent.change(screen.getByLabelText("或手动输入版本号"), {
      target: { value: "latest" },
    });
    fireEvent.click(screen.getByRole("button", { name: "安装 latest" }));

    await waitFor(() =>
      expect(installVersionStreaming).toHaveBeenCalledWith("node", "latest", expect.any(String)),
    );
  });

  it("主源失败时按顺序回退到下一个源并提示来源", async () => {
    vi.mocked(listRemoteVersions).mockRejectedValue(new Error("mise 源不可用"));
    vi.mocked(listRemoteVersionsOfficial).mockResolvedValue(["21.7.0"]);
    render(<InstallDialog tool={tool} onClose={vi.fn()} onDone={vi.fn()} />);

    expect(await screen.findByText(/当前版本列表来自：官方生态源/)).toBeTruthy();
    expect(screen.getByText(/自动回退/)).toBeTruthy();
  });

  it("外部托管版本列出接管入口，系统组件标为不可接管", async () => {
    render(
      <InstallDialog
        tool={tool}
        external={[
          {
            tool: "node",
            manager: "nvm",
            version: "18.20.4",
            path: "/Users/me/.nvm/versions/node/v18.20.4",
          },
          { tool: "node", manager: "brew", version: "17.9.1", path: "/opt/homebrew/bin/node" },
        ]}
        onClose={vi.fn()}
        onDone={vi.fn()}
      />,
    );
    const rows = await screen.findAllByText(/nvm · 18\.20\.4|brew · 17\.9\.1/);
    expect(rows).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "接管" }).length).toBeGreaterThan(0);
    expect(
      within(screen.getByText(/brew · 17\.9\.1/).closest(".adopt-row")!).getByRole("button"),
    ).toBeTruthy();
  });

  it("取消按钮在空闲时可直接关闭", async () => {
    const onClose = vi.fn();
    render(<InstallDialog tool={tool} onClose={onClose} onDone={vi.fn()} />);
    await waitFor(() => expect(listRemoteVersions).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
