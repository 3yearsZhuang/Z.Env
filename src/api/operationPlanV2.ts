import { invoke } from "@tauri-apps/api/core";
import type { OperationPlan, PlanRequest } from "./operationPlan";

type ToolSubject = { kind: "tool"; toolId: "node" | "python" };

export interface PlanRequestV2 {
  schemaVersion: 2;
  subject: ToolSubject;
  request: PlanRequest;
}

export interface OperationPlanV2 {
  schemaVersion: 2;
  subject: ToolSubject;
  plan: OperationPlan;
  stepGraph: { stepId: string; stepIndex: number; dependsOn: string[] }[];
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function subject(value: unknown): value is ToolSubject {
  return (
    record(value) && value.kind === "tool" && (value.toolId === "node" || value.toolId === "python")
  );
}

function id(value: unknown): value is string {
  return typeof value === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(value);
}

function timestamp(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function validateRequest(value: unknown): asserts value is PlanRequestV2 {
  if (
    !record(value) ||
    value.schemaVersion !== 2 ||
    !subject(value.subject) ||
    !record(value.request) ||
    typeof value.request.snapshotRequestId !== "string" ||
    !value.request.snapshotRequestId.trim() ||
    !record(value.request.intent) ||
    typeof value.request.intent.action !== "string" ||
    !["verify", "install", "uninstall", "bind", "unbind"].includes(value.request.intent.action)
  ) {
    throw new Error("不支持或无效的 v2 操作计划请求");
  }
}

function validateResponse(value: unknown, request: PlanRequestV2): OperationPlanV2 {
  const invalid = () => new Error("v2 操作计划响应无效或与当前请求不匹配，请重新预览");
  if (
    !record(value) ||
    value.schemaVersion !== 2 ||
    !subject(value.subject) ||
    value.subject.toolId !== request.subject.toolId ||
    !record(value.plan)
  ) {
    throw invalid();
  }
  const plan = value.plan;
  if (
    plan.schemaVersion !== 1 ||
    (plan.toolId === undefined ? "node" : plan.toolId) !== request.subject.toolId ||
    !id(plan.planId) ||
    !timestamp(plan.createdAt) ||
    !timestamp(plan.expiresAt) ||
    plan.expiresAt <= plan.createdAt ||
    typeof plan.snapshotGeneration !== "string" ||
    !plan.snapshotGeneration ||
    plan.snapshotRequestId !== request.request.snapshotRequestId ||
    plan.action !== request.request.intent.action ||
    !record(plan.capability) ||
    typeof plan.capability.canPreview !== "boolean" ||
    typeof plan.capability.canExecute !== "boolean" ||
    !Array.isArray(plan.steps) ||
    !Array.isArray(value.stepGraph) ||
    value.stepGraph.length !== plan.steps.length
  ) {
    throw invalid();
  }
  // v2 首切片只承载原执行器的线性步骤；稳定 ID 同时约束索引、缺项、重复和环。
  for (let index = 0; index < plan.steps.length; index += 1) {
    const step = plan.steps[index];
    const node = value.stepGraph[index];
    const expectedDependencies = index === 0 ? [] : [`${plan.planId}-step-${index - 1}`];
    if (
      !record(step) ||
      !record(node) ||
      node.stepId !== `${plan.planId}-step-${index}` ||
      node.stepIndex !== index ||
      !Array.isArray(node.dependsOn) ||
      node.dependsOn.length !== expectedDependencies.length ||
      node.dependsOn.some((dependency, position) => dependency !== expectedDependencies[position])
    ) {
      throw invalid();
    }
  }
  // 保留后端签发的 v1 payload，尤其不能推导或提升 canExecute。
  return value as unknown as OperationPlanV2;
}

export async function operationPlanV2(request: PlanRequestV2): Promise<OperationPlanV2> {
  validateRequest(request);
  const response: unknown = await invoke("operation_plan_v2", { request });
  return validateResponse(response, request);
}
