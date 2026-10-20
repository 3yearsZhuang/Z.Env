import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import contract from "../../fixtures/operations/contract-v2.json";
import { operationExecute, operationPlan } from "./operationPlan";
import { operationPlanV2, type OperationPlanV2, type PlanRequestV2 } from "./operationPlanV2";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => vi.resetAllMocks());

const nodeRequest = contract.node.request as PlanRequestV2;
const pythonRequest = contract.python.request as PlanRequestV2;
function nodeResponse(): OperationPlanV2 {
  return structuredClone(contract.node.response) as OperationPlanV2;
}

describe("v2 操作计划边界", () => {
  it.each(["node", "python"] as const)("%s 共享契约原样往返，不改写原计划", async (toolId) => {
    const fixture = contract[toolId];
    vi.mocked(invoke).mockResolvedValue(fixture.response);
    const result = await operationPlanV2(fixture.request as PlanRequestV2);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("operation_plan_v2", {
      request: fixture.request,
    });
    expect(result).toBe(fixture.response);
    expect(result.plan.toolId ?? "node").toBe(toolId);
    expect(result.stepGraph[2].dependsOn).toEqual([`${result.plan.planId}-step-1`]);
  });

  it.each([nodeRequest, pythonRequest])("旧 helper 委托 v2，执行仍只传计划 ID", async (request) => {
    const response = contract[request.subject.toolId].response;
    vi.mocked(invoke)
      .mockResolvedValueOnce(response)
      .mockResolvedValueOnce({ id: response.plan.planId });
    const plan = await operationPlan(request.request, request.subject.toolId);
    expect(plan).toBe(response.plan);
    expect(invoke).toHaveBeenNthCalledWith(1, "operation_plan_v2", { request });
    await operationExecute(plan.planId);
    expect(invoke).toHaveBeenNthCalledWith(2, "operation_execute", { planId: plan.planId });
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("不会将服务端不可执行预览提升为可执行", async () => {
    const response = nodeResponse();
    response.plan.capability.canExecute = false;
    response.plan.capability.reason = "仅预览";
    vi.mocked(invoke).mockResolvedValue(response);
    const result = await operationPlanV2(nodeRequest);
    expect(result.plan.capability).toEqual(response.plan.capability);
    expect(result.plan.capability.canExecute).toBe(false);
  });

  it.each([
    ["未来请求版本", { ...nodeRequest, schemaVersion: 3 }],
    ["未知工具", { ...nodeRequest, subject: { kind: "tool", toolId: "rust" } }],
    ["未知主体", { ...nodeRequest, subject: { kind: "recipe", toolId: "node" } }],
    [
      "未知动作",
      { ...nodeRequest, request: { ...nodeRequest.request, intent: { action: "execute" } } },
    ],
    ["空请求", null],
  ])("%s 不发出预览或执行请求", async (_label, request) => {
    await expect(operationPlanV2(request as PlanRequestV2)).rejects.toThrow("请求");
    expect(invoke).not.toHaveBeenCalled();
  });

  const invalidResponses: [string, (response: OperationPlanV2) => void][] = [
    ["未来响应版本", (response) => Object.assign(response, { schemaVersion: 3 })],
    ["未来内层计划版本", (response) => Object.assign(response.plan, { schemaVersion: 2 })],
    ["未知响应主体", (response) => Object.assign(response.subject, { kind: "recipe" })],
    [
      "响应工具串台",
      (response) => {
        response.subject.toolId = "python";
      },
    ],
    [
      "内层工具串台",
      (response) => {
        response.plan.toolId = "python";
      },
    ],
    ["工具为空并非缺省 Node", (response) => Object.assign(response.plan, { toolId: null })],
    [
      "清单串台",
      (response) => {
        response.plan.snapshotRequestId = "other-snapshot";
      },
    ],
    [
      "动作串台",
      (response) => {
        response.plan.action = "uninstall";
      },
    ],
    [
      "非法计划身份",
      (response) => {
        response.plan.planId = "../another-plan";
      },
    ],
    [
      "无限时间",
      (response) => {
        response.plan.expiresAt = Number.POSITIVE_INFINITY;
      },
    ],
    ["非数字时间", (response) => Object.assign(response.plan, { createdAt: "1792339200000" })],
    [
      "倒置有效期",
      (response) => {
        response.plan.expiresAt = response.plan.createdAt - 1;
      },
    ],
    [
      "非布尔执行能力",
      (response) => Object.assign(response.plan.capability, { canExecute: "false" }),
    ],
    [
      "空步骤元素",
      (response) => {
        response.plan.steps[1] = null as never;
      },
    ],
    ["步骤不是数组", (response) => Object.assign(response.plan, { steps: null })],
    [
      "坏依赖",
      (response) => {
        response.stepGraph[2].dependsOn = [response.stepGraph[0].stepId];
      },
    ],
    [
      "依赖成环",
      (response) => {
        response.stepGraph[0].dependsOn = [response.stepGraph[2].stepId];
      },
    ],
    [
      "重复步骤身份",
      (response) => {
        response.stepGraph[1].stepId = response.stepGraph[0].stepId;
      },
    ],
    [
      "重复索引",
      (response) => {
        response.stepGraph[1].stepIndex = 0;
      },
    ],
    [
      "缺少步骤",
      (response) => {
        response.stepGraph.pop();
      },
    ],
    [
      "额外索引",
      (response) => {
        response.stepGraph[2].stepIndex = 3;
      },
    ],
    [
      "额外步骤",
      (response) => {
        response.stepGraph.push({ stepId: "extra", stepIndex: 3, dependsOn: [] });
      },
    ],
    ["图不是数组", (response) => Object.assign(response, { stepGraph: null })],
  ];
  it.each(invalidResponses)("拒绝%s，且不回退旧 endpoint 或请求执行", async (_label, mutate) => {
    const response = nodeResponse();
    mutate(response);
    vi.mocked(invoke).mockResolvedValue(response);
    await expect(operationPlanV2(nodeRequest)).rejects.toThrow("响应");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("operation_plan_v2", { request: nodeRequest });
  });

  it.each([
    null,
    undefined,
    [],
    {},
    { schemaVersion: 2, subject: null },
    { ...contract.node.response, plan: null },
  ])("异常响应安全拒绝而非解引用空值", async (response) => {
    vi.mocked(invoke).mockResolvedValue(response);
    await expect(operationPlanV2(nodeRequest)).rejects.toThrow("响应");
  });

  it("Python 不接受省略工具身份的 Node 计划", async () => {
    const response = structuredClone(contract.python.response);
    delete (response.plan as { toolId?: string }).toolId;
    vi.mocked(invoke).mockResolvedValue(response);
    await expect(operationPlanV2(pythonRequest)).rejects.toThrow("响应");
  });

  it("后端拒绝原样传播且不调用旧 endpoint", async () => {
    const error = new Error("清单已变化");
    vi.mocked(invoke).mockRejectedValue(error);
    await expect(operationPlanV2(nodeRequest)).rejects.toBe(error);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("operation_plan_v2", { request: nodeRequest });
  });
});
