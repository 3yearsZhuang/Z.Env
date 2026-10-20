# P11 / 40 — PR-02 Node 只读来源与受限探针

日期：2026-10-19。分支：`codex/inventory-contracts`。
状态：**第二个实施工作包通过本机自动检查与 macOS 只读采集验证**。PR-02 是规划中的工作包编号，
本轮未创建远程 Pull Request。随后 PR-03 已完成查询与界面接入，见 [最新实施记录](./p11-inventory-ui.md)；下文保留 PR-02 当时的范围与验证。

## 动因

PR-01 已定义工具与安装实例的统一契约，但尚无来源采集器。本轮建立可独立失败、取消与重试的
macOS Node 来源，保证缺失的管理器不会遮蔽其它来源，也不会在查看清单时触发修复。

## 落地

- [providers](../src-tauri/src/providers/mod.rs) 提供 `scan_node_provider` 和 `scan_node_sources`，
  四来源最多四个工作线程并行；来源分别返回状态、覆盖范围、时间、局部观测和错误。扫描结果直接可供现有纯合并器处理。
- mise 读取 `MISE_DATA_DIR` / `XDG_DATA_HOME` / 默认数据目录中的 node 与 nodejs 安装目录；
  nvm 读取 `NVM_DIR` 或默认目录中的 `versions/node`。目录布局不证明安装所有权，两者保留 Observed。
- Homebrew 独立读取 Apple Silicon / Intel Cellar 和显式配置的 Cellar。只识别登记的 Node formula，
  官方 core 收据与目录身份一致时才标为 Owned；缺失、损坏、第三方收据保留未知归属，不执行其程序。
- PATH 按原始顺序检查全部绝对目录中的 node/nodejs，保留顺序证据、真实路径和断链。
  未知 PATH 程序仅展示文件证据，不自动执行；相对或空目录明确报错并继续其它条目。
- 不调用 mise/brew CLI，不 source nvm.sh 或用户 rc，不调用托管对账、安装、配置更新或任务日志。
  来源可用表示安装目录可以观测，不代表管理器 CLI、项目或用户 shell 已可用。
- 目录名记录为 `declaredVersion` 证据，实际版本与架构只有受控 Node JSON 探针成功后才填入。
  探针使用绝对可执行路径、明确 cwd、清空继承环境后仅设置 HOME 和固定系统 PATH；
  `NODE_OPTIONS`、`NODE_PATH`、动态加载变量和其它应用秘密不会继承。
- [探针执行器](../src-tauri/src/process/probe.rs) 公平读取 stdout/stderr，合并计算输出预算；
  取消、超时、输出超限与读取失败均清理创建的进程组并回收直接子进程，包含继承管道的同组后代。
  旧进程执行器和 P9 写操作流程只新增模块声明，原行为保持不变。
- 原始输出不写历史或观测详情，证据只记录验证结果、退出码和输出长度。未知值、损坏状态与来源错误独立保留。

### 默认预算与保证边界

| 范围 | 默认预算 |
| --- | --- |
| 来源 | 8 秒协作预算；2,048 个目录/路径条目；128 个候选；32 次探针 |
| 收据 | 每来源合计 128 KiB，最多额外读取 1 字节确认溢出；超限或失败读取同样计账 |
| 单个探针 | 3 秒硬超时；stdout + stderr 合计 32 KiB |
| 并发 | 最多 4 个来源、同时最多 4 个探针 |

条目、候选或探针数量触顶会标记截断，不能据此推导未安装。收据只读普通文件，Unix 使用
O_NOFOLLOW/O_NONBLOCK 并再次核对已打开文件，拒绝符号链接与 FIFO。探针执行不构成沙箱；
受支持的管理目录按本机来源信任策略验证，未知 PATH 与未确认 brew 来源等待显式验证入口。

文件系统 API 为同步调用，时间和取消在步骤之间检查；离线网络挂载等场景中，单次系统调用可能阻塞，
因此不承诺任意文件系统下整轮扫描严格按 8 秒结束。新扫描只声明 macOS 支持；其它平台明确返回 Unsupported，
原有跨平台功能继续沿旧入口工作。

## 验证记录

| 检查 | 结果 |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 113 项通过，较 PR-01 新增 33 项 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | 通过 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `cargo build --manifest-path src-tauri/Cargo.toml` | 本机 dev profile 构建通过 |
| `npm test` | 12 个文件 / 70 项通过 |
| 前端 lint、格式、版本一致性与生产构建 | 通过，版本字段仍为 0.7.1 |

临时目录与伪程序覆盖：没有 mise 时保留 nvm/brew；双 Cellar 同版本；软链接别名、断链、缺失及
不可执行文件；来源权限与相对路径错误；真实版本不同于目录名；不调用管理器或 rc；未知程序不执行；
输出、目录、候选、探针与多收据共享预算；超时、取消、局部成功与后代回收；受保护配置原文不变。
独立审阅发现的共享收据预算计账与相对 mise 根目录问题均已修正并加入回归测试。

本机执行 [只读示例](../src-tauri/examples/node_inventory.rs)：

```sh
cargo run --manifest-path src-tauri/Cargo.toml --example node_inventory
```

本次结果：四来源均为 ok、完整且未截断；合并后保留 **16 条来源实例记录、10 条同实体关联**，
验证到五个精确版本。示例执行含启动约 2 秒，不作为性能基准；计数包含同实体的不同来源记录。
前后核对 **10 个配置路径**，内容、存在状态及链接目标均无变化。
原始快照、配置指纹与摘要保存在本机
`~/.codex/visualizations/2026/10/19/01a14fe4-ee6b-7ec1-933e-d7d2e53c89f8/pr02-node-inventory/`，
其包含本机路径，不加入仓库。

本轮没有增加 IPC 命令或页面，没有执行安装、卸载、修复、服务启停或缓存清理。未进行新界面的浏览器验证、
远程三平台 CI、依赖安全审计或发布验收，未提交、合并或发布。macOS 只读验证不能代替其它平台验收。

## 接手与下一步

实施前已备份 73 个现有变更文件。应用源码仅向 lib.rs 与 process.rs 添加模块声明，新功能位于新增目录；
逐文件核对确认其余旧代码（含 PR-01 和 P9/P10）完整保留。

当时下一步为 **PR-03**（现已完成，见上方记录）：注册清单查询与刷新取消命令，将统一来源状态、过期数据、重试、实例与关联证据接入工具页；
移出旧页面刷新链中的自动修复调用，保留显式修复入口。完成该工作包后再评估 P11 阶段退出条件。
