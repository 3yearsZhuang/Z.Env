# P14 / 43 — Python 五来源只读清单（工作包 02）
> 2026-10-20 后续：写渠道见 [工作包 03](./p14-python-plans.md)、[04](./p14-python-mise.md) 与 [05](./p14-python-uv.md)。

日期：2026-10-19（Asia/Shanghai），分支 `codex/inventory-contracts`。
状态：**macOS CPython 3 的只读来源、独立缓存与详情入口完成；P14 继续进行**。
本轮没有开放 Python 安装、卸载、绑定或项目环境计划，没有提交、推送或发布。
前置目录与导航见 [工作包 01](./p14-tool-catalog.md)，Node 原有实现和验收边界继续保留。
后续 [工作包 03](./p14-python-plans.md) 已接入计划预览与受控重新验证；下文保留本工作包当时的范围和验证。

## 动因

目录登记了 Python，不代表机器上存在可信的 Python 安装。
Python 还需要区分管理器安装、项目虚拟环境、实现/构建变体和任意 PATH 文件。
这轮先补齐可核对的读取证据，避免把 Node 清单或旧兼容列表的结果当作 Python 覆盖。

## 落地

### 来源与身份

[Python 来源模块](../src-tauri/src/providers/python.rs) 复用既有预算、元数据读取和有界进程运行器，
在原 `ProviderId` 中增加 `pyenv`、`uv`；Node 的四来源范围保持不变。
规范工具身份为 `python`，各来源的原生包、目录与文件各自保留；同版本不同目录不合并。
同一真实文件可关联多个来源记录，但不会因此转移归属。

| 来源 | 读取范围 | 本轮可证明的边界 |
| --- | --- | --- |
| mise | `MISE_DATA_DIR/installs/python` | 支持的精确 CPython 3 目录及其受控探针；仅观测，不由目录取得所有权 |
| Homebrew | 捕获的 Cellar 中 `python`、`python3`、`python@3.N` | 核对 core 收据后才运行探针；原生 formula 和 keg 原样保留。收据仍不等于拥有卸载权限 |
| pyenv | `PYENV_ROOT/versions`，默认 `~/.pyenv/versions` | 读取版本目录，不 source 初始化脚本，不调用管理器或插件 |
| uv | `UV_PYTHON_INSTALL_DIR`，默认 `$XDG_DATA_HOME/uv/python` 或 `~/.local/share/uv/python` | 核对受支持 CPython 完整目录布局，不调用 `uv python find/install`，不触发下载 |
| PATH | `python`、`python3`、`python3.N` 文件候选 | 保留文件与真实路径，**不执行任意 PATH 程序或 shim**，版本保持未知 |

Python 3.8 起的稳定、常规 CPython 3 构建为本轮样板；版本名别名、预发布、debug、free-threaded
及其他实现不会自动升级为已验证安装。检测到 `pyvenv.cfg` 的候选跳过，项目虚拟环境不充当管理器原安装。
不支持的变体明确保留未验证或来源覆盖不完整；不能据此断言系统没有 Python。
Windows/Linux 返回明确不支持，不沿用 macOS 的目录或命令能力。

