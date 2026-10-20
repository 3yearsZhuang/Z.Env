import { expect, it } from "vitest";
import fixture from "../../fixtures/tool-catalog/contract-v1.json";
import inventory from "../../fixtures/inventory/contract-v1.json";
import { validateToolCatalog } from "../api/toolCatalog";
import type { InventorySnapshot } from "../api/inventory";
import { catalogEvidence } from "./catalogEvidence";
const catalog = validateToolCatalog(fixture[0]);
function read() {
  return {
    snapshot: structuredClone(inventory.expectedSnapshot) as InventorySnapshot,
    loading: false,
    error: null,
  };
}
it("未接入工具即使收到同名安装也保持未知，Node 覆盖不扩展为 Java 覆盖", () => {
  const state = read();
  state.snapshot.installations.push({ ...state.snapshot.installations[0], toolId: "java" });
  expect(catalogEvidence(catalog.tools[2], state)).toEqual({
    state: "unknown",
    count: 0,
    reason: expect.stringContaining("尚未接入"),
  });
  expect(catalogEvidence(validateToolCatalog(fixture[1]).tools[0], state).state).toBe("unknown");
});
it("PATH 观测和过期目录不能充当新鲜的安装实例", () => {
  const state = read();
  state.snapshot.installations = state.snapshot.installations.filter(
    (i) => i.providerPackage.providerId === "path",
  );
  const node = catalog.tools[0];
  expect(catalogEvidence(node, state).count).toBe(0);
  state.snapshot.installations = [
    {
      ...inventory.expectedSnapshot.installations[0],
      stale: true,
    } as InventorySnapshot["installations"][number],
  ];
  expect(catalogEvidence(node, state).state).toBe("unknown");
  state.snapshot.providers = state.snapshot.providers.map((p) => ({
    ...p,
    status: "ok",
    error: null,
    stale: false,
    coverage: { toolIds: ["node"], complete: true, truncated: false },
  }));
  state.snapshot.installations = state.snapshot.installations.map((i) => ({
    ...i,
    stale: false,
    providerPackage: { ...i.providerPackage, platform: "windows" },
  }));
  expect(catalogEvidence(node, state).state).toBe("unknown");
});
it("只有工具自身所有已接入来源完整读取才能报告当前来源未观察到", () => {
  const state = read();
  state.snapshot.installations = [];
  state.snapshot.providers = state.snapshot.providers.map((p) => ({
    ...p,
    status: "ok",
    stale: false,
    error: null,
    coverage: { toolIds: ["node"], complete: true, truncated: false },
  }));
  expect(catalogEvidence(catalog.tools[0], state).state).toBe("notObserved");
  state.snapshot.providers[0].coverage.toolIds = ["python"];
  expect(catalogEvidence(catalog.tools[0], state).state).toBe("unknown");
  state.snapshot.providers[0].coverage = { toolIds: ["node"], complete: true, truncated: true };
  expect(catalogEvidence(catalog.tools[0], state).state).toBe("unknown");
});
it("新鲜实例、刷新中的旧实例和失败缓存保持不同状态", () => {
  const state = read();
  const node = catalog.tools[0];
  state.snapshot.providers = state.snapshot.providers.map((p) => ({
    ...p,
    status: "ok",
    stale: false,
    error: null,
    coverage: { toolIds: ["node"], complete: true, truncated: false },
  }));
  state.snapshot.installations = state.snapshot.installations.map((i) => ({ ...i, stale: false }));
  expect(catalogEvidence(node, state).state).toBe("observed");
  expect(catalogEvidence(node, { ...state, loading: true }).state).toBe("unknown");
  expect(catalogEvidence(node, { ...state, error: "failed" }).state).toBe("unknown");
});

it("来源部分失败不抹掉本次健康实例，但不能推导其他安装或写权限", () => {
  const state = read();
  state.snapshot.installations = state.snapshot.installations.map((i) => ({
    ...i,
    health: "healthy",
    stale: false,
  }));
  state.snapshot.providers = state.snapshot.providers.map((p) => ({
    ...p,
    status: "error",
    stale: false,
    error: { code: "partial", message: "部分读取失败", retryable: true },
    coverage: { toolIds: ["node"], complete: false, truncated: true },
  }));
  const result = catalogEvidence(catalog.tools[0], state);
  expect(result.state).toBe("observed");
  expect(result.reason).toContain("部分来源");
  state.snapshot.installations = state.snapshot.installations.map((i) => ({ ...i, stale: true }));
  expect(catalogEvidence(catalog.tools[0], state).state).toBe("unknown");
});
