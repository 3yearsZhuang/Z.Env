import { describe, expect, it } from "vitest";
import rawFixture from "../../fixtures/inventory/contract-v1.json";
import type {
  AliasEvidence,
  Health,
  Installation,
  InstallationAlias,
  InventorySnapshot,
  Observation,
  ObservationEvidence,
  Ownership,
  PackageKind,
  Platform,
  ProviderCoverage,
  ProviderError,
  ProviderId,
  ProviderPackage,
  ProviderReport,
  ProviderStatus,
  ToolDefinition,
} from "./inventory";

type Check<T> = (value: unknown) => asserts value is T;
type Fields<T> = { [K in keyof T]-?: Check<T[K]> };

// 此处只校验共享 JSON 的实际形状，不把静态类型断言当作反序列化证据。
function object<T>(fields: Fields<T>): Check<T> {
  return (value: unknown): asserts value is T => {
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      throw new Error("契约要求一个对象");
    }
    expect(Object.keys(value).sort()).toEqual(Object.keys(fields).sort());
    for (const key in fields) {
      const check: (field: unknown) => void = fields[key];
      check(Reflect.get(value, key));
    }
  };
}

function array<T>(check: Check<T>): Check<T[]> {
  return (value: unknown): asserts value is T[] => {
    if (!Array.isArray(value)) throw new Error("契约要求一个数组");
    for (const item of value) check(item);
  };
}

function nullable<T>(check: Check<T>): Check<T | null> {
  return (value: unknown): asserts value is T | null => {
    if (value !== null) check(value);
  };
}

function choices<T extends string | number>(...values: T[]): Check<T> {
  return (value: unknown): asserts value is T => {
    expect(values).toContain(value);
  };
}

function text(value: unknown): asserts value is string {
  expect(typeof value).toBe("string");
}

function boolean(value: unknown): asserts value is boolean {
  expect(typeof value).toBe("boolean");
}

function timestamp(value: unknown): asserts value is number {
  expect(Number.isSafeInteger(value)).toBe(true);
  expect(value).toBeGreaterThanOrEqual(0);
}

const providerId: Check<ProviderId> = choices("mise", "brew", "nvm", "path");
const platform: Check<Platform> = choices("macos", "windows", "linux");
const packageKind: Check<PackageKind> = choices("runtime", "formula", "cask", "executable");
const health: Check<Health> = choices("healthy", "broken", "unverified");
const ownership: Check<Ownership> = choices("owned", "observed", "unknown");
const providerStatus: Check<ProviderStatus> = choices(
  "ok",
  "unavailable",
  "unsupported",
  "permissionDenied",
  "timeout",
  "cancelled",
  "error",
);

const tool: Check<ToolDefinition> = object<ToolDefinition>({
  toolId: text,
  displayName: text,
  aliases: array(text),
  executableNames: array(text),
});
const providerPackage: Check<ProviderPackage> = object<ProviderPackage>({
  providerId,
  packageId: text,
  kind: packageKind,
  platform,
});
const evidence: Check<ObservationEvidence> = object<ObservationEvidence>({
  kind: text,
  path: nullable(text),
  detail: nullable(text),
});
const observationFields: Fields<Observation> = {
  providerPackage,
  toolId: text,
  locator: text,
  executablePath: nullable(text),
  canonicalExecutablePath: nullable(text),
  version: nullable(text),
  architecture: nullable(text),
  health,
  ownership,
  evidence: array(evidence),
};
const observation: Check<Observation> = object<Observation>(observationFields);
const installation: Check<Installation> = object<Installation>({
  ...observationFields,
  installationId: text,
  stale: boolean,
});
const coverage: Check<ProviderCoverage> = object<ProviderCoverage>({
  toolIds: array(text),
  complete: boolean,
  truncated: boolean,
});
const providerError: Check<ProviderError> = object<ProviderError>({
  code: text,
  message: text,
  retryable: boolean,
});
const report: Check<ProviderReport> = object<ProviderReport>({
  providerId,
  status: providerStatus,
  observedAt: timestamp,
  stale: boolean,
  coverage,
  error: nullable(providerError),
});
const aliasEvidence: Check<AliasEvidence> = object<AliasEvidence>({
  kind: choices("canonicalExecutable"),
  canonicalExecutablePath: text,
  version: nullable(text),
  architecture: nullable(text),
});
const alias: Check<InstallationAlias> = object<InstallationAlias>({
  installationId: text,
  targetInstallationId: text,
  evidence: aliasEvidence,
});
const snapshot: Check<InventorySnapshot> = object<InventorySnapshot>({
  schemaVersion: choices(1),
  requestId: text,
  observedAt: timestamp,
  tools: array(tool),
  providers: array(report),
  installations: array(installation),
  aliases: array(alias),
});

