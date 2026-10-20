import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import fixture from "../../fixtures/project-environment/contract-v1.json";
import { projectEnvironment } from "./projectEnvironment";
import { inventorySnapshot, inventoryCancel } from "./inventoryQuery";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
it("项目与清单共享读取队列，取消信号不等待读取槽", async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementation((command) =>
    command === "inventory_snapshot"
      ? new Promise((r) => {
          finish = r;
        })
      : Promise.resolve(command === "inventory_cancel" ? true : fixture.report),
  );
  const scan = inventorySnapshot("scan");
  const report = projectEnvironment(fixture.request);
  expect(invoke).toHaveBeenCalledTimes(1);
  await expect(inventoryCancel("scan")).resolves.toBe(true);
  expect(invoke).toHaveBeenCalledTimes(2);
  finish({ requestId: "scan" });
  await scan;
  await expect(report).resolves.toEqual(fixture.report);
  expect(invoke).toHaveBeenLastCalledWith("project_environment", { request: fixture.request });
});
it("读取失败后释放队列，不吞掉错误或阻止后续核对", async () => {
  vi.mocked(invoke)
    .mockRejectedValueOnce(new Error("scan failed"))
    .mockResolvedValueOnce(fixture.report);
  const bad = inventorySnapshot("failed");
  const next = projectEnvironment(fixture.request);
  await expect(bad).rejects.toThrow("scan failed");
  await expect(next).resolves.toEqual(fixture.report);
});

it("Python 与 Node 读取共用队列但请求保留显式工具身份", async () => {
  vi.mocked(invoke).mockResolvedValue({ requestId: "python-read" });
  await inventorySnapshot("python-read", "pyenv", "python");
  expect(invoke).toHaveBeenCalledExactlyOnceWith("inventory_snapshot", {
    requestId: "python-read",
    providerId: "pyenv",
    toolId: "python",
  });
});
