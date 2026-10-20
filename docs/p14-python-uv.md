# P14 / 43 — Python uv 安装、卸载与结果核验（工作包 05）

日期：2026-10-20（Asia/Shanghai），分支 `codex/inventory-contracts`。
状态：**macOS uv 托管 CPython 精确安装/卸载、重验与真实隔离验收完成；P14 继续进行**。
本轮之后 P14 仍有 Homebrew/pyenv 写渠道、Python 绑定、虚拟环境和项目配方；
uv 自身安装/升级、Windows/Linux 布局与 PyPy 不在本工作包范围。

## 动因

工作包 03 已为 uv 生成安装预览（`python install --no-bin cpython@X.Y.Z`），但执行被关闭：
来源目录里的次版本别名会被判为未知变体，导致 uv 来源永远不完整；卸载归属和
`--install-dir` 尚未绑定；而且 `uv python uninstall` 在目标不存在时仍退出 0，
不能用退出码宣告删除成功。本工作包把这几项一起收口，并保持与 mise 渠道相同的
签发、重验、写锁、取消、journal 和终态重扫语义。

## 落地

### 来源别名与目录身份

- [Python 来源扫描](../src-tauri/src/providers/python.rs) 新增 `known_uv_alias`：
  识别 `cpython-<major>.<minor>-<os>-<arch>-<libc>` 符号链接，要求规范目标位于 uv 根目录内、
  目标名为精确补丁版目录、次版本前缀一致。命中后不再产生 `pythonVariantUnsupported`，
  别名仍作为观测候选保留（无版本、未验证），精确目录才是写目标。
- 新增 `uv_arch` / `uv_dir_name`，扫描与写适配共用同一目录命名；macOS aarch64 / x86_64 由
  运行时架构决定，其他平台返回未支持。
- 别名指向根目录之外、悬空、pypy 等未知布局仍标未支持并保持只读。

### 受控执行

[uv 写适配](../src-tauri/src/operation_plan/python_uv.rs)：

- 能力证据：`uv --version`、`uv --help` 含 `--no-config`、`uv python install --help` 含
  `--no-bin/--install-dir`、`uv python uninstall --help` 含 `--install-dir`。
- 固定 argv：
  - 安装：`--no-config --no-progress python install --no-bin --install-dir <root> cpython@X.Y.Z`
  - 卸载：`--no-config --no-progress python uninstall --install-dir <root> cpython@X.Y.Z`
- 受控环境：清空继承环境，固定 `HOME`、最小 `PATH`、`cwd=home`；设置
  `UV_PYTHON_INSTALL_DIR`、`HOME/.cache/uv`、`UV_NO_CONFIG`、`UV_NO_PROGRESS`、`UV_COLOR`。
- `Facts` 每次读取 uv 版本、能力、精确安装行与别名一致性；预览、Preflight、Command 前分别重验。
  根目录允许尚不存在（安装会创建），但必须绝对、无 `..`，最近存在的祖先可规范化；
  已存在的根目录不能是外部链接。
- 安装拒绝已存在的精确版本/目录；卸载要求目标当前健康、provider/package/locator 一致、
  可执行文件规范路径在 locator 内、locator 不是符号链接、facts 恰好一行且规范路径一致。
- 结果核验不以退出码为依据：安装核对精确目录、健康实例与隔离探针；卸载核对行、目录和
  快照实例均消失；最后重扫未完整时返回 `ScanIncomplete`，不能报告成功。

### 计划接线与能力

- [Python 计划适配](../src-tauri/src/operation_plan/python.rs) 的 `WriteSpec` 扩为
  `Mise | Uv` 枚举；Install/Uninstall 统一在签发阶段尝试对应渠道准备，成功才开放执行。
  卸载预览不再一律标“未接入”，健康目标可先看四步计划，再由准备结果决定能否执行。
- Python 写租约从“仅 mise”扩为 `mise | uv`；Command 步骤按计划目标 provider 建立租约，
  Refresh 仅在 mise 时失效 mise 探测缓存。
