# 原生 launcher：按实例启动并保留 argv、退出码与环境

日期：2026-10-21（Asia/Shanghai），分支 `main`。
状态：**后端启动服务与真实 Node 验收完成；生成式 shim、文件归属/安装删除与 UI 入口待继续。**

## 动因

现有探针只服务于只读版本核对；工具适配和项目执行需要从清单中已核验的实例直接启动命令，
同时保留用户参数、工作目录、受控环境与真实退出码。若依赖 PATH shim 或符号链接，Windows
会受链接权限限制，也无法证明实际启动的是清单中的那份实例。本切片先提供后端 launcher：
不经过 shell，直接使用实例可执行文件，并把结果回映到安装身份。

## 落地

- [launcher.rs](../src-tauri/src/launcher.rs) 接收安装 ID、argv、cwd、显式环境与超时/输出预算。
  argv 不包含程序路径，程序只能来自清单实例。
- 启动前核对实例：健康、未过期、非 PATH 观测、可执行文件为绝对路径、规范路径与清单
  `canonicalExecutablePath` 一致、Unix 可执行位存在；不满足即拒绝。
- 环境完全显式：清空继承环境后设置 `HOME`（Windows 另设 `USERPROFILE`）、`PATH` 与调用方变量；
  可执行文件所在目录自动置于 PATH 首位。拒绝非法键值、超预算变量和
  `LD_PRELOAD`/`LD_LIBRARY_PATH`/`DYLD_*` 等加载器注入项。
- 复用 [受控探针](../src-tauri/src/process/probe.rs)：Unix 进程组、Windows Job、超时、取消、
  双流总预算和终态后代回收与现有探针一致；退出码与信号分别返回，输出按 UTF-8 有损解码。
- 结果包含安装 ID、原始/规范可执行路径、退出码、信号、成功标记、stdout/stderr 与耗时，
  便于把运行结果回映到真实实例。
- 原生示例 [launch_instance.rs](../src-tauri/examples/launch_instance.rs) 扫描本机 Node 并执行
  `node --version`。

## 验证记录

| 检查 | 结果与边界 |
| --- | --- |
| Rust 单测 | launcher 4 项：真实 argv/cwd/env/退出码、未知实例/相对 cwd/非法 argv/环境、超时与输出超限回收、非可执行与篡改目标 |
| 真实 macOS 启动 | 清单中的 mise Node 22.23.3：`node --version` 退出码 0，stdout `v22.23.3`，规范路径回映到 `.../node/22.23.3/bin/node`，耗时约 35 ms |
| 静态/构建 | fmt、clippy `-D warnings`、构建与完整测试在本轮门禁中执行 |
| Windows | 代码走共享探针与 Job 守卫，Windows CI 编译/clippy/测试；launcher 单测使用 Unix 脚本，未提供 Windows 原生启动用例 |
| 桌面/UI | 尚无前端入口；本切片只提供后端 API 与原生示例 |

## 边界与后续

- launcher 不是沙箱：显式 PATH 和变量不能阻止被启动程序访问用户文件或网络；权限边界仍由
  操作系统与调用方负责。
- 尚未生成/安装 launcher shim，也未实现“只删除本应用拥有且指纹一致文件”的安装删除流程；
  当前只启动已有实例，不改动任何文件。
- 尚无 Tauri 命令或页面入口；工具适配、项目执行与统一任务日志接入后再开放。
- 下一步：launcher shim 的归属/指纹/安装删除、Windows provider 受控环境与工具适配器；
  之后接项目执行和配方。
