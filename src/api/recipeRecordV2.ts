import type { ExecutionRecord, ExecutionStatus } from "./operationPlan";
import {
  recipeId,
  recipeKeys,
  recipeObject,
  recipeStepTemplate,
  recipeSubject,
  recipeTimestamp,
  recipeTool,
  type RecipeStepIdentityV2,
  type RecipeTool,
} from "./recipeOperationV2";

type RecipeStepStatus = "planned" | "running" | "completed" | "failed" | "skipped" | "interrupted";
type RecipeItemStatus =
  "planned" | "running" | "completed" | "failed" | "cancelled" | "interrupted";
type RecipeItemOutcome =
  | "satisfied"
  | "configured"
  | "stepFailed"
  | "cancelled"
  | "interrupted"
  | "compensationConflict"
  | "scanIncomplete"
  | "preconditionChanged";
type RecipeOutcome = "recipeCompleted" | Exclude<RecipeItemOutcome, "satisfied" | "configured">;
type Compensation = "completed" | "conflict" | "failed";

export interface RecipeExecutionRecordV2 {
  schemaVersion: 2;
  subject: { kind: "recipe" };
  id: string;
  status: ExecutionStatus;
  createdAt: number;
  updatedAt: number;
  items: {
    toolId: RecipeTool;
    status: RecipeItemStatus;
    outcome: RecipeItemOutcome | null;
    compensation?: Compensation | null;
  }[];
  steps: (RecipeStepIdentityV2 & { status: RecipeStepStatus })[];
  outcome: RecipeOutcome | null;
}

const executionStatuses = ["running", "completed", "partial", "failed", "cancelled", "interrupted"];
const itemStatuses = ["planned", "running", "completed", "failed", "cancelled", "interrupted"];
const stepStatuses = ["planned", "running", "completed", "failed", "skipped", "interrupted"];
const itemOutcomes = [
  "satisfied",
  "configured",
  "stepFailed",
  "cancelled",
  "interrupted",
  "compensationConflict",
  "scanIncomplete",
  "preconditionChanged",
];
const parentOutcomes = ["recipeCompleted", ...itemOutcomes.slice(2)];
const compensations = ["completed", "conflict", "failed"];
const oneOf = (value: unknown, values: readonly string[]) =>
  typeof value === "string" && values.includes(value);
const nullable = (value: unknown, values: readonly string[]) =>
  value === null || oneOf(value, values);

