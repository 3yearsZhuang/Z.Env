# P14 / 43 — Python 计划预览与实例重新验证（工作包 03）
> 2026-10-20 后续：mise 写渠道见 [工作包 04](./p14-python-mise.md)，uv 见 [工作包 05](./p14-python-uv.md)，Homebrew 见 [工作包 07](./p14-python-brew.md)，pyenv 见 [工作包 08](./p14-python-pyenv.md)。

日期：2026-10-19（Asia/Shanghai），分支 `codex/inventory-contracts`。
状态：**macOS Python 受控重新验证、四渠道安装计划预览与工具范围隔离完成；P14 继续进行**。
本轮没有开放 Python 安装、卸载、绑定、项目虚拟环境或配方执行；没有提交、推送或发布。
前置只读来源见 [工作包 02](./p14-python-sources.md)。
后续 [工作包 04](./p14-python-mise.md) 已开放限定范围的原生 mise 安装/卸载；下文保留本工作包当时的范围与验证。

## 动因

Python 来源清单已经能分别核对管理器目录和真实解释器，但原计划服务只引用 Node 快照与上下文。
直接复用 Node 的计划入口会产生错误的来源、缓存和任务标签，也可能把 Python 包交给 Node 写渠道。
本工作包先让 Python 计划绑定自己的证据，复用已有签发和执行机制，并把可验证的能力与安装意图预览分开。

## 落地

### 计划签发与执行范围

[Python 计划适配](../src-tauri/src/operation_plan/python.rs) 使用现有 `OperationPlanService`。
`operation_plan` 新增可选的顶层 `toolId`，省略时继续走 Node；请求内仍只接受原有意图，
不接受客户端程序、参数或步骤。执行仍只提交 `planId`，由后端已存储计划决定工具和驱动。

[查询服务](../src-tauri/src/inventory/query.rs) 提供内部按工具取得最近提交的接口。
Node 默认查询和写入租约保持原范围；Python 计划引用 Python 快照，不替换 Node 快照。
工具间继续共享活动读取槽、计划容量、全局任务互斥、120 秒有效期、单次消费、取消与 journal。
计划签发的容量和身份生成抽成共用实现，没有新增执行队列或 shell 执行入口。

Python 的计划与最小任务记录增加 `toolId: "python"`。
旧 Node 计划和 journal 不带此字段时按 Node 读取，写回 Node 记录时仍省略；
Python 恢复后的任务标签保持 Python，未完成步骤标中断，不自动重放。
兼容方向是新版本读取旧记录，不承诺旧二进制能读取包含 Python 新字段的 journal。
记录仍采用白名单，只保留工具、动作、身份、时间和步骤状态，不保存路径、版本、配置内容、命令参数或输出。

### 实例重新验证

当前可执行动作只有 Python `verify`：

1. 执行前核对所引用 Python 快照、来源上下文、全局配置指纹、原始及真实可执行文件指纹。
2. 重新扫描该来源，运行工作包 02 的固定 `-I -S -B` 隔离探针，再将结果提交到 Python 缓存。
3. 核对同一实例的来源原生包、版本、架构、文件路径和归属，确认后才报告验证通过。

来源根目录（包括 `PYENV_ROOT` 和 uv 安装目录）、文件、配置、快照或有效期变化会拦截旧计划。
未验证、过期、异常或 PATH 记录没有执行能力；不会通过计划运行任意 PATH 文件。
当前健康实例可以在来源含其他未支持条目时单独重新验证；结果只证明该实例，
不会把部分覆盖升级为完整覆盖或安装/卸载权限。安装预览仍要求该来源完整读取。

取消继承既有任务机制：前置探针终止并回收进程组，刷新阶段将取消转发到对应 Python 查询。
等待真实工作退出后结束任务，不把取消当验证通过。只有临时/正式 journal 与 Python 内存清单状态发生相应记录更新，
不改动解释器、Python 请求、项目配置、虚拟环境或 shell。

### 安装意图预览

| 来源 | 接受的意图与步骤示意 | 当前边界 |
| --- | --- | --- |
| mise | `python` + 精确稳定 CPython 3.8+；`install core:python@X.Y.Z` | 显式 core；下载/构建和产物能力尚未核验 |
| Homebrew | core `python` / `python3` / `python@3.N`；`install --formula homebrew/core/<formula>` | 不接受精确补丁版承诺；依赖、链接与多 keg 影响门禁待补 |
| pyenv | `python` + 精确稳定版本；`install X.Y.Z` | python-build、插件、构建和 rehash 的影响未验收 |
| uv | `python` + 精确稳定版本；`python install --no-bin cpython@X.Y.Z` | `--no-bin` 是避免创建 bin 入口的意图；本机版本兼容和产物待核验 |

