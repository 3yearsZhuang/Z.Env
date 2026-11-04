import { afterEach, describe, expect, it, vi } from "vitest";
import { getEnvInfo, type EnvInfo } from "../api";
import { clearEnvInfoCache, readEnvInfo } from "./envInfo";

vi.mock("../api", () => ({
  getEnvInfo: vi.fn(),
}));

const env: EnvInfo = {
  mise_version: "2024.1.1",
  git_version: "2.45.0",
  os: { name: "macOS", version: "15.0", arch: "arm64" },
  pkg: [],
};

afterEach(() => {
  clearEnvInfoCache();
  vi.resetAllMocks();
});

describe("readEnvInfo", () => {
  it("并发调用合并为一次采集", async () => {
    vi.mocked(getEnvInfo).mockResolvedValue(env);
    const [a, b, c] = await Promise.all([readEnvInfo(), readEnvInfo(), readEnvInfo()]);
    expect(getEnvInfo).toHaveBeenCalledTimes(1);
    expect(a).toBe(env);
    expect(b).toBe(env);
    expect(c).toBe(env);
  });

  it("会话内复用缓存，force 时重新采集", async () => {
    vi.mocked(getEnvInfo).mockResolvedValue(env);
    await readEnvInfo();
    await readEnvInfo();
    expect(getEnvInfo).toHaveBeenCalledTimes(1);

    await readEnvInfo({ force: true });
    expect(getEnvInfo).toHaveBeenCalledTimes(2);
  });

  it("失败后不缓存，下一次会重试", async () => {
    vi.mocked(getEnvInfo).mockRejectedValueOnce(new Error("mise 读取失败"));
    await expect(readEnvInfo()).rejects.toThrow("mise 读取失败");

    vi.mocked(getEnvInfo).mockResolvedValue(env);
    await expect(readEnvInfo()).resolves.toBe(env);
    expect(getEnvInfo).toHaveBeenCalledTimes(2);
  });
});
