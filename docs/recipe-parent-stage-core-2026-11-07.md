# 第 51 项：父作用域与内部阶段核心

日期：2026-11-07。实施基线：已合并的 [PR #12](https://github.com/3yearsZhuang/Z.Env/pull/12)，`22c73d5`。
状态：内部执行接口已接入旧 Node/Python Tool；Recipe 仍在纯契约校验后返回 `Disabled`，`recipe.executable=false`。

## 动因与落地

旧共用循环同时负责进入应用任务、消费批准、创建 v1 记录和驱动阶段，不能直接嵌套执行一批子计划。
本切片把批准与记录留在 Tool 包装层，为后续父任务提供在同一写作用域内串接阶段的接口。

- [operations.rs](../src-tauri/src/operations.rs) 新增内部 `run_scoped`。`WriteScope` 只在持有 Guard 时构造并借给同步闭包，提供父 ID、取消查询、日志与阶段 query ID；引用不能跨线程共享。旧 `run` 的调用形式和终态规则保持兼容。
- [Tool 包装层](../src-tauri/src/operation_plan/mod.rs) 的 `execute_tool_with_context` 保留能力检查、单次消费、120 秒批准、初次证据校验、Running 记录落盘、终态投影与具体 `PlanError`。Command 之前仍重新读取上下文并校验冻结的目标、清单与配置。
- [stages.rs](../src-tauri/src/operation_plan/stages.rs) 接受借来的父作用域、后端阶段与批准重验/驱动/记录回调，承接执行循环、失败补偿和必要终态扫描；不消费计划、不初始化 journal、不结束父任务。进入 Command 后保守标记可能发生变更，驱动失败或恐慌仍核对残留。
- Node/Python 驱动仍持有各自的 query lease 与事务；租约在写前 `mark_mutating`，由后续扫描承接。父 Guard 不预占 query lease。Python 纯验证扫描保留父取消观察，写后与补偿扫描保留独立取消边界。

阶段 query ID 为 `stage-{16位父nonce}-i{item}-{phase}-s{sequence}`。nonce 在进程内按 checked 序号唯一，
工具项限定 0/1，phase 限定 `command`、`refresh`、`compensate`；同一父作用域最多生成 256 个身份，
逐个通过真实 query ID 校验器。原父 ID 不参与拼接，避免长 ID 超过 128 字节。父取消与日志继续使用父 ID。
同一次写租约的重扫保留租约身份；新租约使用新阶段身份，query 的 request/generation 所有权与历史拒绝规则保持。

## 验证范围

新增确定性回归覆盖父 ID/日志、阶段内与阶段间嵌套拒绝、独立线程排斥、跨线程取消、取消后清理期间持锁、
panic 恢复之前的线程上下文与释放 Guard、有界 query ID、真实租约接受与历史重复拒绝。
两组假阶段共用父任务与 Node/Python 查询槽，第二组 Command 取消后只做补偿和重扫；清理临时清除线程取消上下文时，
scope 仍保持父取消与持锁身份。既有 Tool 一次消费、过期/证据变化、记录故障、解析失败/外改补偿等回归继续验证包装层。

冻结源码的 Rust 356/356、前端 571/571 通过；格式、lint/clippy、构建、版本与文档检查通过，
生产和完整依赖审计均为 0 漏洞，独立源码复审 PASS。新增回归共 6 项；远端三平台门禁以 PR 为准。
测试与假阶段不作为配方真实安装、配置编排或桌面 IPC 已验收的证明。

## 后续切片

下一步拆分未签发准备与签发，在安装后以实际实例和父批准的冻结项目/配置/字段物化绑定，不能提前签一批 Tool 计划。
随后增加本父阶段提交并核验的 scan receipt，拒绝把任意最新缓存当作自身更新；再独立接入混合 v2 journal 与 Recipe 任务投影。
首次批准 Recipe 父记录才升级 journal，启动或 Tool-only 更新不迁移；旧 binary 会拒绝 v2 文档，回退时必须保全完整混合记录。

当前 live journal 仍为 v1，Recipe 父任务、实际安装/配置串接、新 UI 和失败项重签尚未接入。
真实 WKWebView 验收仍受 ScreenCaptureKit `-3811` 影响；桌面解锁与屏幕录制授权已由用户确认，根因未确认。
本切片未重建或验收 QA app，未发布，也未执行真实运行时安装或写用户项目。