来源布局依据：[pyenv 版本目录](https://github.com/pyenv/pyenv#locating-pyenv-provided-python-installations)、
[uv 存储](https://docs.astral.sh/uv/reference/storage/#python-versions)、
[uv Python 版本与实现](https://docs.astral.sh/uv/concepts/python-versions/)、
[Homebrew Python formula](https://formulae.brew.sh/formula/python@3.13)。
这些布局和版本识别规则不作为安装渠道版本承诺。

### 隔离探针与预算

探针使用明确的真实文件路径、清空的继承环境、固定 HOME 和系统 PATH，运行固定的
`-I -S -B -c` 表达式；不加载 shell、用户启动模块、site、`.pth` 或项目任务，不写 Python 字节码。
返回并校验实现、稳定发布标记、构建类型、版本、架构及 `sys.executable`。
目录声明与实际版本不同会报告异常；真实文件身份、路径或链接在探测中变化，则保持未验证。
探针失败不写原始 stdout/stderr 到业务历史。

参数语义见 [Python 命令行文档](https://docs.python.org/3/using/cmdline.html)：
`-I` 隔离环境变量和不安全导入路径，`-S` 禁用自动 site 初始化，`-B` 禁止生成 `.pyc`。
它们与进程预算不是安全沙箱；已有管理器目录也不等于任意本地程序可信。

沿用每来源 8 秒、每探针 3 秒、最多 128 个候选、32 次探针、2048 个目录项、
128 KiB 元数据和 32 KiB 探针输出的默认限制。四个可探测来源并行，PATH 只读文件；
取消终止并回收进程组，超时/取消本身不把安装判为损坏。
文件系统调用之间协作检查预算，不声称能中断任意内核文件系统阻塞。

### 查询范围与界面

[查询服务](../src-tauri/src/inventory/query.rs) 现在按工具保存来源缓存和最近提交，
仍共用全局活动租约、请求身份历史和取消通道。`inventory_snapshot` 新增可选 `toolId`；
省略时仍是 Node，旧调用不需要改变。Python 请求仅接受其五来源范围，返回的工具覆盖和观测也必须匹配。
Node 写计划和 P13 项目核对仍只读取 Node 快照；Python 刷新、失败或取消不能替换它。
当前只有两个已登记工具缓存，不接受任意工具名来扩张缓存。

[前端读取 hook](../src/lib/useInventory.ts) 保留同一队列和退出等待规则。
工具切换取消旧读取，等待其真正结束后才分派下一读取；跨工具响应、错请求身份及接受取消后的迟到成功被拒绝。
失败保留旧证据并标过期，不能恢复健康状态。

[Python 详情](../src/components/PythonInventoryPanel.tsx) 在工具目录显式点击后按需加载。
展示各来源状态、时间、预算截断、健康、归属、原生包、版本、架构、原路径和真实路径，
支持单来源重试和取消；Node/Python 详情切换时保留独立会话。
目录直接复用对应详情状态，刷新目录元数据不额外扫描机器。

目录的正向与负向判断分别处理：本次健康探针可以证明存在该实例，即使同来源其他条目失败；
同时明确“部分来源未完整读取”，不由此推导其他安装或写权限。
负向结果仍要求本工具全部已接入来源完整读取且没有记录；PATH、过期和失败不能证明未安装。
Python 安装、卸载、绑定和项目核对仍标“尚未接入”，不展示假执行按钮或自动退回兼容写入。

共享样本：[Python 清单](../fixtures/python-inventory/contract-v1.json)、
[三平台目录](../fixtures/tool-catalog/contract-v1.json)。没有增加依赖或修改锁文件。

## 验证记录

| 检查 | 结果与范围 |
| --- | --- |
| `npm test -- --maxWorkers=2` | 27 文件 / 166 项通过，较工作包 01 新增 9 项 |
| Rust `cargo test --manifest-path src-tauri/Cargo.toml --locked` | 209 项通过，较工作包 01 新增 11 项 |
| lint、Prettier、版本一致性及前端生产构建 | 通过，版本仍为 0.7.1；Python 面板为独立按需加载 chunk |
| Rust fmt、all-targets clippy `-D warnings`、原生构建 | 通过 |
| `npm audit --audit-level=high` | 当前依赖报告 0 个漏洞 |
| Chromium + 模拟 Tauri IPC | 15 场景通过：明暗 1200/460/390、长路径/证据、键盘、范围、重试、部分失败、取消迟到成功、Node/Python 切换；无页面异常 |
| 本机真实只读查询 | 最终五来源返回 20 条记录；Homebrew/PATH 读取完成，mise 部分条目错误、pyenv 不可用、uv 含未支持变体；这些结果保留原因，没有修复 |
| 真实 CPython 3.13.15 + 临时来源布局 | mise/pyenv/uv 三种临时目录链接分别验证同一真实解释器；PATH、虚拟环境、注入的启动模块与工作目录模块没有执行；未生成临时 `.pyc`，原解释器哈希不变 |

Rust 场景包括：五来源独立实例、收据与实现边界、虚拟环境、别名/构建变体、断链、
固定 flags/清空环境、版本错配、文件重定向、预算、取消、共享契约与包映射。
查询回归明确验证全局互斥但缓存分离、拒绝其他工具/来源、取消不提交、Node 快照保持。
原有“合并错误归还租约”测试改用来源包错配触发合并器，因为工具错配现在已在更早的查询范围守卫拒绝；断言和覆盖仍保留。

原生验收前后核对 22 个正式配置/清单/journal/元数据路径，哈希及链接相同。
临时链接是来源布局夹具，不代表通过 pyenv、uv 或 mise 新安装过解释器；
浏览器是模拟 IPC，不代表打包桌面 IPC 已端到端验收。
本轮没有执行包安装、卸载、绑定、环境切换、项目写入或修复。

本地完整证据：
`/Users/3yearszhuang/.codex/visualizations/2026/10/19/01a14fe4-ee6b-7ec1-933e-d7d2e53c89f8/p14-python-sources/`。
含真实本机/隔离原生 JSON、保护路径、浏览器脚本与截图、测试日志、最终检查及保全审计。
review 使用的 `npm run tauri dev` 保持运行；原生代码修改会触发自动重载。

## 后续

P14 保持进行中：下一步是 Python 能力/操作计划及项目配方的预览、初始化和失败步骤重试。
只有补齐包身份、所有权、渠道实际输出与副作用门禁后，才开放 Python 安装/卸载或绑定；
项目虚拟环境和有效解析需要独立模型，不能复用 Node 的要求语义。
Java/Go/Rust/其他 CLI 继续逐项接入；P12 Homebrew 原生写入/托管、打包桌面 IPC、P15/P16 门槛保持。
