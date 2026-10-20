# 七类工具与三平台重构实施方案

日期：2026-10-20（Asia/Shanghai）。状态：**用户已确认范围，实施进行中**。
本文承接 [2026-10-19 架构方案](./refactor-plan.md)，阶段进度只在 [活计划](../plan.md) 维护。
既有 macOS Node 与 Python mise 切片保留；不从 PR-01 重新开始，不把新目标误记为已实现。

## 目标与完成线

Node、Python、Java、Go、Rust、Git、uv 七类工具均覆盖三平台新流程：发现、查看来源、
计划预览、安装、显式升级、绑定、实际验证、解除和受支持卸载。目录登记、模拟测试、
跨编译或管理器零退出码不能单独作为闭环完成证明。

- 默认验收：macOS 13+ ARM64 与 Intel、Windows 11 x64、Ubuntu 22.04/24.04 x64。
  保留既有其他 Linux 发行版与 pacman 兼容能力；按渠道记录支持和未覆盖项。
- Python 本轮处理解释器选择、项目要求、绑定及已有 `.venv`；不创建虚拟环境、不安装或同步项目依赖。
  conda、复杂 uv workspace、动态 Poetry 配置不猜测，保持未知并说明原因。
- pyenv 在支持的 Unix 平台加入受控源码编译计划，明确构建依赖、插件和 rehash 影响；
  不将系统依赖安装藏入解释器安装，不用 Unix pyenv 假装支持 Windows 原生 Python。
- mise 为可选 provider。保留 Tauri/Rust/React、原配置原子写、全局写互斥、取消及日志保护。
  不引入通用插件 SDK、任意用户脚本执行、云同步或破坏性数据库迁移。

## 渠道与工具语义

| 工具 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Node | mise `core:node`；保留 Homebrew 写渠道与 nvm 观测/绑定 | mise `core:node` | mise `core:node`；兼容系统包渠道逐项接入 |
| Python | mise `core:python`；补 uv、Homebrew、pyenv | uv managed CPython | mise `core:python`；补 uv、pyenv |
| Java | mise `core:java`，默认 Temurin JDK | 同左 | 同左 |
| Go | mise `core:go` | 同左 | 同左 |
| Rust | 原生 rustup | rustup，MSVC host | 原生 rustup |
| Git | Homebrew core `git` | WinGet `Git.Git`、固定 winget source | 官方发行版 apt `git`；保留 pacman |
| uv | 官方固定 release 二进制，Z.Env 自有目录托管 | 同左 | 同左 |

每个开放渠道独立核验版本、平台、架构、管理器能力、安装收据与副作用。缺管理器时生成独立前置计划，
不静默下载；精确版本与渠道当前版本承诺分开，执行中不换来源。
现有 winget/apt/pacman 等兼容入口按替代矩阵迁入统一计划，不批量移除旧能力。

- Node 保持 nvm/PATH 的所有权边界，不把观测目录转交其他管理器卸载。
- Python 区分解释器、虚拟环境、项目声明和实际执行；检查已登记项目及 `.venv` 的基础解释器引用。
  uv 禁止隐式写 bin 和 Windows 注册表；mise 保留禁源码回退、禁默认 pip 包的现有策略。
- Java 将 vendor、完整 build、JDK/JRE 纳入身份，验证 `JAVA_HOME`、java、javac。
- Go 验证显式工具链路径，禁止探针隐式下载其他 toolchain；最小编译在临时项目中执行。
- Rust 区分 rustup 与工具链，固定精确版本及 host triple；卸载工具链不等于卸载 rustup。
  Windows linker/SDK 单独作为前置条件，安装与最小编译分别验收。
- Git 只承诺已预览的渠道可供版本；绑定改变本工具的项目/全局路径选择，不修改仓库身份、凭据或 hooks。
  不卸载 Apple CLT 或无归属系统 Git，已登记项目和渠道依赖影响先展示。
- uv 固定版本/架构/摘要，暂存验证后原子落位；升级和卸载只处理本应用的实例。
  Python、cache、tool 数据独立保留。所有绑定都只改变选择或入口，解除不删除原安装。

## 接口与执行规则

- 扩展 inventory 的工具和来源身份，继续保留独立快照、局部失败、过期状态、alias 和实际生效证据。
  ProviderId 增补 rustup、winget、apt、pacman、官方二进制托管；不是给任意名称授予执行能力。
- 新增 `operation_plan_v2`，目标采用工具、配方、诊断修复、服务、缓存、快照恢复的 tagged 类型。
  后端签发计划，执行仍只提交 `planId`；旧 Node/Python v1 请求经后端转换后签发，旧 journal 可读。
- 计划绑定快照、实例、文件身份、配置指纹和有效期；单次消费，在写锁内重验。
  步骤记录 stable ID、依赖与状态，复用现有 journal、取消、补偿和终态重扫，不另建执行器。