interface ContractFixture {
  requestId: string;
  observedAt: number;
  scans: { report: ProviderReport; observations: Observation[] }[];
  expectedSnapshot: InventorySnapshot;
}

const contract: Check<ContractFixture> = object<ContractFixture>({
  requestId: text,
  observedAt: timestamp,
  scans: array(
    object<ContractFixture["scans"][number]>({
      report,
      observations: array(observation),
    }),
  ),
  expectedSnapshot: snapshot,
});

function readFixture(): ContractFixture {
  const fixture: unknown = rawFixture;
  contract(fixture);
  return fixture;
}

describe("inventory 跨语言契约", () => {
  it("逐字段解析共享 JSON，保留来源失败和未知值", () => {
    const { requestId, observedAt, expectedSnapshot } = readFixture();
    expect(expectedSnapshot.requestId).toBe(requestId);
    expect(expectedSnapshot.observedAt).toBe(observedAt);
    expect(expectedSnapshot.providers.find((item) => item.providerId === "mise")).toMatchObject({
      status: "timeout",
      coverage: { complete: false },
      error: { code: "probeTimeout", retryable: true },
    });
    expect(
      expectedSnapshot.installations.find((item) => item.providerPackage.providerId === "nvm"),
    ).toMatchObject({
      toolId: "node",
      version: null,
      canonicalExecutablePath: null,
      architecture: null,
      health: "broken",
      ownership: "unknown",
    });
  });

  it("快照保留同版本不同目录的实例及 PATH 关联的独立归属", () => {
    const { expectedSnapshot } = readFixture();
    const installations = expectedSnapshot.installations;
    const brew = installations.filter((item) => item.providerPackage.providerId === "brew");
    const path = installations.find((item) => item.providerPackage.providerId === "path");
    expect(installations).toHaveLength(4);
    expect(new Set(installations.map((item) => item.installationId)).size).toBe(4);
    expect(installations.every((item) => item.toolId === "node")).toBe(true);
    expect(brew).toHaveLength(2);
    expect(brew.map((item) => item.version)).toEqual(["22.11.0", "22.11.0"]);
    expect(new Set(brew.map((item) => item.canonicalExecutablePath)).size).toBe(2);
    expect(brew.every((item) => item.ownership === "owned")).toBe(true);
    expect(path?.ownership).toBe("observed");
    expect(expectedSnapshot.aliases).toHaveLength(1);
    expect(expectedSnapshot.aliases[0]).toMatchObject({
      installationId: path?.installationId,
      targetInstallationId: brew.find((item) => item.architecture === "arm64")?.installationId,
      evidence: {
        kind: "canonicalExecutable",
        version: "22.11.0",
        architecture: "arm64",
      },
    });
  });

  it("拒绝遗漏 nullable 字段和错误的枚举拼写", () => {
    const fixture = readFixture();
    const healthyReport = fixture.scans[0].report;
    const { error: omitted, ...missingError } = healthyReport;
    expect(omitted).toBeNull();
    expect(() => report(missingError)).toThrow();
    expect(() => report({ ...healthyReport, status: "permission_denied" })).toThrow();
    expect(() => report({ ...healthyReport, status: "permissionDenied" })).not.toThrow();
    expect(() => snapshot({ ...fixture.expectedSnapshot, schemaVersion: 2 })).toThrow();
  });
});
