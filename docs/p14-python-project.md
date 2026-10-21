# P14 / 43 — Python 项目环境与绑定（工作包 06）

日期：2026-10-21（Asia/Shanghai），分支 `main`。
状态：**声明解析、候选匹配、mise 项目/全局绑定写入、私有收据、解绑、失败回滚与 `mise which` 实际解析核验完成。**
声明与候选是只读能力；绑定/解绑只在用户确认的计划内写目标 mise 配置，不执行解释器、不安装依赖。

## 动因

Python 已有 mise/uv 精确安装与卸载，但项目页仍只能看到 Node 的声明与候选。
Python 项目通常同时用 `mise.toml`、`.python-version`、`pyproject.toml` 的
`requires-python` 表达要求；这些语法不是 npm 范围，不能直接套用 Node 的约束模型。
先补只读解析与候选匹配，给出可审阅的声明、要求状态和候选，再开放写入。

## 落地

### 声明解析

新增 [Python 项目核对](../src-tauri/src/project_environment/python.rs)，读取：

| 来源 | 解析 | 边界 |
| --- | --- | --- |
| `mise.toml` / `.mise.toml` | `[tools] python` 或 `[tools] "core:python"` 字符串 | 表、数组、别名等复杂形式标未知，不猜测 |
| `.python-version` | 单个非空版本 | 多版本标未知；不执行 pyenv |
| `pyproject.toml` | `[project] requires-python` | 非字符串标未知；poetry/uv 动态配置不推断 |
| `.tool-versions` | `python <version>` | 多版本/缺版本标未知 |

复用项目环境的有界文件读取（单文件 128 KiB、总量 1 MiB、前后指纹一致），
文件证据只保留路径、规范路径、存在性与 SHA-256，不保存内容。

### 版本约束（PEP 440 子集）

- 支持精确 `3.12.13`、次版本前缀 `3.12`、主版本 `3`、`>=`/`>`/`<=`/`<`、
  `==3.12.*`、`!=3.12.13`、`!=3.12.*`、`~=3.12`/`~=3.12.1` 与逗号 AND。
- `^`、`||`、空格分隔、`latest`、`3.12.x` 等未支持语法标未知，不做近似解释。
- 多个声明交叉合并；区间为空、精确版本被排除或上下界冲突时报告 `conflict`。

### 候选与选择

- 候选来自 Python 清单：版本匹配分别给出 `satisfied` / `mismatch` / `unknown`；
  仅健康、未过期、非 PATH 记录的实例可标记为可用，PATH 只作观测。
- 当前选择优先报告项目 `mise` 的 `[tools] python`；同时存在 `.python-version`
  或多个配置时标 `unknown`，不推断优先级。
- `inventoryComplete` 汇总来源是否完整；不完整时候选不能作为完整结论。

### 接口与契约

- Tauri 命令 `python_project_environment` 复用 Python 清单租约与只读快照，
  不触发重扫或写入；请求包含 `requestId`、`snapshotRequestId` 与绝对项目路径。
- 前端 [Python 项目 API](../src/api/pythonProject.ts) 校验版本、状态、候选来源、
  文件证据与预算；共享契约样本为
  [python-project-v1.json](../fixtures/project-environment/python-project-v1.json)。
- 原生只读入口 [python_project.rs](../src-tauri/examples/python_project.rs)。

### mise 绑定写入

新增 [Python 绑定适配](../src-tauri/src/operation_plan/python_binding.rs)：

- 只接受 `mise` 健康实例、`direct` 策略与项目/全局作用域；版本必须是精确稳定 `3.x.y`。
- 项目作用域复用项目目标解析（目录或配置文件），全局作用域写入当前全局 mise 配置。
- 使用 `toml_edit` 只改 `[tools] python`（已有 `core:python` 时沿用该键），保留其他工具、
  注释与结构；同时存在 `python` 与 `core:python` 时拒绝猜测。
- 预览展示 `tools.python` 前后值与完整配置差异；执行前重读配置并拒绝外部修改；
  写入后重扫 Python 来源、核对配置值与实例仍健康。