- Windows 先补有界进程、稳定文件身份和原子配置保护，再开放适配器。
  原生 launcher 保留 argv、退出码与环境，回映真实实例；安装和删除仅针对本应用拥有且指纹一致的文件。
  Unix 使用原有受控链接策略，不能将 Windows symlink 权限当作默认前提。
- 项目环境 API 增加工具维度；分别返回声明、候选、选择、受控上下文、应用 PATH 与用户终端状态。
  未执行的终端/IDE 验证保持未知。配置仍复用 TOML AST、备份和外部修改检测。
- 配方 v2 表达工具要求、渠道选择、作用域和依赖图；先安装再配置再验证。
  部分完成保留成功项，重试重新扫描与签发，仅包含仍需执行的失败/未执行项。
  继续读预设 v1、纯 mise 配置；不自动运行项目 task、安装依赖或 hook。

## 工作包与依赖

| 顺序 | 工作包 | 退出条件 |
| --- | --- | --- |
| 01 | 接手、恢复包、对话汇总与归档核实 | 10 个其他对话可追溯；恢复清单校验；当前对话为唯一未归档入口 |
| 02 | 跨平台执行和领域接口 | Windows probe、文件保护、launcher、环境策略、v2 兼容及原生场景通过 |
| 03 | Node 三平台、P12 收口 | 补真实 Homebrew 写入/托管和打包 IPC；Windows/Linux Node 全流程完成 |
| 04 | Python uv | 三平台精确 CPython 安装、卸载、重扫及 bin/注册表副作用核验 |
| 05 | Python Homebrew、pyenv | formula 与依赖影响明确，受控编译及归属/失败/取消场景通过 |
| 06 | Python 项目环境与绑定 | 解析 mise、`.python-version`、`requires-python`；已有虚拟环境与基础解释器分别判断 |
| 07 | Java、Go | 三平台逐工具安装、升级、绑定、解除、卸载与实际验证闭环 |
| 08 | Rust | rustup/工具链身份、作用域、版本/host、原生卸载及最小编译验收 |
| 09 | Git、uv | 渠道归属或自有托管、升级/卸载、路径选择、共享数据保护闭环 |
| 10 | 项目配方 | 后端步骤依赖、预览、部分失败、失败步骤重新签发与旧预设兼容 |
| 11 | P15 诊断与运维 | doctor 只读与修复分离；PATH 差异/备份/恢复；服务缓存归属及统一计划 |
| 12 | P16 快照与旧入口收口 | v3 迁移意图、v1/v2 读取、三平台重建、替代矩阵、Intel Mac 产物及更新验收 |

基础契约冻结后工具适配可并行开发；真实写入在独立测试环境中执行。每包保存独立实施记录和可审阅提交，
与既有的 P9–P14 修改分清来源（这些实现已于 2026-10-20 分批次提交）；未经过全量验证不把大工作树打包成“纯重构”。

## 验收与发布门槛

- 纯契约覆盖身份、跨来源同版本、构建/架构、过期和未来格式；临时文件和伪 provider 覆盖并发改写、
  权限失败、取消、超时、零退出无产物、错误版本、补偿冲突和重启中断。
- Windows/Linux/macOS 使用真实 WebView IPC 验收；测试自动化插件仅在测试构建启用，正式产物另做安装启动。
  保留前端草稿、焦点、键盘、错误重试、明暗和 390/460 像素布局。
- 每个开放工具/provider 按实际 OS/架构、管理器版本记录完整写流程和保护路径变化。
  UAC、polkit、shell、新终端、服务、缓存及更新验收分别保存，不从单元测试推导。
- 包管理器事务不能立即停止时显示“取消处理中”，待结束/安全终止后重扫与报告残留。
  已登记项目引用必须展示，不承诺扫描到机器上所有未知消费者；不自动卸载共享依赖补偿失败。
- P15 的 Windows 服务先有只读证据再开放写；缓存沿用官方清理命令和目录预算。
  手动流程稳定后再接定时巡检，不能用后台任务掩盖手动流程缺口。
- 快照 v3 分开观测与迁移意图，保留 v1/v2 读取；源机器绝对路径不成为目标绑定，秘密默认不导出。
  导入先预览重签；旧写入口仅在对应功能及平台替代矩阵通过后退出。
- 各包运行现有版本、lint、格式、测试、构建及依赖审计门禁；完成线分别标注“实现、自动验证、实机验证、发布”。
  macOS ARM/Intel、Windows、Linux 和 updater 产物均有独立验收证据；正式发布沿独立发布流程进行。

渠道依据：[mise core](https://mise.jdx.dev/core-tools.html)、
[mise Windows](https://mise.jdx.dev/faq.html#does-mise-work-on-windows)、
[uv Python CLI](https://docs.astral.sh/uv/reference/cli/#uv-python-install)、
[pyenv](https://github.com/pyenv/pyenv#installation)、
[rustup Windows](https://rust-lang.github.io/rustup/installation/windows.html)、
[Git 安装](https://git-scm.com/install/)、
[Tauri 原生测试](https://v2.tauri.app/develop/tests/webdriver/)。
