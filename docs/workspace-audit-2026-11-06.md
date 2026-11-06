# 工作区整理与 PR 质量审核（2026-11-06）

日期采用 Asia/Shanghai。首轮审核结论：**PR #7 修正两项 P2 回退后再合并；近期先校准能力和配方输入，
再打通 macOS Node/Python 项目流程与跨平台底座。** 迭代状态统一在 [活计划](../plan.md) 维护。
以下保留首轮整理的证据与当时边界；后续获授权的修正和验证见文末。
首轮仅整理工作区与文档，未修复功能、提交 PR 评论、合并、推送或发布。

## 审核范围与证据版本

| 项目 | 本次核实 |
| --- | --- |
| 主线 | `fe9130fd55b23ca6b7ad4e6afe69c7bcb294f049` |
| 唯一开放 PR | [#7：refactor(ui): 能力表并入软件渠道行（阶段一）](https://github.com/3yearsZhuang/Z.Env/pull/7) |
| PR head / base | `28d1783ae5097c917ec73bc2ba89b8667fc715ed` / `fe9130fd55b23ca6b7ad4e6afe69c7bcb294f049` |
| 差异 | 12 文件，273 行增加 / 614 行删除，前端与文档 |
| 远端状态 | 可合并、无现有 review；[CI run 42007312291](https://github.com/3yearsZhuang/Z.Env/actions/runs/42007312291) 前端与 Rust macOS/Windows/Linux 全部成功 |
| 最新公开版 | [v0.7.1](https://github.com/3yearsZhuang/Z.Env/releases/tag/v0.7.1)，北京时间 2026-10-03 01:00 发布，14 个资产；主线领先该标签 61 个提交 |

独立审查覆盖 diff、API 校验、后端能力账本、清单快照、配方与执行底座，以及 CI/发布边界。
远端状态为本次读取时点；后续评审或合并仍需核实新 head 与检查结果。

## PR #7：两项需要修正的回退

### P2：工具目录失败被隐藏，刷新无法恢复

[PR head 的 SoftwareView.tsx:328–334](https://github.com/3yearsZhuang/Z.Env/blob/28d1783ae5097c917ec73bc2ba89b8667fc715ed/src/components/SoftwareView.tsx#L328)
吞掉 `tool_catalog` IPC/校验失败；目录请求只依赖 `active`，刷新在 477–480 行只重读旧运行时、
环境与包映射。首次失败时能力标签全部消失，没有错误提示，在当前页点“刷新状态”也不会重试，
用户需切走再返回才能恢复。删除前的能力面板有独立错误提示和重试。

应给统一目录独立读取状态及错误/重试入口，保留已有渠道查看；重试成功后在当前页恢复能力标签。
定向场景确认首次失败无 alert，刷新后系统探测重跑而 `toolCatalog` 仍只有一次调用。

### P2：删除能力面板后丢失来源和各维度能力

[PR head 的 SoftwareView.tsx:700–702](https://github.com/3yearsZhuang/Z.Env/blob/28d1783ae5097c917ec73bc2ba89b8667fc715ed/src/components/SoftwareView.tsx#L700)
仅对旧系统包映射生成的渠道行读取 `installPlan`。旧映射为 brew/winget/apt/pacman；
删除原能力面板后，mise/nvm/pyenv/uv/PATH 目录来源，以及 `inventory`、`uninstallPlan`、
`bindingPlan`、`projectCheck` 都失去能力表展示。例如 Node nvm 绑定与 Python mise/uv 安装说明
不再可查，实际只迁移了部分安装标签。

应在卡片详情或展开行保留目录来源与各维度状态/原因，兼容渠道按既有能力保留。
定向场景确认返回了 nvm、绑定和项目核对信息，但界面未显示。

两项临时场景均通过“确认现有回退”的断言；它们不代表修正已经完成。
未发现新的执行器、正常缓存并发或兼容会话保留问题。标签旁继续旧直接安装是 PR 明确保留的阶段行为，
后续计划入口迁移前须让用户能判断点击将执行哪种流程；本轮未将它另列为新回退。

## 项目已有缺口与迭代约束

| 缺口 | owning source 与影响 | 建议切片 |
| --- | --- | --- |
| Python 能力账本陈旧 | [tool_catalog/mod.rs](../src-tauri/src/tool_catalog/mod.rs) 194–197 称项目核对未接入，243 称 brew/pyenv 安装执行未开放，252–272 未登记对应卸载与 mise 绑定；[python_binding.rs](../src-tauri/src/operation_plan/python_binding.rs) 544–569 已有 `mise which` 核验，实施记录已有真实写闭环 | 校准状态和理由，区分“可预览计划”与“本次计划可执行”；用真实目录契约验证展示 |
| 配方错误工具快照 | [lib.rs](../src-tauri/src/lib.rs) 691–693 用 `current_snapshot()`；[query.rs](../src-tauri/src/inventory/query.rs) 138–139 固定 Node；[recipe.rs](../src-tauri/src/recipe.rs) 116–121 从同一快照复用 Python、157 又把该 request ID 交给 Python 计划。只有 Python 清单时入口不可用，混合工具输入也不能正确核对 | 配方执行前先修正按工具取快照与版本绑定，覆盖真实 IPC 输入，接 Node 只读预览 |
| 配方约束顺序 | recipe.rs 116–128 命中健康实例就复用，130–140 才检查 Python 项目冲突，已有实例会跳过声明冲突 | 先核对项目要求，再判复用；加入“已装精确版本但项目声明冲突”场景 |
| v2/launcher 尚非完整执行平台 | [v2.rs](../src-tauri/src/operation_plan/v2.rs) 15–17、45–48 仅开放 Node/Python tool subject，78–87 为线性步骤图；[launcher 记录](./cross-platform-launcher.md) 仍无 UI/Tauri 命令、自有 shim 归属与安装删除 | 复用现有执行器与写互斥；先补 Windows 原生启动和 shim 归属，再纵向推广 Node 三平台 |
| 发布与实机未闭合 | 三平台 CI 已构建成功，但 Node Homebrew 写入/托管、真实 WebView IPC、授权/取消、终端生效与 updater 未由本轮验证；Intel Mac 实际产物仍未配置 | 各 OS/provider 独立保存完整写流程和保护路径变化；发布前核对产物与升级，不用测试总数关闭门槛 |

这些缺口在 PR #7 base 已存在，不作为该 UI PR 引入的问题。Python 虚拟环境创建、依赖同步和复杂配置
仍在当前范围之外；本轮仅规划已有虚拟环境与基础解释器的核对。

## 验证结果与依赖审计

| 检查 | 结果 |
| --- | --- |
| PR head 前端 | 43 文件 / 283 项通过；lint、Prettier、TypeScript/Vite build、版本一致性与文档链接通过 |
| PR head Rust macOS | 290 通过 / 0 失败 / 0 ignored；`cargo test --locked`、clippy all-targets `-D warnings` 与 fmt 通过 |
| PR 回退定向验证 | 临时测试 2 项通过；未修改共享源码 |
| 生产依赖审计 | 0 漏洞 |
| 完整依赖审计 | 10 项：1 high / 9 moderate，均 `fixAvailable: true`；当前 CI 完整审计为 advisory |

high 来自 dev 链 `jsdom@30.1.2 → undici@8.11.2`；当前生产锁图不包含该漏洞。
维护切片优先兼容更新至 undici 修复边界 8.12.2、vitest/mocker 4.1.12 和相关 minimatch 链，
运行现有前端完整门禁，修复后明确开发依赖 high 是否恢复阻断。
“Vite/launch-editor 暂无修复”的旧豁免说明已过时，本轮同步文档与 CI 注释，未改变审计执行策略。

本轮没有重新安装/卸载真实管理器软件、启动打包桌面完成 IPC 或验证 updater。
历史隔离实机记录保留工具版本和限定范围，不能当作当前三平台完整产品验收。

## 工作区整理与恢复

- 起始工作区没有未提交改动，只有一个 checkout；`fetch --prune` 清掉已删除远端 `docs/reorg` 的跟踪引用。
- 24 个历史本地分支已清理：23 个 commit 已在 `origin/main`，另一个 `pr1-fix-toml-parser`
  的全部 10 个提交在主线具有 patch-equivalent 提交。清理前已保存并验证完整 branches bundle 与 SHA 清单。
- 保留 `main` 与 PR #7 分支，另建立 `codex/workspace-audit-20261106`，从主线编写本记录和活计划。
  PR 源码未改；整理文档仅作本地提交，未推送。
- 验证后仅删除 `src-tauri/target/debug/incremental`，实测释放 **1,827,956 KiB（约 1.7 GiB）**。
  保留依赖、测试/示例二进制、release 和 Windows 构建产物以及 node_modules；后续 Cargo 会重建增量缓存。
- 整理前活计划历史状态保存在 [新历史快照](./archive/plan-status-2026-11-06.md)，原历史档案未修改。
  README、文档索引、发布状态与 CI 审计说明同步当前事实。

本地保全目录为仓库 `.git/workspace-audit/2026-11-06/`：`branches.bundle`、
`branches-before.json`、`branches-removed.json`、`pr7.json`、`validation.json`、两份 npm audit 原始 JSON、
定向测试源与 `cache-cleanup.json`。它不进入 PR 或发布产物，复制仓库时需单独保留。

恢复一个已清理分支可使用以下命令（替换实际分支名）：

```bash
git fetch .git/workspace-audit/2026-11-06/branches.bundle refs/heads/BRANCH_NAME:refs/heads/BRANCH_NAME
```

近期顺序与可审阅 PR 切片见 [活计划](../plan.md)：先修正 PR 回退、能力账本与依赖；
随后修正配方输入，再完成实例/计划导航、macOS Node/Python 配方闭环和跨平台执行底座。

## PR #7 修正与合并前验证

用户授权修正并合并后，两项 P2 与相关 Python 能力账本已完成修正，并通过独立源码复核：

- 统一目录具有独立读取/错误状态；IPC 或格式失败显示原因，独立重试和“刷新状态”均能恢复能力。
  成功结果继续按会话复用，目录失败不阻塞旧渠道查看和安装。
- 卡片“来源与能力”直接展示全部目录来源，保留清单/安装/卸载/绑定状态及原因和独立项目核对；
  原生 details/summary 具有包含工具名的可访问名称和键盘焦点，平台与覆盖说明显示一次。
- Python macOS 账本登记现有 mise/uv/Homebrew/pyenv 安装卸载与 mise 绑定、声明/受控解析核验；
  保留 PATH 所有权、其他来源绑定、已有虚拟环境、终端/IDE 和 Windows/Linux 边界，不修改执行器权限。
  三平台 fixture 由真实目录导出同步。
- 修正后前端 **43 文件 / 287 项**、Rust macOS **291 项**通过；lint、格式、类型/生产构建、
  Rust clippy/build、版本/文档检查通过，生产依赖审计为 0。新增 4 项前端回归结果与 1 项 Rust 能力矩阵测试。
- 真实 React 组件配合模拟 IPC 及当前目录 fixture 的 Chromium 检查通过：1200/460/390 像素无横向溢出，
  Python 五来源说明完整；390 像素明暗截图、独立重试恢复和 Node summary 键盘展开均通过，无页面异常。
  本地截图与检查结果保存在 `/Users/3yearszhuang/.codex/visualizations/2026/11/06/01a1b0c7-1120-7de3-967c-31fbae7db922/pr7-fix-qa/`。
- 配方快照/约束修正与开发依赖维护继续作为独立后续切片，不包含在本 PR；本轮未新增真实渠道写入、
  打包桌面 IPC 或 updater 验收。

最终 head、三平台 CI 及合并提交绑定到 [PR #7](https://github.com/3yearsZhuang/Z.Env/pull/7) 的记录，
前述 `28d1783` 和旧测试数字只对应首轮初审版本。
