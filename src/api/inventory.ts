/** 工具的领域身份；不随显示名称或来源包名变化。 */
export type ToolId = string;

/** 单个安装实例的稳定身份。 */
export type InstallationId = string;

export type ProviderId = "mise" | "brew" | "nvm" | "pyenv" | "uv" | "path";
export type Platform = "macos" | "windows" | "linux";
export type PackageKind = "runtime" | "formula" | "cask" | "executable";
export type Health = "healthy" | "broken" | "unverified";
export type Ownership = "owned" | "observed" | "unknown";
export type ProviderStatus =
  "ok" | "unavailable" | "unsupported" | "permissionDenied" | "timeout" | "cancelled" | "error";

export interface ToolDefinition {
  toolId: ToolId;
  displayName: string;
  aliases: string[];
  executableNames: string[];
}

/** 同一工具在某个来源中的包身份。 */
export interface ProviderPackage {
  providerId: ProviderId;
  packageId: string;
  kind: PackageKind;
  platform: Platform;
}

export interface ObservationEvidence {
  kind: string;
  path: string | null;
  detail: string | null;
}

/** 未知值通过显式 null 传递，不能据此推断版本、健康状态或归属。 */
export interface Observation {
  providerPackage: ProviderPackage;
  toolId: ToolId;
  locator: string;
  executablePath: string | null;
  canonicalExecutablePath: string | null;
  version: string | null;
  architecture: string | null;
  health: Health;
  ownership: Ownership;
  evidence: ObservationEvidence[];
}

export interface Installation extends Observation {
  installationId: InstallationId;
  stale: boolean;
}

export interface ProviderCoverage {
  toolIds: ToolId[];
  complete: boolean;
  truncated: boolean;
}

export interface ProviderError {
  code: string;
  message: string;
  retryable: boolean;
}

/** 一次来源扫描的独立结果；失败与空清单是不同状态。 */
export interface ProviderReport {
  providerId: ProviderId;
  status: ProviderStatus;
  observedAt: number;
  stale: boolean;
  coverage: ProviderCoverage;
  error: ProviderError | null;
}

export interface AliasEvidence {
  kind: "canonicalExecutable";
  canonicalExecutablePath: string;
  version: string | null;
  architecture: string | null;
}

/** 关联两个安装实例，保留各自的来源和所有权。 */
export interface InstallationAlias {
  installationId: InstallationId;
  targetInstallationId: InstallationId;
  evidence: AliasEvidence;
}

/** 多来源只读清单的传输契约。 */
export interface InventorySnapshot {
  schemaVersion: 1;
  requestId: string;
  observedAt: number;
  tools: ToolDefinition[];
  providers: ProviderReport[];
  installations: Installation[];
  aliases: InstallationAlias[];
}
