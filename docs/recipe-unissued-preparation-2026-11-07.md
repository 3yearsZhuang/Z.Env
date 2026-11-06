# 第 51 项：未签发准备与冻结项目证据

日期：2026-11-07。实施基线：[父作用域 PR #13](https://github.com/3yearsZhuang/Z.Env/pull/13)，`c621483`。
状态：准备/签发接口与只读 Node 请求匹配已接入；Recipe v2 仍在纯契约校验后返回 `Disabled`，配方执行保持关闭。

## 准备与签发

[preparation.rs](../src-tauri/src/operation_plan/preparation.rs) 的 `PlanDraft` / `PreparedTool` 保存后端阶段、能力和冻结证据，
不包含批准 plan ID、创建/到期时间、执行时钟、消费位，也不插入缓存或创建 journal。结构不能作为执行请求提交。
Node/Python 公共 Tool 预览先准备，再签发；原 wire、渠道能力、120 秒批准、64 个缓存计划、v1 记录和一次消费保留。

只读配方改用 `prepare_install_summary`，返回快照身份与能力，不再建立临时签发会话。
记录门禁在准备期间由健康转为失败时，整个报告拒绝；开始时已经失败仍提供禁止写入的未知摘要。

最终签发前重新比较原上下文、配置完整指纹与文件身份、管理器/运行时文件、绑定资源前像和项目证据。
实际环境入口再次读取来源配置与全局配置路径；显式 Tool API 保留传入的扫描选项。
[query 的短身份临界区](../src-tauri/src/inventory/query.rs) 核对无活动扫描、目标工具 committed 的 generation/request ID 后，
按 query → plan 锁序在同一临界区内分配批准身份与时钟并插入缓存。临界区不读文件或运行管理器。
跨工具已完成扫描不会仅因全局计数增长使原目标失效；任何仍活动的扫描、目标更新或变更租约都会阻止旧准备签发。
外部文件变更仍由执行开始和 Command 写前的原有重验兜底，签发不替代执行验证。

## 项目目标与 Node 请求

[project_target.rs](../src-tauri/src/operation_plan/project_target.rs) 只对实际 Project Bind 保存冻结目标。
捕获时当前配置选择须等于已经准备出的 BindingSpec 目标；保存显式输入、规范目录稳定身份、配置目标、可见/隐藏选择条件、
声明/legacy/项目本地配置变体及祖先标准配置的存在、规范路径、完整 SHA 与文件身份。祖先仅冻结 mise.toml、.mise.toml 和 .tool-versions，不解析完整继承规则。签发和执行写前重验只比较，不重新选目标或接受新基线。
Python Bind 同时冻结实际执行文件与规范执行文件；global/registration 使用既有作用域。

证据预算沿用项目检查边界：单文件 128 KiB、总读取 1 MiB、96 个证据文件、目录枚举 256 项、祖先至多 16 层。
预算内 README、源码等无关文件增改删不改变冻结证据；超预算拒绝，不能用截断的目录或文件授予写能力。
已有 legacy 文件仅作为证据读取，本切片不新增迁移或写入。

[match_node_request](../src-tauri/src/project_environment/mod.rs) 按已读取声明匹配精确请求，不依赖已有候选、执行探针或再读文件。
只接受三段数字与现有 u32 分量域；冲突、未知、不可读或超数值域不获得满足结论，真正没有声明才按未约束处理。
旧候选匹配语义保持。只读 Recipe 在复用或安装准备前匹配 Node 请求，门禁后重验 Node/Python 项目证据和逐工具快照。
现有项目参数仍保留 Python 清单身份要求，本切片未扩展 IPC 请求契约。

## 验证与后续

回归覆盖未签发准备不淘汰满缓存、输入/配置/文件变化、活动/跨工具/取消查询、原子签发锁、上下文复读、签发时钟、
一次消费、记录故障降权、并发签发、项目目标/alias/缺失/变体/声明变化，以及空候选的 Node 请求判断。
本地完整门禁、独立复审和远端三平台结果以本 PR 验证记录为准；假管理器与临时文件测试不作为真实配方执行验收。

下一切片仍需不可变父批准范围、阶段提交并核验的 scan receipt、唯一实际安装实例承接及 Recipe 专用 NoChange。
不能给旧 StoredPlan 替换 generation/config/时钟来接受最新状态，也不能在父任务里签一批 Tool 子计划继续写入。
混合 v2 journal、父记录、新 UI 和实际编排继续独立推进。Node 既有受控验证不等于普通项目 mise/终端解析已生效。
本轮未重建 QA app、实际安装运行时、写用户项目或发布；真实 WKWebView 仍受 ScreenCaptureKit `-3811` 影响。