所有安装计划 `canExecute=false`，直接提交其计划 ID 也会被后端拒绝；不调用管理器探测、安装器、插件或兼容写入口。
预览先检查本工具来源完整覆盖并定位管理器文件，不把管理器可定位当成其参数、版本或运行能力已验证。
步骤展示绝对程序路径、argv、工作目录、环境键，影响说明保留来源根目录及尚未验证的副作用。
范围、别名、预发布、特殊构建、其他工具包和第三方 tap 不被默认为可安装目标。
Python 卸载/绑定返回未接入原因，解除绑定也不进入 Node 绑定记录。

步骤语义核对依据：[mise Python](https://mise.jdx.dev/lang/python.html)、
[Homebrew install](https://docs.brew.sh/Manpage#install-options-formulacask-)、
[pyenv install](https://github.com/pyenv/pyenv/blob/master/COMMANDS.md#pyenv-install)、
[uv Python install](https://docs.astral.sh/uv/reference/cli/#uv-python-install)。
这些文档依据不替代本机渠道能力探测与实机安装验收。

### 界面与契约

[Python 详情](../src/components/PythonInventoryPanel.tsx) 显式打开安装预览，健康实例展开后可打开重新验证计划。
[共用计划弹窗](../src/components/OperationPlanDialog.tsx) 根据工具显示动作、渠道和版本承诺；
Python 只开放重新验证的执行按钮，安装预览没有写执行按钮。
前端拒绝其他工具回复、错快照/实例/来源和误开放写能力的 Python 计划，执行结果也核对工具、动作和计划 ID。
刷新后的快照会使旧预览失效，失败/取消不会报告验证成功。

关闭弹窗后焦点返回触发按钮；弹窗打开时不禁用该触发按钮，交互隔离由模态弹窗负责，
避免按钮失焦后异步加载的弹窗拿不到正确的返回位置。读取刷新与单来源重试在弹窗期间禁用。
明暗和窄窗共用既有可滚动内容、固定头尾结构；Node 原有入口、绑定与补偿保留。

共享契约：[Python 操作样本](../fixtures/operations/python-v1.json)、
[三平台目录](../fixtures/tool-catalog/contract-v1.json)。
目录将 macOS 四渠道安装计划标已接入，并明确“预览可用、安装执行未开放”；PATH 和其他平台不继承这些能力。

## 验证记录

| 检查 | 结果与边界 |
| --- | --- |
| 前端 `npm test -- --maxWorkers=2` | 27 文件、174 项通过，较工作包 02 新增 8 项，无未处理异常 |
| Rust `cargo test --manifest-path src-tauri/Cargo.toml --locked` | 218 项通过，较工作包 02 新增 9 项 |
| lint、Prettier、前端生产构建、版本一致性 | 通过；版本仍为 0.7.1；计划弹窗继续按需加载 |
| Rust fmt、all-targets clippy `-D warnings` | 通过 |
| Chromium 模拟 Tauri IPC | 清单回归 15 场景 + 新计划 16 场景；四渠道参数、写能力拒绝、重新验证、取消、跨工具回复、键盘焦点与明暗 1200/460/390，无页面异常和横向溢出 |
| 真实 macOS 查询及计划执行器 | CPython 3.13.15 / mise，3 步重新验证完成；同一次临时会话中，Python 操作之前签发的 Node 计划仍可执行并通过验证 |
| 配置保全 | 22 个正式配置/清单/journal/元数据保护路径前后哈希及链接一致；预览没有创建临时 journal，执行仅写临时 journal |

Rust 回归包括：四渠道计划禁止执行、精确版本与包白名单、范围/命令注入、Node/Python 快照隔离、
根目录/配置/文件/快照/过期拦截、文件指纹未变但运行输出变化、探针取消回收、部分来源的单实例验证、
最小 journal、旧 Node 字段兼容、Python 中断恢复与共享 JSON。
测试管理器带“不得执行”标记，所有预览和重新验证均未触发它们。

初次扩展的前端回归发现旧安装测试返回了 nvm 计划，而请求是 mise；修正为匹配的来源，保留新增来源校验。
一次全量运行有 Radix 卸载焦点计时器跨 jsdom 生命周期的未处理异常；在测试清理阶段等待该回调退出后重新全量通过，
没有忽略测试运行器错误。初次浏览器检查的焦点返回失败也已修正并复测。

本地完整证据目录：
`/Users/3yearszhuang/.codex/visualizations/2026/10/19/01a14fe4-ee6b-7ec1-933e-d7d2e53c89f8/p14-python-plans/`。
原生入口：[python_operation_plan.rs](../src-tauri/examples/python_operation_plan.rs)。
浏览器测试使用模拟 IPC；原生 helper 验证查询与计划执行器，均不作为打包桌面 IPC 端到端验收。
本轮没有实际安装、卸载或绑定 Python，也没有修改依赖清单和锁文件。

## 后续

P14 继续进行：为 Python 渠道补本机能力、产物、归属与副作用门禁后，逐项开放实际安装/卸载；
Python 绑定、虚拟环境、项目解析与配方仍需独立模型和预览/失败步骤重试。
Java/Go/Rust/其他 CLI 继续按工具/provider 逐项接入。
P12 的真实 Homebrew 写入/托管、打包桌面 IPC，以及 P15/P16 门槛保留。
