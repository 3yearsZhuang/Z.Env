# 文档索引

本目录收录 Z.Env 的架构方案、实施记录、发布运维与历史接手档案。
**阶段进度只以 [`../plan.md`](../plan.md) 为单一事实源**；本索引只负责导航与记录约定，
不重复维护完成状态。

## 建议阅读顺序

1. [`../README.md`](../README.md)：产品定位、当前能力与平台边界。
2. [`../plan.md`](../plan.md)：当前状态、已排定路线、下一步。
3. [`../CHANGELOG.md`](../CHANGELOG.md)：面向用户的变化摘要。
4. 需要了解实现细节时，按下表进入对应方案或实施记录。

## 架构与方案

| 文档 | 用途 |
| --- | --- |
| [跨平台实施方案](./cross-platform-implementation.md) | 七类工具、三平台完成线、十二个工作包与验收门槛（当前实施契约） |
| [重构规划](./refactor-plan.md) | 从 mise 图形界面转向本机工具中心的领域模型与六阶段路线 |
| [发布 Runbook](./release.md) | 版本、标签、三平台打包、更新元数据与历次踩坑 |

## 实施记录（按工作包）

| 文档 | 覆盖范围 |
| --- | --- |
| [P9 / 38 审计修复](./audit-fixes.md) | 项目绑定、TOML 配置、任务互斥、包 ID、Linux、快照、托管与日志脱敏 |
| [P10 / 39 前端优化](./frontend-polish.md) | 项目搜索分页、状态反馈、键盘操作、任务取消、按需加载 |
| [P11 契约与合并器](./p11-inventory-contracts.md) | 工具/来源/实例身份、证据合并、alias 与 Rust/TS 契约 |
| [P11 Node 来源与探针](./p11-node-sources.md) | mise / brew / nvm / PATH 的只读发现、受限探针与预算 |
| [P11 查询与详情](./p11-inventory-ui.md) | 清单查询/取消、来源重试、过期缓存与 Node 详情 |
| [P12 操作计划与中断记录](./p12-operation-plans.md) | 计划签发、单次消费、写锁重验、最小 journal 与取消 |
| [P12 Node 安装/卸载](./p12-node-operations.md) | mise / Homebrew 渠道、结果核验与失败/取消后重扫 |
| [P12 Node 绑定与补偿](./p12-node-bindings.md) | 直连、托管、项目 PATH 绑定与冲突补偿 |
| [P13 Node 项目环境](./p13-project-environment.md) | 声明、候选、配置选择与受控/应用 PATH/终端状态分别判断 |
| [P14 工具目录与导航](./p14-tool-catalog.md) | 七项工具定义、来源账本、实例投影与入口收敛 |
| [P14 Python 五来源](./p14-python-sources.md) | mise / Homebrew / pyenv / uv / PATH 的 Python 只读清单 |
| [P14 Python 计划与重验](./p14-python-plans.md) | 四渠道安装意图预览、受控重新验证与工具隔离 |
| [P14 Python mise 安装/卸载](./p14-python-mise.md) | core:python 精确预编译安装、受控设置与隔离实机闭环 |
| [P14 Python uv 安装/卸载](./p14-python-uv.md) | uv 托管 CPython、别名识别、受控环境与真实安装/卸载验收 |
| [跨平台执行底座](./cross-platform-foundation.md) | Windows 有界探针、Job 对象、稳定文件身份与显式环境策略 |
| [统一操作计划 v2](./operation-plan-v2.md) | tagged subject、步骤图投影、v1 兼容与执行边界 |

## 历史档案（只增不改）

| 文档 | 说明 |
| --- | --- |
| [开发志 P0–P8](./archive/devlog-P0-P8.md) | 2026-09-29 → 2026-10-02 的完整执行档案；条目编号被 `plan.md` 引用 |
| [接手记录 2026-10-19](./archive/workspace-handoff-2026-10-19.md) | 规划接手、对话汇总与验证边界 |
| [接手记录 2026-10-20](./archive/workspace-handoff-2026-10-20.md) | 三平台扩展接手、恢复包与归档核实 |

## 记录约定

- 状态标记：✅ 已完成 · 🚧 进行中 · ⬜ 待办；完成一项就更新 [`../plan.md`](../plan.md)。
- 阶段收口时在 `plan.md`「当前状态」补一行，并在 `CHANGELOG.md` 补对应版本条目（可随发版补）。
- 实施记录沿用「动因 / 落地 / 验证记录」结构，分别写明自动验证、实机验证与未覆盖边界。
- **实施记录中的「未提交 / 未推送 / 未发布」反映记录撰写当时的状态**；2026-10-20 收口后，
  P9–P14 的实现已提交并合入 `main`，但各记录中的测试数字仍是当时的基线，不作为当前验证结果。
- 历史档案（`docs/archive/`）只增不改；需要更正时在新记录中说明，不回写旧档案。
- 文档中的相对链接由 `npm run check:docs` 校验；移动或重命名文档后必须同步链接。