- 安装预览与执行共用同一 argv 构造函数；前端仍只提交 `planId`，不接收任意命令。
- [工具目录](../src-tauri/src/tool_catalog/mod.rs) 把 macOS uv 的安装/卸载账本标为已接入，
  原因写明 `--no-bin`、精确版本、别名重指向与重验；契约夹具随之更新。
- [Python 详情页](../src/components/PythonInventoryPanel.tsx) 文案改为 mise 与 uv；
  执行按钮仍只由后端 `capability.canExecute` 控制。

### 契约与测试

- 新增 [uv 操作样本](../fixtures/operations/python-uv-v1.json)，并为
  [Python 通用样本](../fixtures/operations/python-v1.json) 更新 uv 的 argv 与环境键。
- 新增 `python_uv_tests.rs`：安装/卸载闭环、受控环境、已存在版本、未知布局、范围/注入拒绝、
  0 退出无产物、错误探针版本、外部链接拒绝、命令前重验、共享契约往返。
- 来源测试新增 uv 次版本别名不破坏完整覆盖、越界别名不得信任两项。

## 验证记录

| 检查 | 结果与边界 |
| --- | --- |
| Rust 全量 `cargo test --locked` | **245 项通过**（237 基线 + 2 项来源别名 + 6 项 uv 操作）；fmt、all-targets clippy `-D warnings`、构建通过 |
| 前端 | 28 文件 / 218 项通过；lint、Prettier、生产构建、`npm audit`（0 漏洞）通过 |
| 真实 uv | uv 0.12.1（aarch64-apple-darwin）在 `/tmp/zenv-uv-accept.L3Q7ga` 可丢弃根目录完成：安装 3.12.13 → 重扫出现精确健康实例 → 卸载 → 目录移除；两次记录均为 `completed`，结果分别为 `installed` / `uninstalled` |
| 真实产物 | 安装目录 `cpython-3.12.13-macos-aarch64-none`；别名 `cpython-3.12-macos-aarch64-none` 指向该目录；卸载后两者均移除，仅余 `.gitignore`、`.lock`、`.temp` |
| 受控环境 | 真实命令 argv 使用规范根目录 `/private/tmp/...`；`environmentKeys` 含 HOME/PATH 与 5 个 `UV_*`；未向 bin 安装入口（可丢弃 HOME 下无 `.local/bin`） |
| 别名行为 | 手动隔离验证：同 minor 两个补丁版安装后别名指向新补丁版；卸载别名目标后自动重指向剩余补丁版，不留悬空链接 |
| 保护边界 | 验收使用 `env -i HOME=<可丢弃>`；真实 `~/.config/mise/config.toml`、`~/.local/share/uv/python` 与 `~/.local/bin` 未被写入；测试管理器带“禁止执行”标记的用例继续覆盖预览不执行安装/卸载 |
| 其他 | 浏览器/桌面 IPC 未在本工作包重新验收；打包桌面端到端仍按 P12 门槛保留 |

Rust 回归还覆盖：`PlanError` 经共享执行器后仍保留 `Preconditions/Stale/Expired` 等具体变体；
探针测试不再依赖 setuid `/bin/ps`，在沙箱/加固环境下同样可运行。
实机证据目录为 `/tmp/zenv-uv-accept.L3Q7ga/`（可丢弃，结果已摘录到本记录）。

## 边界与后续

- uv 渠道只覆盖 macOS 的 uv 托管 CPython；uv 自身安装/升级、Windows/Linux 布局、PyPy、
  项目虚拟环境、Python 绑定与配方仍未接入。
- `uv python uninstall` 对不存在目标也退出 0；本实现只以重验、重扫和文件消失作为结果证据。
- uv 可能重指向同 minor 别名；影响说明已写入计划，但不把别名当作额外归属或卸载授权。
- P14 下一步：Python Homebrew、pyenv 写渠道，随后 Python 绑定与项目环境；P12 的
  Homebrew 真实写入/托管与打包桌面 IPC 门槛继续保留。
