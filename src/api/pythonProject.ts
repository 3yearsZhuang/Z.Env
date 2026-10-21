import { invoke } from "@tauri-apps/api/core";
import type { ProviderId } from "./inventory";
import { inventoryRead } from "./inventoryRead";

export type PythonRequirementState = "unspecified" | "supported" | "conflict" | "unknown";
export type PythonMatch = "satisfied" | "mismatch" | "unknown";

export interface PythonProjectRequest {
  requestId: string;
  snapshotRequestId: string;
  path: string;
}

export interface PythonDeclaration {
  source: string;
  path: string;
  requirement: string | null;
  supported: boolean;
  reason: string | null;
}

export interface PythonCandidate {
  installationId: string;
  providerId: ProviderId;
  version: string | null;
  locator: string;
  requirementMatch: PythonMatch;
  usable: boolean;
  reason: string | null;
}

export interface PythonSelection {
  kind: string;
  path: string | null;
  requirement: string | null;
  reason: string | null;
}

export interface PythonBinding {
  bindingId: string;
  installationId: string;
  version: string;
  locator: string;
  configPath: string;
  scope: "project" | "global";
  state: string;
}

export interface PythonProjectEnvironment {
  schemaVersion: 1;
  requestId: string;
  snapshotRequestId: string;
  projectDirectory: string;
  observedAt: number;
  declarations: PythonDeclaration[];
  requirementState: PythonRequirementState;
  candidates: PythonCandidate[];
  inventoryComplete: boolean;
  selection: PythonSelection;
  issues: { code: string; message: string }[];
  files: { path: string; canonicalPath: string; exists: boolean; sha256: string | null }[];
}

const PROVIDERS: PythonProviderId[] = ["mise", "brew", "nvm", "pyenv", "uv", "path"];
type PythonProviderId = ProviderId;
const STATES: PythonRequirementState[] = ["unspecified", "supported", "conflict", "unknown"];
const MATCHES: PythonMatch[] = ["satisfied", "mismatch", "unknown"];

function text(value: unknown, limit: number): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= limit;
}
function nullableText(value: unknown, limit: number): value is string | null {
  return value === null || text(value, limit);
}
function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** 只接受后端报告的完整字段；未知结构、跨来源或超预算记录直接拒绝。 */
export function validatePythonProject(value: unknown): PythonProjectEnvironment {
  if (!record(value) || value.schemaVersion !== 1) throw new Error("Python 项目报告版本不受支持");
  if (!text(value.requestId, 128) || !text(value.snapshotRequestId, 128)) {
    throw new Error("Python 项目报告缺少请求身份");
  }
  if (!text(value.projectDirectory, 4096) || typeof value.observedAt !== "number") {
    throw new Error("Python 项目报告路径或时间无效");
  }
  if (!Array.isArray(value.declarations) || value.declarations.length > 64) {
    throw new Error("Python 声明超出预算");
  }
  for (const item of value.declarations) {
    if (
      !record(item) ||
      !text(item.source, 256) ||
      !text(item.path, 4096) ||
      !nullableText(item.requirement, 256) ||
      typeof item.supported !== "boolean" ||
      !nullableText(item.reason, 512)
    ) {
      throw new Error("Python 声明字段无效");
    }
  }
  if (!STATES.includes(value.requirementState as PythonRequirementState)) {
    throw new Error("Python 要求状态无效");
  }
  if (!Array.isArray(value.candidates) || value.candidates.length > 256) {
    throw new Error("Python 候选超出预算");
  }
  for (const item of value.candidates) {
    if (
      !record(item) ||
      !text(item.installationId, 128) ||
      !PROVIDERS.includes(item.providerId as ProviderId) ||
      !nullableText(item.version, 128) ||
      !text(item.locator, 4096) ||
      !MATCHES.includes(item.requirementMatch as PythonMatch) ||
      typeof item.usable !== "boolean" ||
      !nullableText(item.reason, 512)
    ) {
      throw new Error("Python 候选字段无效");
    }
  }
  if (typeof value.inventoryComplete !== "boolean" || !record(value.selection)) {
    throw new Error("Python 报告摘要无效");
  }
  const selection = value.selection;
  if (
    !text(selection.kind, 64) ||
    !nullableText(selection.path, 4096) ||
    !nullableText(selection.requirement, 256) ||
    !nullableText(selection.reason, 512)
  ) {
    throw new Error("Python 当前选择字段无效");
  }
  if (!Array.isArray(value.issues) || value.issues.length > 64) {
    throw new Error("Python 问题列表超出预算");
  }
  for (const item of value.issues) {
    if (!record(item) || !text(item.code, 128) || !text(item.message, 512)) {
      throw new Error("Python 问题字段无效");
    }
  }
  if (!Array.isArray(value.files) || value.files.length > 96) {
    throw new Error("Python 文件证据超出预算");
  }
  for (const item of value.files) {
    if (
      !record(item) ||
      !text(item.path, 4096) ||
      !text(item.canonicalPath, 4096) ||
      typeof item.exists !== "boolean" ||
      !nullableText(item.sha256, 128)
    ) {
      throw new Error("Python 文件证据无效");
    }
  }
  return value as unknown as PythonProjectEnvironment;
}

export function pythonProject(request: PythonProjectRequest): Promise<PythonProjectEnvironment> {
  return inventoryRead(async () => {
    const value = await invoke("python_project_environment", { request });
    return validatePythonProject(value);
  });
}

/** 私有绑定收据；只返回本应用写入的 Python 配置绑定。 */
export function validatePythonBindings(value: unknown): PythonBinding[] {
  if (!Array.isArray(value) || value.length > 32) throw new Error("Python 绑定列表超出预算");
  for (const item of value) {
    if (
      !record(item) ||
      !text(item.bindingId, 128) ||
      !text(item.installationId, 128) ||
      !text(item.version, 128) ||
      !text(item.locator, 4096) ||
      !text(item.configPath, 4096) ||
      (item.scope !== "project" && item.scope !== "global") ||
      item.state !== "bound"
    ) {
      throw new Error("Python 绑定记录无效");
    }
  }
  return value as unknown as PythonBinding[];
}

export async function pythonBindings(): Promise<PythonBinding[]> {
  return validatePythonBindings(await invoke("python_bindings"));
}
