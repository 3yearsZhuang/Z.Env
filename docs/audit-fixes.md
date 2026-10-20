# P9 / 38：审计修复与验证

更新日期：2026-10-08。基线为 v0.7.1（bd9928e），以下修改尚未发布。

## 修复范围

| 审计问题 | 修复结果 | 回归证据 |
| --- | --- | --- |
| 发现项目 B 后仍可能写入项目 A | 编辑会话统一持有目标、草稿与原文；过时载入丢弃；改路径后必须重新载入；保存核对原文 | `useProjectEditor.test.tsx`、`project.rs` |
| 手写 TOML 解析和正则修改损坏配置 | `toml` / `toml_edit` 结构化处理；保留非目标配置与注释；同目录原子写入、上次内容备份；拒绝无效文档和外部修改 | `config.rs`、`env_center.rs`、`preset.rs` |
| 空项目、隐藏配置和旧格式目标不明确 | 空项目可初始化；目录中的 `.mise.toml` 保持为目标；`.tool-versions` 转换为 `mise.toml`，不修改旧文件 | `project.rs` |
| 安装互斥依赖页面状态、日志串任务 | 后端统一变更任务；整套预设占用一个任务；任务 ID 过滤；页面状态保留；任务面板与安装弹窗支持取消 | `operations.rs`、`useMachineInstall.test.tsx`、`api.test.ts` |
| 子进程超时后继续运行、输出无限增长 | 统一执行器按退出码判断；限制输出与日志；超时和取消终止进程树；支持空成功输出、UTF-8 和 CR/LF | `process.rs`、`operations.rs` |
| Linux 安装命令缺少真正的包管理器 | 使用 `pkexec /usr/bin/apt-get` 或 `/usr/bin/pacman`；参数分开传递；说明 polkit 桌面代理要求 | `env.rs` 命令参数测试 |
| 不同包管理器直接使用同一工具名 | 共享平台包目录，保留规范 ID 与 formula/cask 类型；无映射不显示安装渠道；安装后核验实际 ID | `packages.json`、`env.rs` |
| winget 识别成显示名的第一个词 | 解析 winget JSON 导出的 `PackageIdentifier`；brew JSON 同时解析 formula/cask | `env.rs` 身份解析测试 |
| systemd 把开机启用状态当运行状态 | 使用用户级 `list-units --all` 的 active/sub 状态；操作只判断退出码 | `services.rs` |
| 托管对账静默跨版本升级或删除绑定 | 以真实 formula 身份及精确版本对账；来源消失保留待处理；仅修改可确认归属的链接；失败恢复原链接；清单错误不再吞掉 | `managed.rs` |
| 接管空目录也可能成功 | 检查注册状态、目标目录的真实可执行文件和版本；拒绝覆盖独立安装；补记已存在接入的清单 | `managed.rs` |
| 快照丢失额外激活版本、复杂 env、cask；安装后未激活 | v2 保存全部全局激活版本、可选结构化 env 和 brew 类型；恢复前预览；全部安装成功后提交并核验激活，失败回滚全局配置 | `snapshot.rs` |
| shell 检查误认注释或其他 shell 配置 | 根据当前 shell 选择配置；支持 ZDOTDIR/XDG 和 Windows PowerShell；忽略注释；fish 修复幂等；只声称配置存在 | `doctor.rs` |
| 历史日志保存环境变量明文 | 新记录只记键和结果；旧 `env-set` 记录读取时隐藏值；失败详情不再复制到操作历史 | `history.rs` |
| 依赖漏洞、平台 CI 覆盖不足 | 更新 source-map-js 和测试依赖；全依赖 npm audit；Rust CI 覆盖 Linux、Windows、macOS；发布前执行测试 | lockfiles、CI / release workflows |
| 文档把目标或历史局部验收写成全面能力 | README、计划、发布手册标明当前边界；GPU 为静态硬件信息；已实现与已发布分开 | README、plan、release runbook |

## 行为与数据边界

- `.zenv-backup` 保存前一次写入内容。项目保存和快照提交拒绝覆盖已检测到的外部修改；这不是对其他编辑器的强制文件锁。
- 批量安装失败或取消后，已经成功下载的工具保留。任务有完成、部分失败、失败、取消四种终态；配置和链接的回滚不会被同一取消请求阻断。
- 任务与有限日志保存在应用进程内：切页或 WebView 重载后可查看；退出并重启整个应用后不恢复执行。不是持久任务队列。
- 快照恢复按同名合并，保留本机额外工具与变量。brew 部分生成 `~/.zenv/Brewfile.snapshot`，不自动执行 `brew bundle`。跨平台路径仍需要用户检查。
- 环境变量导出默认关闭。显式包含 env 的快照和配置备份可能含密钥；旧历史文件在磁盘上的既有明文没有自动删除，界面读取时脱敏。
- shell 体检不执行用户 rc 文件。修复追加激活配置并保留备份，是否在新终端真正生效需要平台验收。
- 包目录提供已定义的软件身份映射；仓库版本、网络、许可和授权代理决定能否实际安装。不会把 MySQL 悄悄替换为 MariaDB。
- 预设表单处理单版本请求；多版本配置使用项目 TOML 编辑器，快照完整保存多个全局激活版本。
- Windows 服务列表、Intel Mac 发布产物、实时 GPU 使用率、系统镜像级恢复不属于本轮已实现能力。

## 自动验证

本机 macOS 验证记录在本次修复完成时更新。测试只使用临时文件、进程和模拟 IPC，不安装或卸载用户真实环境。

- 干净 `npm ci` 成功，`npm audit --audit-level=high` 为 0 个漏洞（包含开发依赖）。
- `npm run check:version`：三处版本均为 0.7.1。
- 前端：47 个测试通过；ESLint、Prettier 检查、TypeScript 与 Vite 生产构建通过。
- 后端：57 个测试通过；Rustfmt、Clippy `--all-targets -- -D warnings`、debug 构建通过。
- 使用本机 mise CLI 与独立临时 MISE 数据/配置/缓存目录，验证真实 `mise link` 的注册链接及 `mise ls --global --json` 的 installed/active 契约；临时目录已清理。
- `git diff --check` 通过。Windows/Linux 工作流已配置，尚未在本轮推送并运行，不能宣称三平台验收通过。

## 发布前实机验收

三平台 CI 是编译和自动测试门禁，不代替以下交互验证；本轮未执行真实软件安装、系统授权或发行版发布。

1. Linux：在 apt 与 pacman 桌面分别验证授权成功、拒绝、无代理提示和取消；systemd 用户服务的启停/重启与外部 `systemctl --user status` 一致。
2. Windows：验证 winget 的多词/本地化名称与规范 ID，安装和卸载后的刷新；PowerShell profile 路径、激活与执行策略；进程树取消；软链接权限不足时保留原状态。
3. macOS：验证 brew formula/cask 和同名不同主版本 formula；同版本修复、来源被移除、清单不可写；新终端中 zsh/fish 的激活。
4. 项目：在 A、B、空目录、`.mise.toml`、`.tool-versions` 间切换；编辑期间外部改文件；安装失败后重试；不得串写或丢失外部修改。
5. 迁移：在可丢弃环境导出/恢复多个全局版本及复杂 env；安装失败不得提交全局配置；激活失败应回滚；Brewfile 含正确 formula/cask。
6. 任务：安装中切页、取消、WebView 重载、同时从第二入口发起变更；旧任务日志不得串入新任务，取消后允许后续任务。
7. 发布：三平台工作流全部通过，检查签名工件、版本和更新元数据，再按 `docs/release.md` 发布。