- 写入沿用配置服务的同目录原子替换与 `.zenv-backup`；不改 shell、不卸载实例。
- 前端 Python 详情新增“绑定此实例到项目”，计划弹窗对 Python 隐藏不支持的注册/托管/PATH
  策略，作用域只提供全局/项目。

### 收据、解绑与回滚

- 绑定成功后在 `~/.zenv/python-bindings-v1.json` 写入私有收据：安装身份、包、版本、
  配置路径、键、绑定前值与状态（`pending`/`bound`）；最多 32 条，原子写并带备份。
- `python_bindings` 命令只读返回本应用的 `bound` 收据；Python 详情列出“已有 Python 绑定”，
  可打开解除计划。Python 弹窗的解除入口使用显式收据 ID，不读取 Node 绑定列表。
- 解绑预览重读配置，要求当前值与收据一致才允许；执行只恢复收据记录的原值/移除键盘，
  成功后删除收据；外部修改时返回 `BindingConflict` 并保留收据。
- 失败回滚：绑定写入后核验失败时恢复预览前内容并删除收据；解绑失败时按收据重新写回绑定值，
  恢复 `bound` 收据；外部冲突不覆盖。执行器已接入通用补偿步骤，模块单测覆盖两个方向。
- 实际解析核验：绑定写入后以受控 mise 命令（不加载 shell、禁用 hooks、保留配置读取）在项目目录执行
  `mise which python`，将输出规范路径与目标实例 `canonicalExecutablePath` 比对；不一致或命令失败
  即 `Verification` 失败并触发回滚。终端/IDE 是否继承仍需另行确认。

## 验证记录

| 检查 | 结果与边界 |
| --- | --- |
| Rust 单测 | Python 项目模块 7 项：PEP 440 子集、匹配/冲突、声明、候选、路径、共享契约 |
| 前端 | Python 项目 API 4 项：契约、IPC 调用、未来版本/未知状态/跨来源/超预算拒绝 |
| 真实 macOS 只读核对 | 临时项目 `mise.toml`（`3.13`）+ `pyproject.toml`（`>=3.10,<3.14`）：状态 `supported`，mise 3.13.15 与 uv 3.13.14 `satisfied`，uv 3.12.13/3.14.6 `mismatch`，PATH 实例不可用；`inventoryComplete=false`（PATH/其他来源未完整读取）如实上报 |
| 真实 mise 绑定 | 临时项目 `mise.toml` 含 `node = "22"` 与 `[env] A = "1"`；真实 mise CPython 3.13.15 预览 → 执行 → 重扫核验：`completed/bound`，写入 `python = "3.13.15"`，node 与环境段原样保留，差异仅 `tools.python` |
| 真实 mise 解绑 | 同一项目继续解绑：`completed/unbound`，配置恢复为绑定前内容，收据移除；私有账本与项目均在可丢弃目录 |
| 真实 `mise which` | 绑定写入后真实 mise 2026.10.8 解析到目标 CPython 3.13.15 规范路径；验收使用临时 `MISE_DATA_DIR`（`installs` 指向真实目录、state/cache 可写），避免沙箱拒绝写真实 state |
| 静态/构建 | Rust fmt、clippy `-D warnings`、构建；前端 lint、Prettier、构建、版本与文档链接检查 |

真实核对只读：未写项目配置、未执行解释器、未安装依赖；未把候选可匹配写成项目实际生效。

## 边界与后续

- 绑定/解绑已写入项目/全局 `mise.toml` 并带私有收据、失败回滚与项目作用域的 `mise which`
  实际解析核验；全局作用域与终端/IDE 继承仍需平台实测。
- 实际解析禁用了 mise hooks，避免执行项目自定义脚本；依赖 hooks 的环境可能与终端结果不同。
- 绑定仅限 mise 实例；uv/brew/PATH 实例不能直接作为 mise 配置目标，需先由 mise 管理或链接。
- 未处理 poetry/uv workspace 动态依赖、多版本 `.python-version`、已有 `.venv` 的基础解释器引用。
- 下一步：已有 `.venv` 解释器判断、全局作用域/终端继承验收与项目配方。
