// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  onInstallProgress,
  snapshotExport,
  snapshotPreview,
  snapshotRestore,
  type SnapshotPlan,
} from "../api";
import SnapshotPanel from "./SnapshotPanel";

vi.mock("../api", () => ({
  snapshotExport: vi.fn(),
  snapshotRestore: vi.fn(),
  snapshotPreview: vi.fn(),
  onInstallProgress: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
  open: vi.fn(),
}));

const plan: SnapshotPlan = {
  source: "/tmp/snap.toml",
  original: null,
  tools: { node: ["22.23.2"] },
  envKeys: ["HTTP_PROXY"],
  brewPackages: 2,
  warnings: ["brew 软件需要手动执行 brew bundle"],
};

beforeEach(async () => {
  vi.resetAllMocks();
  vi.mocked(onInstallProgress).mockResolvedValue(() => {});
  vi.mocked(snapshotExport).mockResolvedValue({
    tools: 3,
    envVars: 1,
    brewPackages: 2,
    path: "/tmp/zenv-snapshot.toml",
  });
  vi.mocked(snapshotPreview).mockResolvedValue(plan);
  vi.mocked(snapshotRestore).mockResolvedValue("重建完成，已激活 3 个版本");
});

afterEach(cleanup);

describe("SnapshotPanel", () => {
  it("导出时把「包含全局环境变量」传给后端并显示汇总", async () => {
    const { save } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(save).mockResolvedValue("/tmp/zenv-snapshot.toml");

    render(<SnapshotPanel />);
    fireEvent.click(screen.getByLabelText(/导出包含全局环境变量/));
    fireEvent.click(screen.getByRole("button", { name: "导出快照" }));

    await waitFor(() =>
      expect(snapshotExport).toHaveBeenCalledWith("/tmp/zenv-snapshot.toml", true),
    );
    expect(await screen.findByText(/mise 工具 3 个 · 全局 env 1 个 · brew 软件 2 个/)).toBeTruthy();
  });

  it("导出取消时不调用后端", async () => {
    const { save } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(save).mockResolvedValue(null);

    render(<SnapshotPanel />);
    fireEvent.click(screen.getByRole("button", { name: "导出快照" }));
    await waitFor(() => expect(save).toHaveBeenCalled());
    expect(snapshotExport).not.toHaveBeenCalled();
  });

  it("重建先预览再执行，期间上报通道占用并推送日志", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/tmp/snap.toml");
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const onRestored = vi.fn();
    const onRestoreBusyChange = vi.fn();

    render(<SnapshotPanel onRestored={onRestored} onRestoreBusyChange={onRestoreBusyChange} />);
    fireEvent.click(screen.getByRole("button", { name: "从快照重建" }));

    await waitFor(() => expect(snapshotRestore).toHaveBeenCalledTimes(1));
    // 预览摘要进入确认框：工具、env、brew 与警告都要出现在文案里
    expect(confirm.mock.calls[0][0]).toContain("node: 22.23.2");
    expect(confirm.mock.calls[0][0]).toContain("HTTP_PROXY");
    expect(confirm.mock.calls[0][0]).toContain("brew 软件需要手动执行 brew bundle");

    expect(onRestoreBusyChange.mock.calls).toEqual([[true], [false]]);
    expect(onRestored).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("重建完成，已激活 3 个版本")).toBeTruthy();
    confirm.mockRestore();
  });

  it("预览失败时只提示原因，不进入重建", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/tmp/broken.toml");
    vi.mocked(snapshotPreview).mockRejectedValueOnce(new Error("不是快照文件"));
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<SnapshotPanel />);
    fireEvent.click(screen.getByRole("button", { name: "从快照重建" }));

    expect(await screen.findByText("预览失败：不是快照文件")).toBeTruthy();
    expect(confirm).not.toHaveBeenCalled();
    expect(snapshotRestore).not.toHaveBeenCalled();
    confirm.mockRestore();
  });

  it("用户在确认框取消时不重建", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/tmp/snap.toml");
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);

    render(<SnapshotPanel />);
    fireEvent.click(screen.getByRole("button", { name: "从快照重建" }));
    await waitFor(() => expect(confirm).toHaveBeenCalled());
    expect(snapshotRestore).not.toHaveBeenCalled();
    confirm.mockRestore();
  });

  it("另一路安装进行中时禁用重建并说明原因", () => {
    render(<SnapshotPanel machineInstallBusy />);
    expect(screen.getByRole("button", { name: "从快照重建" })).toHaveProperty("disabled", true);
    expect(screen.getByText(/「环境预设安装」进行中/)).toBeTruthy();
  });
});
