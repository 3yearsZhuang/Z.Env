/** 配方契约的纯解码器；不调用 IPC，也不签发或提升执行能力。 */
export type RecipeTool = "node" | "python";
export interface RecipePlanRequestV2 {
  schemaVersion: 2;
  subject: { kind: "recipe" };
  request: {
    projectPath: string;
    binding: { scope: "project"; strategy: "direct" };
    items: {
      toolId: RecipeTool;
      version: string;
      snapshotRequestId: string;
      selection: { kind: "install" } | { kind: "reuse"; installationId: string };
    }[];
  };
}

export function recipeObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function recipeKeys(
  value: unknown,
  required: readonly string[],
  optional: readonly string[] = [],
): value is Record<string, unknown> {
  return (
    recipeObject(value) &&
    required.every((key) => Object.prototype.hasOwnProperty.call(value, key)) &&
    Object.keys(value).every((key) => required.includes(key) || optional.includes(key))
  );
}

export function recipeId(value: unknown): value is string {
  return typeof value === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(value);
}

export function recipeTimestamp(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

export function recipeProjectPath(value: unknown): value is string {
  return (
    recipeText(value, 4096) &&
    value.startsWith("/") &&
    value !== "/" &&
    !/\\/u.test(value) &&
    value
      .slice(1)
      .split("/")
      .every((part) => part !== "" && part !== "." && part !== "..")
  );
}

export function recipeText(value: unknown, maxBytes: number, allowEmpty = false): value is string {
  return (
    typeof value === "string" &&
    (allowEmpty || value.length > 0) &&
    !/[\p{Cc}\uD800-\uDFFF]/u.test(value) &&
    new TextEncoder().encode(value).length <= maxBytes
  );
}

export function recipeTool(value: unknown): value is RecipeTool {
  return value === "node" || value === "python";
}

export function recipeVersion(tool: RecipeTool, value: unknown): value is string {
  if (typeof value !== "string") return false;
  const parts = value.split(".");
  const limit = tool === "node" ? 10 : 4;
  if (
    parts.length !== 3 ||
    !parts.every(
      (part) =>
        part.length >= 1 &&
        part.length <= limit &&
        /^[0-9]+$/.test(part) &&
        (part.length === 1 || !part.startsWith("0")),
    )
  ) {
    return false;
  }
  return tool === "node" || (parts[0] === "3" && Number(parts[1]) >= 8 && Number(parts[1]) < 100);
}

export function recipeSubject(value: unknown): boolean {
  return recipeKeys(value, ["kind"]) && value.kind === "recipe";
}

export function readRecipeRequestV2(value: unknown): RecipePlanRequestV2 {
  const invalid = () => new Error("配方 v2 请求无效或超出当前范围");
  if (
    !recipeKeys(value, ["schemaVersion", "subject", "request"]) ||
    value.schemaVersion !== 2 ||
    !recipeSubject(value.subject) ||
    !recipeKeys(value.request, ["projectPath", "binding", "items"])
  ) {
    throw invalid();
  }
  const request = value.request;
  if (
    !recipeProjectPath(request.projectPath) ||
    !recipeKeys(request.binding, ["scope", "strategy"]) ||
    request.binding.scope !== "project" ||
    request.binding.strategy !== "direct" ||
    !Array.isArray(request.items) ||
    request.items.length < 1 ||
    request.items.length > 2
  ) {
    throw invalid();
  }
  const tools = new Set<RecipeTool>();
  for (const item of request.items) {
    if (
      !recipeKeys(item, ["toolId", "version", "snapshotRequestId", "selection"]) ||
      !recipeTool(item.toolId) ||
      tools.has(item.toolId) ||
      !recipeVersion(item.toolId, item.version) ||
      !recipeId(item.snapshotRequestId) ||
      !recipeObject(item.selection)
    ) {
      throw invalid();
    }
    tools.add(item.toolId);
    if (!(
      (recipeKeys(item.selection, ["kind"]) && item.selection.kind === "install") ||
      (recipeKeys(item.selection, ["kind", "installationId"]) &&
        item.selection.kind === "reuse" &&
        recipeId(item.selection.installationId))
    )) {
      throw invalid();
    }
  }
  return value as unknown as RecipePlanRequestV2;
}

export const recipeItemPhases = [
  "select",
  "installPreflight",
  "installCommand",
  "installRefresh",
  "installVerify",
  "bindPreflight",
  "bindCommand",
  "bindRefresh",
  "bindVerify",
  "compensate",
  "reconcile",
] as const;
export type RecipePhase = "preflight" | (typeof recipeItemPhases)[number] | "finalize";
export interface RecipeStepIdentityV2 {
  index: number;
  itemIndex: number | null;
  phase: RecipePhase;
}

export function recipeStepTemplate(itemCount: number): RecipeStepIdentityV2[] {
  if (itemCount !== 1 && itemCount !== 2) throw new Error("配方工具数量超出预算");
  const steps: RecipeStepIdentityV2[] = [{ index: 0, itemIndex: null, phase: "preflight" }];
  for (let itemIndex = 0; itemIndex < itemCount; itemIndex += 1) {
    for (const phase of recipeItemPhases) steps.push({ index: steps.length, itemIndex, phase });
  }
  steps.push({ index: steps.length, itemIndex: null, phase: "finalize" });
  return steps;
}

interface RecipeFingerprint {
  path: string;
  canonicalPath: string | null;
  exists: boolean;
  sha256: string | null;
}
export interface RecipeOperationPlanV2 {
  schemaVersion: 2;
  subject: { kind: "recipe" };
  plan: {
    planId: string;
    createdAt: number;
    expiresAt: number;
    projectDirectory: string;
    configuration: RecipeFingerprint;
    configurationDiff: {
      path: string;
      field: string;
      before: string | null;
      after: string | null;
    }[];
    capability: { canPreview: true; canExecute: false; reason: string };
    items: {
      toolId: RecipeTool;
      version: string;
      snapshotRequestId: string;
      snapshotGeneration: string;
      providerPackage: {
        providerId: "mise";
        packageId: RecipeTool;
        kind: "runtime";
        platform: "macos";
      };
      target: { kind: "install" } | { kind: "reuse"; installationId: string; locator: string };
      allowedConfigurationField: "tools.node" | "tools.python";
      declarations: { state: "matched" | "unconstrained"; files: RecipeFingerprint[] };
    }[];
    steps: RecipeStepIdentityV2[];
  };
  stepGraph: { stepId: string; stepIndex: number; dependsOn: string[] }[];
}

function fingerprint(value: unknown): value is RecipeFingerprint & Record<string, unknown> {
  return (
    recipeKeys(value, ["path", "canonicalPath", "exists", "sha256"]) &&
    recipeProjectPath(value.path) &&
    (value.canonicalPath === null || value.canonicalPath === value.path) &&
    typeof value.exists === "boolean" &&
    (value.exists
      ? typeof value.sha256 === "string" && /^[0-9a-f]{64}$/.test(value.sha256)
      : value.sha256 === null)
  );
}

function generation(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^(0|[1-9][0-9]{0,19})$/.test(value) &&
    BigInt(value) <= 18446744073709551615n
  );
}

