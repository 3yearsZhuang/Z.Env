// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useMachineInstall } from "./useMachineInstall";
import { installPreset, onInstallProgress, operationCancel } from "../api";
vi.mock("../api", () => ({
  installPreset: vi.fn(),
  onInstallProgress: vi.fn(),
  errorMessage: String,
  operationCancel: vi.fn(),
}));
it("整套预设作为一个后端任务提交，日志订阅绑定相同任务 ID", async () => {
  const unlisten = vi.fn();
  vi.mocked(onInstallProgress).mockResolvedValue(unlisten);
  vi.mocked(installPreset).mockResolvedValue(2);
  const { result } = renderHook(useMachineInstall);
  const tools = [
    { name: "node", version: "20" },
    { name: "python", version: "3" },
  ];
  await act(() => result.current.install(tools));
  expect(installPreset).toHaveBeenCalledTimes(1);
  const id = vi.mocked(installPreset).mock.calls[0][1];
  expect(onInstallProgress).toHaveBeenCalledWith(expect.any(Function), id);
  expect(unlisten).toHaveBeenCalledOnce();
  expect(result.current.installBusy).toBe(false);
});
it("安装弹窗可取消自身任务并保留已成功数量", async () => {
  vi.clearAllMocks();
  vi.mocked(operationCancel).mockResolvedValue();
  vi.mocked(onInstallProgress).mockResolvedValue(vi.fn());
  let finish!: (count: number) => void;
  vi.mocked(installPreset).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const { result } = renderHook(useMachineInstall);
  let installing!: Promise<number>;
  await act(async () => {
    installing = result.current.install([{ name: "node", version: "22" }]);
  });
  await act(() => result.current.cancel());
  expect(operationCancel).toHaveBeenCalledWith(vi.mocked(installPreset).mock.calls[0][1]);
  expect(result.current.cancelRequested).toBe(true);
  await act(async () => {
    finish(1);
    expect(await installing).toBe(1);
  });
  expect(result.current.installBusy).toBe(false);
});
