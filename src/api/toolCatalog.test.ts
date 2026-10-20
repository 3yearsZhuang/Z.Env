import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import fixture from "../../fixtures/tool-catalog/contract-v1.json";
import { toolCatalog, validateToolCatalog } from "./toolCatalog";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
it("三平台契约保留工具、原生包及带原因的接入状态，不请求安装清单", async () => {
  for (const value of fixture) expect(validateToolCatalog(value)).toEqual(value);
  vi.mocked(invoke).mockResolvedValue(fixture[0]);
  await expect(toolCatalog()).resolves.toEqual(fixture[0]);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("tool_catalog");
});
it("未来格式、空目录、缺失能力、重复身份、跨平台包及危险链接均拒绝", () => {
  const mutate = (edit: (v: ReturnType<typeof validateToolCatalog>) => void) => {
    const v = structuredClone(fixture[0]) as ReturnType<typeof validateToolCatalog>;
    edit(v);
    return v;
  };
  for (const value of [
    { ...fixture[0], schemaVersion: 2 },
    { ...fixture[0], tools: [] },
    mutate((v) => {
      v.tools.push(v.tools[0]);
    }),
    mutate((v) => {
      v.tools[1].definition.aliases.push("node");
    }),
    mutate((v) => {
      v.tools[0].channels[0].packages[0].platform = "linux";
    }),
    mutate((v) => {
      v.tools[0].channels[0].installPlan.reason = "";
    }),
    mutate((v) => {
      v.tools[0].documentationUrl = "javascript:alert(1)";
    }),
    mutate((v) => {
      v.tools[0].documentationUrl = "https://secret:password@example.com/";
    }),
  ])
    expect(() => validateToolCatalog(value)).toThrow(/格式不受支持/);
});
