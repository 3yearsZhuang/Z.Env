# P14 / 43 — Python pyenv 受控编译渠道（工作包 08）

日期：2026-10-21（Asia/Shanghai），分支 `main`。
状态：**受控源码编译计划、构建依赖/插件门禁、伪管理器场景与真实可丢弃 pyenv 编译安装/卸载完成；其他版本/平台与真实失败/取消场景待补。**
真实验收使用临时克隆的 pyenv 与可丢弃 `PYENV_ROOT`，未安装到用户 pyenv，也未修改用户环境。

## 动因

Python 五来源清单已能观测 pyenv 版本目录，但计划服务仍把 pyenv 安装标为“仅预览”。
pyenv 与 mise/uv 不同：它通过 python-build 现场编译 CPython，需要 Xcode CLT、OpenSSL/readline
等构建依赖，可能运行插件/补丁，耗时长且失败会留下部分产物。需要先建立根目录、插件、构建工具、
精确版本和结果核验门禁，再开放执行。

## 落地

- 新增 [pyenv Python 写适配](../src-tauri/src/operation_plan/python_pyenv.rs)。
- 根目录必须是绝对、存在、非符号链接的目录；`pyenv root` 必须与扫描根目录一致。
- 能力证据：`pyenv --version`、`root`、`commands` 包含 `install`；`cc/make/patch/git/curl/tar`
  六项构建工具全部可定位，缺一即拒绝执行。
- 只接受精确稳定 `3.x.y`；版本目录 `root/versions/<version>` 已存在时拒绝安装。
- 安装 argv 固定为 `install <version>`，卸载为 `uninstall -f <version>`；不接收前端程序或参数。
- 受控环境：清空继承环境，固定 `HOME`、`PATH`（`PYENV_ROOT/bin`、`PYENV_ROOT/shims` 与系统目录）、
  `PYENV_ROOT`、`PYENV_YES`；不 source shell、不自动安装系统依赖。
- 卸载要求目标当前健康、属于 pyenv、locator 与版本目录一致、目录非链接、可执行文件位于目录内。
- 执行超时 1 小时、输出预算 1 MiB；取消/失败回收进程树。结果以版本目录与重扫实例为准，
  退出码不作为编译或删除成功证明。
- 计划接线：Python `WriteSpec` 增加 Pyenv 分支；`python.rs` 预览的命令与执行共用固定 argv。
  Python 写租约同步扩到 `brew`/`pyenv`，否则真实 Command 步骤会被旧的 mise/uv 白名单拒绝。
- 原生验收入口 [python_pyenv_operations.rs](../src-tauri/examples/python_pyenv_operations.rs)：
  在 `ZENV_ACCEPTANCE_ROOT` 内扫描、预览、安装、重扫、卸载，并输出计划与任务记录。

## 验证记录

| 检查 | 结果与边界 |
| --- | --- |
| Rust 单测 | pyenv 5 项：安装编译并核验、卸载并重扫、缺构建工具/插件/已有版本拒绝、0 退出无目录不算成功、取消回收且不落版本目录 |
| 伪管理器 | 固定 pyenv 脚本覆盖 version/root/commands/install/uninstall；构建工具为本地空脚本 |
| 真实 pyenv 编译 | 临时克隆 pyenv + 可丢弃 `PYENV_ROOT`：python-build 真实编译 CPython 3.12.13 成功；应用计划 4 步安装全部 `completed/installed`，重扫出现健康 Pyenv 实例；4 步卸载 `completed/uninstalled`，版本目录移除。未写用户 pyenv/state/cache，未安装系统依赖 |
| 真实构建环境 | 使用系统 `cc/make/patch/git/curl/tar` 与 Homebrew `openssl@3`/readline/ncurses；python-build 自动发现依赖 |
| 静态/构建 | 随本轮全量门禁执行 fmt、clippy `-D warnings`、测试与构建 |

## 边界与后续

- 真实 3.12.13 编译/卸载已闭环；其他 Python 版本、Linux/Windows pyenv、真实构建失败残留与真实取消
  场景仍待补。伪管理器的失败/取消/零退出场景与真实编译不能互相替代。
- 不自动安装系统依赖，不修改 shell 或项目版本请求；rehash 与已有 venv 影响需另行核对。
- 下一步：真实构建失败/取消与更多版本矩阵；Homebrew 可丢弃写入已完成；随后项目配方与 P15/P16。
