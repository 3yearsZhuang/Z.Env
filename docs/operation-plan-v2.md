# 统一操作计划 v2：工具目标与步骤身份

日期：2026-10-20（Asia/Shanghai），工作包 02 的后续切片。
状态：Node/Python v2 传输入口接入；七工具、配方与运维目标仍按活计划继续。

## 动因

旧预览命令在请求之外传递可省略的 toolId，Node 依赖缺省值；未来配方、服务与快照
不能继续冒充 Node 目标。先用明确版本与 tagged subject 包裹现有计划，保持同一执行服务，
再按领域开放新目标。v2 不是新的命令执行通道。

## 落地

- `operation_plan_v2` 接收 `{schemaVersion:2, subject:{kind:"tool",toolId}, request}`，
  `request` 仍为原始 snapshotRequestId 与意图。仅 Node/Python 开放；其他工具及 subject
  返回不支持，版本、未知字段和前端拼接 program/argv/steps 在签发前拒绝。
- 返回 `{schemaVersion:2, subject, plan, stepGraph}`；`plan` 保留原 v1 字段与能力，
  Node 的旧省略字段与 Python 的显式字段兼容。图中每项含 stepId、stepIndex 和 dependsOn。
  既有步骤按顺序链接，stepId 为本次 planId 加步骤序号；同一签发计划的转换稳定。
- 这些 ID 仅在本次签发内稳定，不跨重新预览复用；当前图是线性投影，尚非配方分支调度器。
  未来依赖图和失败重试在原执行器内扩展，不在前端循环执行一组任意命令。
- 原 Node/Python 预览委托同一 OperationPlanService，复用原 StoredPlan、写锁、有效期、
  目标和配置指纹、取消、终态重扫及最小 journal。`operation_execute` 仍只接收 planId。
  旧预览 IPC 继续保留；不重放 journal、不把响应中的步骤重新当作执行请求。
- 前端 `operationPlan` 的调用形式与返回值保持兼容，内部改走 v2；新 helper 对版本、工具、
  快照、动作、时间、能力字段与图结构作校验，再返回原计划。失败不会自动回退旧入口。
  计划弹窗原有实例/渠道校验和写能力限制继续适用。
- Rust 与 TypeScript 使用同一 [v2 样本](../fixtures/operations/contract-v2.json)，
  原 v1、Python 及 Python mise 样本继续保留为兼容回归。

## 验证边界

- 前端：`src/api/operationPlanV2.test.ts` 41 项通过，覆盖版本/工具/步骤图、错快照、
  未知字段与跨工具回复拒绝；v1 计划样本继续作为兼容回归。
- Rust：全量 245 项通过（见本轮收口的 [uv 记录](./p14-python-uv.md)），v2 签发复用
  既有一次性执行；真实本机 `operation_plan_v2` helper 完成 v2 签发与旧入口消费。
- 结果按“单元测试 / 真实本机查询与计划服务 / 打包桌面 IPC”分开记账；本轮未重新验收打包桌面 IPC。
- 尚未开放 Java/Go/Rust/Git/uv 工具目标、升级、配方、服务、缓存或快照的 v2 执行能力；
  不以新增 endpoint 或步骤图宣告整个统一计划工作包完成。
