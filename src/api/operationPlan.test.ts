import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { operationExecute, operationPlan } from "./operationPlan";
import v2Contract from "../../fixtures/operations/contract-v2.json";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
describe("后端签发计划的协议", () => {
  it("预览传递清单身份与意图，执行只传后端计划 ID", async () => {
    vi.mocked(invoke).mockResolvedValue(v2Contract.node.response);
    const request = v2Contract.node.request.request as PlanRequest;
    await operationPlan(request);
    expect(invoke).toHaveBeenCalledWith("operation_plan_v2", {
      request: v2Contract.node.request,
    });
    await operationExecute("server-plan");
    expect(invoke).toHaveBeenLastCalledWith("operation_execute", { planId: "server-plan" });
  });
});

import contract from "../../fixtures/operations/contract-v1.json";
import type { OperationPlan, ExecutionRecord, PlanRequest } from "./operationPlan";
it("共享契约保留空值、来源身份和中断步骤", () => {
  const request = contract.request as PlanRequest;
  const plan = contract.plan as OperationPlan;
  const record = contract.record as ExecutionRecord;
  expect(request.intent.action).toBe("verify");
  expect(plan.configuration.sha256).toBeNull();
  expect(plan.target.providerPackage.providerId).toBe("nvm");
  expect(plan.steps[0].program).toBeNull();
  expect(record.status).toBe("interrupted");
  expect(record.steps[2].status).toBe("skipped");
});

it("Python 预览单独传递工具范围，执行仍只提交后端 ID", async () => {
  vi.mocked(invoke).mockResolvedValue(v2Contract.python.response);
  const request = v2Contract.python.request.request as PlanRequest;
  await operationPlan(request, "python");
  expect(invoke).toHaveBeenLastCalledWith("operation_plan_v2", {
    request: v2Contract.python.request,
  });
  await operationExecute("python-plan");
  expect(invoke).toHaveBeenLastCalledWith("operation_execute", { planId: "python-plan" });
});

import pythonContract from "../../fixtures/operations/python-v1.json";
it("共享 Python 契约保留范围、只读步骤和四渠道不可执行预览", () => {
  const request = pythonContract.scope.request as PlanRequest;
  const verify = pythonContract.verifyPlan as OperationPlan;
  const previews = pythonContract.installPlans as OperationPlan[];
  const record = pythonContract.record as ExecutionRecord;
  expect(request.snapshotRequestId).toBe(verify.snapshotRequestId);
  expect(verify.toolId).toBe("python");
  expect(record.toolId).toBe("python");
  expect(verify.steps.map((s) => s.kind)).toEqual(["preflight", "refresh", "verify"]);
  expect(previews.map((p) => p.target.providerPackage.providerId)).toEqual([
    "mise",
    "brew",
    "pyenv",
    "uv",
  ]);
  expect(previews.every((p) => p.toolId === "python" && !p.capability.canExecute)).toBe(true);
});

import pythonMiseContract from "../../fixtures/operations/python-mise-v1.json";
it("Python mise 可执行契约明确原生包、固定命令与隔离环境键", () => {
  for (const value of Object.values(pythonMiseContract)) {
    const plan = value as OperationPlan;
    expect(plan.toolId).toBe("python");
    expect(plan.target.providerPackage).toEqual({
      providerId: "mise",
      packageId: "python",
      kind: "runtime",
      platform: "macos",
    });
    expect(plan.steps[1].argv.slice(0, 3)).toEqual(["--no-config", "--no-env", "--no-hooks"]);
    expect(plan.steps[1].argv[4]).toBe("core:python@3.12.12");
    expect(plan.steps[1].environmentKeys).toContain("MISE_PYTHON_DEFAULT_PACKAGES_FILE");
  }
});