export function readRecipePlanV2(value: unknown): RecipeOperationPlanV2 {
  const invalid = () => new Error("配方 v2 计划无效或超出当前范围");
  if (
    !recipeKeys(value, ["schemaVersion", "subject", "plan", "stepGraph"]) ||
    value.schemaVersion !== 2 ||
    !recipeSubject(value.subject) ||
    !recipeKeys(value.plan, [
      "planId",
      "createdAt",
      "expiresAt",
      "projectDirectory",
      "configuration",
      "configurationDiff",
      "capability",
      "items",
      "steps",
    ])
  ) {
    throw invalid();
  }
  const plan = value.plan;
  if (
    !recipeId(plan.planId) ||
    !recipeTimestamp(plan.createdAt) ||
    !recipeTimestamp(plan.expiresAt) ||
    plan.expiresAt <= plan.createdAt ||
    plan.expiresAt - plan.createdAt > 120000 ||
    !recipeProjectPath(plan.projectDirectory) ||
    !fingerprint(plan.configuration) ||
    ![`${plan.projectDirectory}/mise.toml`, `${plan.projectDirectory}/.mise.toml`].includes(
      plan.configuration.path,
    ) ||
    !recipeKeys(plan.capability, ["canPreview", "canExecute", "reason"]) ||
    plan.capability.canPreview !== true ||
    plan.capability.canExecute !== false ||
    !recipeText(plan.capability.reason, 512) ||
    plan.capability.reason.trim().length === 0 ||
    !Array.isArray(plan.items) ||
    plan.items.length < 1 ||
    plan.items.length > 2 ||
    !Array.isArray(plan.configurationDiff) ||
    plan.configurationDiff.length > plan.items.length
  ) {
    throw invalid();
  }
  const tools = new Map<RecipeTool, string>();
  for (const item of plan.items) {
    if (
      !recipeKeys(item, [
        "toolId",
        "version",
        "snapshotRequestId",
        "snapshotGeneration",
        "providerPackage",
        "target",
        "allowedConfigurationField",
        "declarations",
      ]) ||
      !recipeTool(item.toolId) ||
      tools.has(item.toolId) ||
      !recipeVersion(item.toolId, item.version) ||
      !recipeId(item.snapshotRequestId) ||
      !generation(item.snapshotGeneration) ||
      !recipeKeys(item.providerPackage, ["providerId", "packageId", "kind", "platform"]) ||
      item.providerPackage.providerId !== "mise" ||
      item.providerPackage.packageId !== item.toolId ||
      item.providerPackage.kind !== "runtime" ||
      item.providerPackage.platform !== "macos" ||
      item.allowedConfigurationField !== `tools.${item.toolId}` ||
      !recipeKeys(item.declarations, ["state", "files"]) ||
      !["matched", "unconstrained"].includes(item.declarations.state as string) ||
      !Array.isArray(item.declarations.files) ||
      item.declarations.files.length > 8
    ) {
      throw invalid();
    }
    tools.set(item.toolId, item.version);
    if (!(
      (recipeKeys(item.target, ["kind"]) && item.target.kind === "install") ||
      (recipeKeys(item.target, ["kind", "installationId", "locator"]) &&
        item.target.kind === "reuse" &&
        recipeId(item.target.installationId) &&
        recipeProjectPath(item.target.locator))
    )) {
      throw invalid();
    }
    const paths = new Set<string>();
    for (const file of item.declarations.files) {
      if (
        !fingerprint(file) ||
        !file.path.startsWith(`${plan.projectDirectory}/`) ||
        paths.has(file.path)
      ) {
        throw invalid();
      }
      paths.add(file.path);
    }
  }
  const fields = new Set<string>();
  for (const diff of plan.configurationDiff) {
    if (
      !recipeKeys(diff, ["path", "field", "before", "after"]) ||
      diff.path !== plan.configuration.path ||
      typeof diff.field !== "string" ||
      fields.has(diff.field) ||
      ![...tools.keys()].some(
        (tool) => diff.field === `tools.${tool}` && diff.after === tools.get(tool),
      ) ||
      !(diff.before === null || recipeText(diff.before, 128, true))
    ) {
      throw invalid();
    }
    fields.add(diff.field);
  }
  const template = recipeStepTemplate(plan.items.length);
  if (
    !Array.isArray(plan.steps) ||
    plan.steps.length !== template.length ||
    !Array.isArray(value.stepGraph) ||
    value.stepGraph.length !== template.length
  ) {
    throw invalid();
  }
  for (const expected of template) {
    const step = plan.steps[expected.index];
    const graph = value.stepGraph[expected.index];
    const dependencies = expected.index === 0 ? [] : [`${plan.planId}-step-${expected.index - 1}`];
    if (
      !recipeKeys(step, ["index", "itemIndex", "phase"]) ||
      step.index !== expected.index ||
      step.itemIndex !== expected.itemIndex ||
      step.phase !== expected.phase ||
      !recipeKeys(graph, ["stepId", "stepIndex", "dependsOn"]) ||
      graph.stepId !== `${plan.planId}-step-${expected.index}` ||
      graph.stepIndex !== expected.index ||
      !Array.isArray(graph.dependsOn) ||
      graph.dependsOn.length !== dependencies.length ||
      graph.dependsOn.some((dependency, index) => dependency !== dependencies[index])
    ) {
      throw invalid();
    }
  }
  return value as unknown as RecipeOperationPlanV2;
}
