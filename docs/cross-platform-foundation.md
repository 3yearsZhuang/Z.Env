# 三平台执行底座：有界探针与 Windows 文件身份

日期：2026-10-20（Asia/Shanghai）。本记录覆盖三平台实施工作包 02 的首个切片。
**源码已接入；Windows 原生探针已首次在 CI 运行，仍有 1 个场景待复核，完整应用构建及工作包收口仍待验收。**

## 动因

原有有界探针在非 Unix 平台直接返回 unsupported；操作计划的文件证据在 Windows
只比较规范路径、长度和修改时间，无法区分保持相同长度和时间的文件替换。
先补执行与身份保障，再开放 Windows 的工具适配和写能力。

## 落地

- Unix 与 Windows 共用探针的输出、超时、取消、双流轮询和退出状态逻辑，调用方的参数、cwd
  与环境由其自己设置；探针不增加 PATH、不执行 shell 初始化，也不写任务 journal。
- Windows 使用稳定 Rust 与 Win32：以 `CREATE_SUSPENDED` 启动，先加入不可继承、
  `KILL_ON_JOB_CLOSE` 的 Job，再定位初始线程、核对其进程归属并恢复运行。
  附加或恢复失败时不降级为无管理执行；守卫关闭 Job、终止并等待直接进程。
- 初始进程退出后仍清理本次 Job 中的后代。正常返回、错误、取消、输出超限和超时均走同一守卫。
  调用方可复用的 Command 不保留探针加入的挂起标记。
- Windows 独占读取管道，`PeekNamedPipe` 后只读已到达字节，避免为每个探针创建可能阻塞的读取线程。
  stdout/stderr 合用总字节预算；一个流持续输出不会阻止检查另一个流或取消。
- Windows 文件证据记录卷序列号、128 位 File ID 及属性。通过已打开句柄确认普通磁盘文件，
  `OPEN_REPARSE_POINT` 防止打开时静默跟随被替换的最终链接。
  已由上层解析的正常 alias 仍可用；设备、命名管道、ADS 和保留 DOS 设备名不作为配置或可执行证据。
- Windows API 使用锁文件已有的 `windows-sys 0.61.2`，新增直接目标依赖及所需 feature；不更新其他依赖版本。
- 流式执行器增加 `EnvironmentPolicy::AsConfigured`，不再强制覆盖明确配置的 PATH/cwd。
  旧 `run`/`output` 保留兼容策略；已有显式 PATH 的 `mise::run_mise` 使用新策略。
  环境清理仍由调用方负责，不把“保留配置”当作自动隔离。
- CI/发布的 Rust clippy、测试和构建使用 `--locked`；Windows 默认原生场景由现有平台矩阵运行，
  配置接入不等于远程 job 已运行。

实现沿用 [探针入口](../src-tauri/src/process/probe.rs)、
[Windows 平台细节](../src-tauri/src/process/probe_windows.rs)与
[操作计划文件守卫](../src-tauri/src/operation_plan/guards.rs)。

## 验证与边界

| 检查 | 结果及边界 |
| --- | --- |
| 前端基线 | 27 文件 / 177 项通过；本切片未修改前端 |
| 最终本机 Rust 全量 | 232 项通过，较 227 基线新增成功终态后代回收及 4 个环境策略场景 |
| Rust 格式、all-targets clippy、构建 | 通过，使用当前锁文件 |
| 前端版本、lint、格式、构建、依赖审计 | 通过，审计报告 0 漏洞 |
| Windows 目标隔离检查 | 真实进程/探针源码与真实文件守卫及其 Windows tests 分别 `check --tests`、clippy `-D warnings` 通过；外围类型使用小型 stub，不是完整应用编译 |
| Windows 原生场景（2026-10-21 CI） | **132 通过 / 0 失败 / 2 ignored**；ignored 仅为子进程 fixture 与需要 Windows 符号链接权限的文件守卫测试。直接进程退出后终止 Job 后代并返回父状态，超时用例改为直接进程慢启动 |

Windows 探针 fixture 由测试二进制派生；在输出洪泛、取消和超时之前先取得观察句柄，
再通过确认文件让 fixture 继续运行，避免用已回收的 PID 猜测终态。断言失败时先兜底清理。
双流预算用固定载荷与组合溢出验证，避免 libtest 前缀在 Windows 上的字节差异；另有 argv/cwd/Unicode、
父退出终止后代并返回父状态、直接进程超时回收、正常成功后代回收和 Command 复用场景。
文件守卫覆盖同内容/长度/mtime 替换、hardlink、设备路径拒绝及正常/verbatim/UNC/缺失配置。

验收区分本机 Unix 测试、Windows 目标编译、Windows 原生测试和完整桌面 IPC；跨编译不证明
进程树或文件系统在 Windows 上已实际验收。

- 全应用 Windows 交叉检查在 `ring 0.17.14` 的 C 编译阶段遇到缺少 Windows SDK `assert.h`，
  不能据此声明完整应用编译通过。单独检查真实探针与文件守卫的 Windows 分支，避免此平台分支完全不受检查。
- File ID 不可用的文件系统拒绝提供操作证据。正常 UNC 路径可保留，但远端文件系统访问和
  Windows `canonicalize` 没有本模块提供的 OS 级超时；输出和进程预算不构成沙箱。
- 需要 Windows 符号链接权限的额外测试明确 ignored，不计作默认通过。
- 此切片不开放 Windows 的 Node/Python 安装、目录扫描或绑定能力；目录账本仍保持实际范围。
  原生 launcher、Windows provider 的受控环境、适配器、统一计划 v2 与实际桌面流程继续按活计划实施。

本地证据目录：
`/Users/3yearszhuang/.codex/visualizations/2026/10/20/01a15823-350f-7f61-9551-bb80094c52ba/`。
其中 `foundation-final-native-tests.log` 保存 232 项全量结果，`foundation-native-tests.log` 是此前 228 项的阶段结果；
`windows-probe-check/` 与 `windows-guards-check/` 保存隔离编译入口和锁文件，不用于替代仓库 CI。
`conversation-inventory.json`、恢复包及 `foundation-preservation-audit.json` 记录接手与保全边界。

Win32 依据：[Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)、
[PeekNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe)、
[FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)、
[CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)。
Rust 的 `spawn_with_attributes` 仍为 nightly API，因此本切片没有引入 nightly 要求，
见 [CommandExt](https://doc.rust-lang.org/std/os/windows/process/trait.CommandExt.html#tymethod.spawn_with_attributes)。
