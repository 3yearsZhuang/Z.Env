import { describe, expect, it } from "vitest";
import contract from "../../fixtures/operations/recipe-contract-v2.json";
import records from "../../fixtures/operations/recipe-record-v2.json";
import legacyNode from "../../fixtures/operations/contract-v1.json";
import legacyPython from "../../fixtures/operations/python-v1.json";
import { readRecipePlanV2, readRecipeRequestV2, recipeStepTemplate } from "./recipeOperationV2";
import { readExecutionRecordEnvelope, readRecipeRecordV2 } from "./recipeRecordV2";
import { materializeRecipeFixture } from "./recipeFixtureCases";

describe("配方 v2 共享契约", () => {
  it.each(Object.entries(contract.requests))(
    "%s 请求原样读取且不补填执行证据",
    (_name, request) => {
      expect(readRecipeRequestV2(request)).toBe(request);
    },
  );
  it.each(Object.entries(contract.plans))(
    "%s 计划固定身份、来源和阶段，执行保持关闭",
    (_name, plan) => {
      expect(readRecipePlanV2(plan)).toBe(plan);
      expect(plan.plan.capability.canExecute).toBe(false);
    },
  );
  it.each(contract.invalidRequests)("拒绝共享请求反例 $name", (sample) => {
    const value = materializeRecipeFixture(contract.requests, sample);
    expect(() => readRecipeRequestV2(value)).toThrow("请求");
  });
  it.each(contract.invalidPlans)("拒绝共享计划反例 $name", (sample) => {
    const value = materializeRecipeFixture(contract.plans, sample);
    expect(() => readRecipePlanV2(value)).toThrow("计划");
  });
  it.each(Object.entries(records.records))("%s 记录保留各工具结果与终态", (_name, record) => {
    expect(readRecipeRecordV2(record)).toBe(record);
    expect(readExecutionRecordEnvelope(record)).toBe(record);
  });
  it.each(records.invalidRecords)("拒绝共享记录反例 $name，不能回退旧 Node", (sample) => {
    const value = materializeRecipeFixture(records.records, sample);
    expect(() => readRecipeRecordV2(value)).toThrow("记录");
    expect(() => readExecutionRecordEnvelope(value)).toThrow("记录");
  });
  it("旧记录的 Node 缺省与 Python 显式身份保持兼容", () => {
    expect(readExecutionRecordEnvelope(legacyNode.record)).toBe(legacyNode.record);
    expect(readExecutionRecordEnvelope(legacyPython.record)).toBe(legacyPython.record);
    expect(legacyNode.record).not.toHaveProperty("toolId");
    expect(legacyPython.record.toolId).toBe("python");
  });
  it.each(Object.entries(records.legacyRecords))("旧格式 %s 保留兼容语义", (_name, record) => {
    const expected = "outcome" in record ? record : { ...record, outcome: null };
    expect(readExecutionRecordEnvelope(record)).toEqual(expected);
  });
  it("非法新版本不能被旧字段组合读成 Node 任务", () => {
    for (const schemaVersion of [0, 2, 3, "1", null]) {
      expect(() => readExecutionRecordEnvelope({ ...legacyNode.record, schemaVersion })).toThrow();
    }
    expect(() =>
      readExecutionRecordEnvelope({ ...legacyNode.record, subject: { kind: "recipe" } }),
    ).toThrow();
  });
  it("UTF-8 路径预算与 Rust 字节预算一致", () => {
    const request = structuredClone(contract.requests.nodeInstall);
    request.request.projectPath = `/${"界".repeat(1365)}`;
    expect(readRecipeRequestV2(request)).toBe(request);
    request.request.projectPath += "界";
    expect(() => readRecipeRequestV2(request)).toThrow();
    request.request.projectPath = "/bad\ud800";
    expect(() => readRecipeRequestV2(request)).toThrow();
  });
  it("固定模板预算同时包含补偿和终态重扫", () => {
    expect(recipeStepTemplate(1)).toHaveLength(13);
    const steps = recipeStepTemplate(2);
    expect(steps).toHaveLength(24);
    expect(steps.filter((step) => step.phase === "compensate")).toHaveLength(2);
    expect(steps.filter((step) => step.phase === "reconcile")).toHaveLength(2);
    expect(() => recipeStepTemplate(0)).toThrow();
    expect(() => recipeStepTemplate(3)).toThrow();
  });
  it("禁用原因须能显示给用户，不能只有空白", () => {
    const plan = structuredClone(contract.plans.mixed);
    plan.plan.capability.reason = "   ";
    expect(() => readRecipePlanV2(plan)).toThrow("计划");
  });
});
