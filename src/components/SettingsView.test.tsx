// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { historyList, type HistoryEntry } from "../api";
import SettingsView from "./SettingsView";

vi.mock("../api", () => ({
  historyList: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(() => Promise.resolve("0.7.1")) }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-autostart", () => ({
  isEnabled: vi.fn(() => Promise.resolve(false)),
  enable: vi.fn(() => Promise.resolve()),
  disable: vi.fn(() => Promise.resolve()),
}));

const history: HistoryEntry[] = [
  { time: 1730000000000, kind: "install", detail: "安装 node@22.3.0" },
  { time: 1730000100000, kind: "unknown-kind", detail: "未知动作" },
];

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(historyList).mockResolvedValue(history);
});

afterEach(cleanup);

describe("SettingsView", () => {
  it("显示应用版本与操作历史（kind 映射为中文）", async () => {
    render(<SettingsView onEaster={vi.fn()} />);
    await waitFor(() => expect(screen.getByText("0.7.1")).toBeTruthy());
    expect(await screen.findByText("安装")).toBeTruthy();
    expect(screen.getByText("安装 node@22.3.0")).toBeTruthy();
    // 未知 kind 原样显示，不猜测
    expect(screen.getByText("unknown-kind")).toBeTruthy();
  });

  it("历史为空时给出空态说明", async () => {
    vi.mocked(historyList).mockResolvedValue([]);
    render(<SettingsView onEaster={vi.fn()} />);
    expect(await screen.findByText(/还没有管理动作记录/)).toBeTruthy();
  });

  it("连点版本号 6 次触发彩蛋", async () => {
    const onEaster = vi.fn();
    render(<SettingsView onEaster={onEaster} />);
    const version = await screen.findByText("0.7.1");

    for (let i = 0; i < 5; i += 1) fireEvent.click(version);
    expect(onEaster).not.toHaveBeenCalled();
    fireEvent.click(version);
    expect(onEaster).toHaveBeenCalledTimes(1);
  });

  it("开机自启状态来自后端并可切换", async () => {
    const autostart = await import("@tauri-apps/plugin-autostart");
    render(<SettingsView onEaster={vi.fn()} />);
    expect(await screen.findByText("已关闭")).toBeTruthy();

    fireEvent.click(screen.getByTitle("登录系统时自动启动 Z.Env"));
    await waitFor(() => expect(autostart.enable).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("已开启")).toBeTruthy();
  });
});
