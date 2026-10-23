# P14 / 43 — Python Homebrew 写渠道（工作包 07）

日期：2026-10-21（Asia/Shanghai），分支 `main`。
状态：**core formula 安装/卸载、依赖/归属门禁、伪管理器场景与可丢弃前缀真实 Homebrew 安装/卸载完成；系统 Homebrew 与其他 formula/平台待补。**
真实验收使用 `/private/var/tmp/hb` 可丢弃 Homebrew 前缀，未修改系统 `/opt/homebrew`。

## 动因

Python 只读清单已能看到 Homebrew core Python 实例，但计划服务仍把 Brew 安装标为“仅预览”。
Homebrew 的版本承诺与 mise/uv 不同：formula 随渠道更新，不能承诺精确补丁版；安装/卸载还会
改变依赖、`python3` 链接、site-packages 与其他 keg。需要先建立 formula 白名单、依赖/归属门禁
和结果核验，再开放执行。

## 落地

- 新增 [Homebrew Python 写适配](../src-tauri/src/operation_plan/python_brew.rs)。
- formula 白名单：仅 `python` 与 `python@3.N`（N=8..99）；拒绝 `python3`、精确补丁版、第三方 tap。
  安装请求不得携带精确版本；预览承诺明确“版本由执行时 formula 决定”。
- 能力证据：`brew --prefix`、`--cellar`、`info --json=v2 --formula`、`list --versions`、
  `uses --installed`；核验 formula 身份、keg 与反向依赖。
- 安装门禁：该 formula 已有任何 keg 时拒绝，不覆盖、不静默升级；argv 固定为
  `install --formula homebrew/core/<formula>`。
- 卸载门禁：目标需当前健康、属于同一 formula、locator 与唯一 keg 一致、无反向依赖；
  argv 固定为 `uninstall --formula <formula>`；多 keg 或依赖存在时保持只读。
- 执行使用现有受控 brew 环境（`HOMEBREW_NO_*`）、超时/取消与进程树回收；结果以 keg 与重扫为准，
  退出码不作为安装或删除证明。安装后要求唯一 keg 与健康 Brew 实例；卸载后要求无 keg、无残留实例。
- 计划接线：Python `WriteSpec` 增加 Brew 分支，预览展示 formula、argv、影响与回滚边界；
  Python 弹窗文案同步说明 Homebrew 计划按能力核验。
- 原生验收入口 [python_brew_operations.rs](../src-tauri/examples/python_brew_operations.rs)：
  通过 `ZENV_BREW_ACCEPTANCE_PREFIX` 指定可丢弃 Homebrew 前缀，在验收 HOME/journal 内扫描、预览、安装、重扫、卸载。
- Python 写租约已扩到 `brew`/`pyenv`，真实 Command 步骤不再被 mise/uv 白名单拒绝。

## 验证记录

| 检查 | 结果与边界 |
| --- | --- |
| Rust 单测 | Homebrew 6 项：formula 白名单、安装写 keg 并核验、已有 keg/精确版本/不完整来源拒绝、卸载单 keg/无依赖/归属、0 退出无产物与外部变化、取消回收且不落 keg |
| 伪管理器 | 固定 `brew` 脚本覆盖 prefix/cellar/info/list/uses/install/uninstall；不访问真实 Homebrew |
| 真实 Homebrew 写入 | 可丢弃 `/private/var/tmp/hb` Homebrew 前缀（Tier 3）安装 `python@3.13` 3.13.17 及 7 个依赖；应用计划 4 步安装 `completed/installed`，重扫健康 Brew 实例；4 步卸载 `completed/uninstalled`，formula 目录移除。未修改系统 `/opt/homebrew` |
| 真实迁移条件 | 前缀 11 字符满足 bottle 重定位上限；`bin/brew` 只读以通过继承沙箱检测；补齐 `Caskroom` 后 `brew list --versions` 正常 |
| 静态/构建 | 随本轮全量门禁执行 fmt、clippy `-D warnings`、测试与构建 |

## 边界与后续

- 真实验收使用的是 **Tier 3 自定义前缀**，不是系统 `/opt/homebrew`；系统 Homebrew、其他 formula、
  其他平台与真实失败/取消残留仍待补。依赖会保留在 Cellar，卸载不自动清理共享依赖。
- 卸载只处理唯一 keg 且无反向依赖的 formula；不自动卸载共享依赖，也不改写 venv/项目引用。
- 第三方 tap、精确补丁版、Homebrew Python 的 `python3` 别名与多 keg 选择未开放。
- 下一步：系统/其他 formula 与失败取消真实场景；项目配方与 P15/P16。
