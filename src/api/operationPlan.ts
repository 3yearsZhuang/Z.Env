import { invoke } from "@tauri-apps/api/core";
import type { ProviderId, ProviderPackage } from "./inventory";
import { operationPlanV2 } from "./operationPlanV2";

export type PlanAction = "verify" | "install" | "uninstall" | "bind" | "unbind";
export type BindingStrategy = "direct" | "managed" | "projectPath";
export type BindingScope = "registration" | "global" | "project";
export interface BindingOptions {
  strategy: BindingStrategy;
  scope: BindingScope;
  projectPath: string | null;
}
export interface BindingSummary {
  bindingId: string;
  providerPackage: ProviderPackage;
  version: string;
  locator: string;
  strategy: BindingStrategy;
  scope: BindingScope;
  configurationPath: string | null;
  state: "pending" | "bound";
}
export type PlanIntent =
  | { action: "verify" | "uninstall"; installationId: string }
  | { action: "bind"; installationId: string; binding?: BindingOptions }
  | { action: "unbind"; bindingId: string }
  | { action: "install"; providerId: ProviderId; packageId: string; version: string | null };
export interface PlanRequest {
  snapshotRequestId: string;
  intent: PlanIntent;
}
export type PlanStepKind = "preflight" | "command" | "refresh" | "verify" | "compensate";
export interface OperationPlan {
  schemaVersion: 1;
  /** 旧 Node 计划省略此字段。 */
  toolId?: "node" | "python";
  planId: string;
  createdAt: number;
  expiresAt: number;
  snapshotGeneration: string;
  snapshotRequestId: string;
  action: PlanAction;
  target: {
    providerPackage: ProviderPackage;
    installationId: string | null;
    locator: string | null;
  };
  capability: { canPreview: boolean; canExecute: boolean; reason: string | null };
  versionPromise: string;
  configuration: {
    path: string;
    canonicalPath: string | null;
    exists: boolean;
    sha256: string | null;
  };
  configurationChange: string;
  permission: string;
  rollback: string;
  steps: {
    kind: PlanStepKind;
    title: string;
    program: string | null;
    argv: string[];
    cwd: string;
    environmentKeys: string[];
  }[];
  impact: string[];
  configurationDiff?: {
    path: string;
    field: string;
    before: string | null;
    after: string | null;
  }[];
}
export type ExecutionStatus =
  "running" | "completed" | "partial" | "failed" | "cancelled" | "interrupted";
export interface ExecutionRecord {
  schemaVersion: 1;
  toolId?: "node" | "python";
  id: string;
  action: PlanAction;
  status: ExecutionStatus;
  createdAt: number;
  updatedAt: number;
  steps: {
    kind: PlanStepKind;
    status: "planned" | "running" | "completed" | "failed" | "skipped";
  }[];
  outcome:
    | "verified"
    | "installed"
    | "uninstalled"
    | "bound"
    | "unbound"
    | "compensationConflict"
    | "scanIncomplete"
    | "preconditionChanged"
    | "cancelled"
    | "stepFailed"
    | "interrupted"
    | null;
  compensation?: "completed" | "conflict" | "failed" | null;
}
export function operationBindings(): Promise<BindingSummary[]> {
  return invoke("operation_bindings");
}
export async function operationPlan(
  request: PlanRequest,
  toolId: "node" | "python" = "node",
): Promise<OperationPlan> {
  const response = await operationPlanV2({
    schemaVersion: 2,
    subject: { kind: "tool", toolId },
    request,
  });
  return response.plan;
}
/** 执行只提交后端签发的身份，不回传可改写的步骤、argv 或配置。 */
export function operationExecute(planId: string): Promise<ExecutionRecord> {
  return invoke("operation_execute", { planId });
}
