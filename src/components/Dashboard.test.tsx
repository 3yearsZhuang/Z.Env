// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { checkMise, getEnvInfo, systemStats, type SystemStats } from "../api";
import { clearEnvInfoCache } from "../lib/envInfo";
import Dashboard from "./Dashboard";

vi.mock("../api", () => ({
  systemStats: vi.fn(),
  checkMise: vi.fn(),
  getEnvInfo: vi.fn(),
  errorMessage: (error: unknown) => String(error),
}));

const stats: SystemStats = {
  cpuUsage: 12,
  cpuCores: 8,
  cpuBrand: "Apple M3",
  memoryTotalGb: 16,
  memoryUsedGb: 6,
  memoryPercent: 38,
  memories: [],
  storage: [],
  disks: [{ name: "disk1", model: null, totalGb: 512, availableGb: 200, percent: 61 }],
  battery: null,
  gpus: [],
  interfaces: [],
  osName: "macOS",
  hostName: "test",
};

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "hidden", { value: hidden, configurable: true });
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
}

beforeEach(() => {
  clearEnvInfoCache();
  vi.resetAllMocks();
  vi.useFakeTimers();
  vi.mocked(systemStats).mockResolvedValue(stats);
  vi.mocked(checkMise).mockResolvedValue({ installed: true, version: "2024.1.1" });
  vi.mocked(getEnvInfo).mockResolvedValue({
    mise_version: "2024.1.1",
    git_version: "2.45.0",
    os: { name: "macOS", version: "15.0", arch: "arm64" },
    pkg: [],
  });
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  setHidden(false);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("Dashboard 采样调度", () => {
  it("窗口聚焦时每 2 秒采样一次", async () => {
    render(<Dashboard onNavigate={() => {}} />);
    await flush();
    expect(vi.mocked(systemStats)).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(vi.mocked(systemStats)).toHaveBeenCalledTimes(2);
  });

  it("窗口失焦后降到 10 秒一次，并显示低频采样", async () => {
    render(<Dashboard onNavigate={() => {}} />);
    await flush();

    vi.mocked(document.hasFocus).mockReturnValue(false);
    await act(async () => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(screen.getByText("低频采样")).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(vi.mocked(systemStats)).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(8000);
    });
    expect(vi.mocked(systemStats)).toHaveBeenCalledTimes(2);
  });

  it("窗口隐藏时停止采样，回到前台立即补一次", async () => {
    render(<Dashboard onNavigate={() => {}} />);
    await flush();

    setHidden(true);
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60000);
    });
    expect(vi.mocked(systemStats)).toHaveBeenCalledTimes(1);

    setHidden(false);
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await flush();
    expect(vi.mocked(systemStats)).toHaveBeenCalledTimes(2);
  });
});
