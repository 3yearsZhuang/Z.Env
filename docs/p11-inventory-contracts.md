# P11 / 40 — PR-01 工具清单契约与纯合并器

日期：2026-10-19。分支：`codex/inventory-contracts`。
状态：**首个实施工作包已完成本机自动验证，P11 整阶段仍进行中**。
PR-01 是 [重构规划](./refactor-plan.md) 的工作包编号，本轮未创建远程 Pull Request。
阶段进度见 [plan.md](../plan.md)。
后续 PR-02 已完成只读来源与受限探针，当前验证与接手位置见 [来源实施记录](./p11-node-sources.md)；
下文保留 PR-01 交付时的范围与验证基线。

## 动因

当前工具状态分散在 mise、外部管理器和系统包的多套数组中，项目缺失判断与工具页也没有共用口径。
先明确工具、渠道包、安装实例、来源观测与关联证据，给后续只读 adapters 和统一界面建立可测试的边界。
本工作包先加入纯内核，现有查询和写操作入口继续沿用原实现。

## 落地

- [领域模型](../src-tauri/src/domain/mod.rs)：工具/安装 ID、原生包、平台、健康与归属、观测证据、
  来源状态与覆盖范围、安装实例、alias 和版本化 inventory 快照。Rust 与 JSON 使用 camelCase；
  未知版本、路径、架构与错误槽位显式输出 null。
- Node 的 `node` / `nodejs` / `node.js` 仅按登记的别名归一。Homebrew `node`、`node@22` 和 core
  限定名称可映射到 Node 工具，原始包 ID 保留；第三方 tap 或 cask 不因名称相近自动认作官方运行时包。
- [纯合并器](../src-tauri/src/inventory/mod.rs)：只处理传入观测，无文件系统、进程、安装或修复副作用。
  来源失败和局部观测保留；过期标记逐来源传到实例。成功、完整、未截断且新鲜的来源覆盖，才可能支持该来源范围的缺失判断。
- 安装 ID 使用 UTF-8 长度前缀与独立的 null 标记，包含工具、provider、包、平台、定位符、路径、
  版本和架构。同版本不同目录保持独立，版本或真实目标变化产生新身份；未知字段不会与空串产生编码碰撞。
  ID 是本机定位身份，包含路径信息，不作为跨机器迁移标识。
- 重复观测合并证据并稳定排序；健康冲突采取损坏/待验证状态，归属冲突保留未知。
  provider、包、路径大小写等信息不被丢弃；PATH 来源不能声称安装所有权。
- 同工具、同平台且规范可执行路径一致时可建立 alias，同时保留两份实例。过期、损坏的路径不参与；
  整组版本或架构存在冲突时不建立关联，未知版本不会把矛盾实例串联。关联不填补实例未知字段，不授予卸载权。
- [TypeScript 契约](../src/api/inventory.ts) 与现有 `api.ts` 类型导出兼容；新包类型以
  `InventoryProviderPackage` 导出，避免覆盖原有软件渠道类型。
- [共享 JSON](../fixtures/inventory/contract-v1.json) 包含输入扫描与固定预期快照：两个不同目录的
  brew Node、一份 PATH 关联、一份损坏且版本未知的 nvm 观测、超时的 mise 来源。
  Rust 合并后的真实序列化结果与其精确比较；TypeScript 从 unknown 逐字段校验必填字段、枚举、null、时间与形状。

## 验证记录

| 检查 | 结果 |
| --- | --- |
| `npm test` | 12 个文件、70 项通过；本轮新增 3 项共享契约测试 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 80 项通过；本轮新增领域 7 项、合并器 16 项 |
| `npm run lint`、`npm run format:check` | 通过 |
| `npm run check:version` | 通过，版本字段一致为 0.7.1 |
| `npm run build` | TypeScript 与 Vite 生产构建通过 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | 通过 |
| `cargo build --manifest-path src-tauri/Cargo.toml` | 通过，本机 dev profile |
| `git diff --check`、相关文档本地链接及新增文件空白检查 | 通过 |

边界场景包括：输入顺序与重复证据不影响结果；多目录同版本不合并；同路径版本或架构冲突；
未知值不补猜；失败来源保留其他来源；截断/过期不推导缺失；错误 provider、工具映射、非法字段和
超出 JavaScript 安全整数范围的时间被拒绝。两次独立只读代码审阅未发现阻塞问题。

未增加 Tauri 查询命令、真实扫描、页面或安装流程；没有执行真实软件安装、卸载、服务启停和缓存清理。
前端产物保持原有页面行为，测试不代表新清单已在桌面界面可用。未重跑浏览器矩阵、远程三平台 CI、
依赖安全审计或发布验收，未提交、合并或发布。

## 接手边界与下一步

实施前已备份当时全部 66 个变更文件及工作树差异，并新建分支保留原内容。既有应用代码仅增加
Rust 模块注册和 TypeScript 类型导出，其余 P9/P10 代码不作迁移；本工作包新增代码均在新目录中。
逐文件 SHA-256 与原文比较已确认：原 66 个文件无缺失，既有应用源码除这两处增量注册/导出外原样保留；
其余发生变化的原文件只有 README、活计划、重构规划和交接文档。

下一步为 **PR-02**：用受限、独立的 macOS Node adapters 接入这些输入契约，提供路径、探针、
时间和失败证据。该工作包需要完成目录/输出预算、显式探针环境、来源超时与取消，并证明只读扫描
不会触发托管修复或写配置；随后 PR-03 才接入统一清单查询与页面。