/** 仅解码白名单传输模型；不恢复执行批准或写入 journal。 */
export function readRecipeRecordV2(value: unknown): RecipeExecutionRecordV2 {
  const invalid = () => new Error("配方 v2 记录无效或超出预算");
  if (
    !recipeKeys(value, [
      "schemaVersion",
      "subject",
      "id",
      "status",
      "createdAt",
      "updatedAt",
      "items",
      "steps",
      "outcome",
    ]) ||
    value.schemaVersion !== 2 ||
    !recipeSubject(value.subject) ||
    !recipeId(value.id) ||
    !oneOf(value.status, executionStatuses) ||
    !recipeTimestamp(value.createdAt) ||
    !recipeTimestamp(value.updatedAt) ||
    value.updatedAt < value.createdAt ||
    !nullable(value.outcome, parentOutcomes) ||
    !Array.isArray(value.items) ||
    value.items.length < 1 ||
    value.items.length > 2 ||
    !Array.isArray(value.steps)
  ) {
    throw invalid();
  }
  const tools = new Set<RecipeTool>();
  for (const item of value.items) {
    if (
      !recipeKeys(item, ["toolId", "status", "outcome"], ["compensation"]) ||
      !recipeTool(item.toolId) ||
      tools.has(item.toolId) ||
      !oneOf(item.status, itemStatuses) ||
      !nullable(item.outcome, itemOutcomes) ||
      !(item.compensation === undefined || nullable(item.compensation, compensations))
    ) {
      throw invalid();
    }
    tools.add(item.toolId);
  }
  const template = recipeStepTemplate(value.items.length);
  if (value.steps.length !== template.length) throw invalid();
  for (const expected of template) {
    const step = value.steps[expected.index];
    if (
      !recipeKeys(step, ["index", "itemIndex", "phase", "status"]) ||
      step.index !== expected.index ||
      step.itemIndex !== expected.itemIndex ||
      step.phase !== expected.phase ||
      !oneOf(step.status, stepStatuses)
    ) {
      throw invalid();
    }
  }
  const result = value as unknown as RecipeExecutionRecordV2;
  const unfinished = (status: RecipeStepStatus) => status === "planned" || status === "running";
  const successful = (status: RecipeStepStatus) => status === "completed" || status === "skipped";
  if (
    result.steps.filter((step) => step.status === "running").length > 1 ||
    result.items.filter((item) => item.status === "running").length > 1 ||
    (result.status !== "running" && result.steps.some((step) => unfinished(step.status)))
  ) {
    throw invalid();
  }
  switch (result.status) {
    case "running":
      if (result.outcome !== null) throw invalid();
      break;
    case "completed":
      if (
        result.outcome !== "recipeCompleted" ||
        result.items.some((item) => item.status !== "completed") ||
        result.steps.some((step) => !successful(step.status)) ||
        result.steps[0].status !== "completed" ||
        result.steps[result.steps.length - 1].status !== "completed"
      ) {
        throw invalid();
      }
      break;
    case "interrupted":
      if (result.outcome !== "interrupted") throw invalid();
      break;
    case "cancelled":
      if (result.outcome !== "cancelled") throw invalid();
      break;
    default:
      if ([null, "recipeCompleted", "interrupted", "cancelled"].includes(result.outcome))
        throw invalid();
  }
  for (const [index, item] of result.items.entries()) {
    const steps = result.steps.filter((step) => step.itemIndex === index);
    const success = item.outcome === "satisfied" || item.outcome === "configured";
    switch (item.status) {
      case "planned":
        if (
          item.outcome !== null ||
          item.compensation != null ||
          steps.some((step) => step.status !== "planned" && step.status !== "skipped")
        ) {
          throw invalid();
        }
        break;
      case "running":
        if (result.status !== "running" || item.outcome !== null) throw invalid();
        break;
      case "completed":
        if (
          !success ||
          item.compensation != null ||
          steps.some((step) => !successful(step.status)) ||
          steps[0].status !== "completed" ||
          steps[steps.length - 1].status !== "completed" ||
          steps.find((step) => step.phase === "compensate")!.status !== "skipped" ||
          (item.outcome === "configured" &&
            steps.some(
              (step) =>
                ["bindPreflight", "bindCommand", "bindRefresh", "bindVerify"].includes(
                  step.phase,
                ) && step.status !== "completed",
            )) ||
          (item.outcome === "satisfied" &&
            (steps.find((step) => step.phase === "bindCommand")!.status !== "skipped" ||
              steps.find((step) => step.phase === "bindVerify")!.status !== "completed"))
        ) {
          throw invalid();
        }
        break;
      case "interrupted":
        if (item.outcome !== "interrupted") throw invalid();
        break;
      case "failed":
        if (
          item.outcome === null ||
          success ||
          item.outcome === "interrupted" ||
          item.outcome === "cancelled"
        ) {
          throw invalid();
        }
        break;
      default:
        if (item.outcome === null || success || item.outcome === "interrupted") throw invalid();
    }
    if (
      ["completed", "failed", "cancelled", "interrupted"].includes(item.status) &&
      steps.some((step) => unfinished(step.status))
    ) {
      throw invalid();
    }
    const compensationPhase = steps.find((step) => step.phase === "compensate")!;
    if (compensationPhase.status === "completed") {
      if (item.compensation !== "completed") throw invalid();
    } else if (compensationPhase.status === "failed") {
      if (item.compensation !== "failed" && item.compensation !== "conflict") throw invalid();
    } else if (item.compensation != null) {
      throw invalid();
    }
  }
  if (
    result.steps.some(
      (step) =>
        step.status === "running" &&
        step.itemIndex !== null &&
        result.items[step.itemIndex].status !== "running",
    )
  ) {
    throw invalid();
  }
  return result;
}

/** 按版本分派；新 Recipe 格式缺失身份时直接拒绝。 */
export function readExecutionRecordEnvelope(
  value: unknown,
): ExecutionRecord | RecipeExecutionRecordV2 {
  if (!recipeObject(value)) throw new Error("操作记录格式无效");
  if (value.schemaVersion === 2) return readRecipeRecordV2(value);
  if (
    !recipeKeys(
      value,
      ["schemaVersion", "id", "action", "status", "createdAt", "updatedAt", "steps"],
      ["toolId", "compensation", "outcome"],
    ) ||
    value.schemaVersion !== 1 ||
    (value.toolId !== undefined && !recipeTool(value.toolId)) ||
    !recipeId(value.id) ||
    !oneOf(value.action, ["verify", "install", "uninstall", "bind", "unbind"]) ||
    !oneOf(value.status, executionStatuses) ||
    !recipeTimestamp(value.createdAt) ||
    !recipeTimestamp(value.updatedAt) ||
    !Array.isArray(value.steps) ||
    value.steps.length > 8 ||
    !(
      value.outcome === undefined ||
      nullable(value.outcome, [
        "verified",
        "installed",
        "uninstalled",
        "bound",
        "unbound",
        "compensationConflict",
        "scanIncomplete",
        "preconditionChanged",
        "cancelled",
        "stepFailed",
        "interrupted",
      ])
    ) ||
    !(value.compensation === undefined || nullable(value.compensation, compensations)) ||
    value.steps.some(
      (step) =>
        !recipeKeys(step, ["kind", "status"]) ||
        !oneOf(step.kind, ["preflight", "command", "refresh", "verify", "compensate"]) ||
        !oneOf(step.status, stepStatuses.slice(0, 5)),
    )
  ) {
    throw new Error("操作记录格式无效或超出预算");
  }
  return (value.outcome === undefined
    ? { ...value, outcome: null }
    : value) as unknown as ExecutionRecord;
}
