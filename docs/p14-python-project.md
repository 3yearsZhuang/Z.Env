# P14 / 43 — Python 项目环境与绑定（工作包 06）

日期：2026-10-21（Asia/Shanghai），分支 `main`。
状态：**第一切片（声明解析、候选匹配与只读报告）完成；mise 绑定写入、解绑与补偿待继续。**
本轮是只读能力：不写项目配置、不执行解释器、不安装依赖。

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

## 验证记录

| 检查 | 结果与边界 |
| --- | --- |
| Rust 单测 | Python 项目模块 7 项：PEP 440 子集、匹配/冲突、声明、候选、路径、共享契约 |
| 前端 | Python 项目 API 4 项：契约、IPC 调用、未来版本/未知状态/跨来源/超预算拒绝 |
| 真实 macOS 只读核对 | 临时项目 `mise.toml`（`3.13`）+ `pyproject.toml`（`>=3.10,<3.14`）：状态 `supported`，mise 3.13.15 与 uv 3.13.14 `satisfied`，uv 3.12.13/3.14.6 `mismatch`，PATH 实例不可用；`inventoryComplete=false`（PATH/其他来源未完整读取）如实上报 |
| 静态/构建 | Rust fmt、clippy `-D warnings`、构建；前端 lint、Prettier、构建、版本与文档链接检查 |

真实核对只读：未写项目配置、未执行解释器、未安装依赖；未把候选可匹配写成项目实际生效。

## 边界与后续

- 尚未写入 `mise.toml`/全局配置，未实现解绑、补偿或绑定收据；项目实际解析仍未知。
- 未处理 poetry/uv workspace 动态依赖、多版本 `.python-version`、已有 `.venv` 的基础解释器引用。
- 下一步（WP06-B）：mise 项目/全局绑定、配置差异预览、执行后实际解析核验、解绑与补偿；
  随后再评估虚拟环境与配方。
