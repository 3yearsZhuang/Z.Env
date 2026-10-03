# Z.Env

**整机环境管理中心** —— 跨平台 **mise** 图形化管理器。安装交给 brew / winget 等系统包管理器，
项目绑定交给 mise，Z.Env 做两者之间的**托管接入层**，并用可视化界面统合系统监控、运行时管理、
环境变量、服务与缓存。判定标准：新机装好 Z.Env + mise 后不打开终端，能装齐软件、配好 shell
接入、跑起项目环境。

[![CI](https://github.com/3yearsZhuang/Z.Env/actions/workflows/ci.yml/badge.svg)](https://github.com/3yearsZhuang/Z.Env/actions/workflows/ci.yml)

> 原名 EnvForge，现更名为 **Z.Env**。

## 下载安装

从 [GitHub Releases](https://github.com/3yearsZhuang/Z.Env/releases/latest) 下载对应平台的安装包：

| 平台                   | 安装包                               |
| ---------------------- | ------------------------------------ |
| macOS（Apple Silicon） | `Z.Env_<ver>_aarch64.dmg`            |
| Windows（x64）         | `Z.Env_<ver>_x64-setup.exe` / `.msi` |
| Linux（x86_64）        | `.AppImage` / `.deb` / `.rpm`        |

- 应用内置**自动更新**（更新包经 minisign 签名验证）：设置 → 应用更新 → 检查更新，
  下载完成后自动换包重启，无需手动重装。
- 已知平台缺口：Windows 服务列表暂缺（待实机验证后提供）、Intel Mac 产物未覆盖。
- 各版本变更摘要见 [CHANGELOG.md](./CHANGELOG.md)。

## ✨ 功能

**系统概览**

- CPU / 内存 / 磁盘 / 网络 / 电量 / GPU 实时占用（正方形亚克力卡片）
- 历史使用率曲线、网络上下行双环
- 硬件型号首次采样缓存；顶部环境横幅（系统 + git + 各包管理器）
- 刷新间隔自选（0.1 / 0.5 / 2 / 5 秒 / 关闭）

**支持列表**

- 全量软件/运行时清单，与「环境」页共用同一套分类标签
- mise 可安装运行时 + 官方下载软件；系统包管理器（brew/winget/apt/pacman）一键原生安装
- 已安装状态真实探测 + 10 分钟 TTL 缓存，安装/卸载后自动失效

**环境**（整机环境的职能闭环：健康 → 运行时 → 变量 → 预设/快照）

- 顶部按钮切换「运行时 / 环境变量 / 环境预设」三个区块；区块常驻挂载，切走不丢进行中的流式安装
- **环境健康**：mise 可用性、shell 集成、PATH 重复与失效目录、托管接入健康度巡检，
  可修复项（shell 接入、托管对账）一键修复，装好 ≠ 生效的问题就地解决
- 运行时：聚合本机所有来源——mise 管理的 + 其他渠道托管（nvm/pyenv/asdf/brew 等，带渠道标签）；
  一键安装（流式进度 + CLI 输出）、卸载、设为全局、外部渠道**一键接管**（自动选策略并校验回滚）
- 环境变量：全局 mise `[env]` 增删改 + 同名冲突提示 + 系统/用户级 env 只读查看
- 环境预设与整机快照：见下文对照表
- 「装到整机」与「快照重建」共用同一条流式安装通道；两路**互斥**——同一时刻只允许一路安装，
  进度日志不串台

**项目配置**

- **项目发现**：扫描常用目录自动识别技术栈项目，对照本机已装列出缺失工具，点击即载入编辑流程
- CodeMirror 编辑器查看/编辑项目 `mise.toml`（TOML 高亮，明暗自适应）
- 内置技术栈预设 + 自定义用户预设；一键安装全套环境到**该项目**
- 预设可导出为 `.zenv.toml` 分享给他人，也可导入他人分享的预设文件（兼容纯 `mise.toml`）
- 整机环境绑定：把 brew/scoop 运行时以 PATH 方式写入项目 mise.toml（零链接、零侵入）

**服务与缓存**

- 本地开发服务：brew services（macOS / Linuxbrew）启停与重启；systemd 用户级服务（Linux）
- 开发缓存治理：brew / mise / npm / yarn / pip / uv / Go / cargo registry 八类体积统计
  （大目录带预算截断），清理只走官方命令，绝无自带 rm

**设置**

- 深浅色主题（跟随系统/浅色/深色）、开机自启、应用内检查更新与自动重启
- 操作历史：本机管理动作留痕（`~/.zenv/history.jsonl`，成功失败都记，不上传）
- 彩蛋 🥚：版本号连点 6 次解锁

## 环境预设 vs 整机快照

两者都在「环境」页，但解决的问题不同——一个是**配方**，一个是**照片**：

|          | 环境预设                                          | 整机快照                                                  |
| -------- | ------------------------------------------------- | --------------------------------------------------------- |
| 是什么   | 一份配方：**我要装什么**                          | 一张照片：**我这台机器现在是什么样**                      |
| 内容     | 工具 + 版本请求（`20` / `latest` / `temurin-21`） | mise 已激活工具（精确版本）+ 全局 `[env]` + brew 软件清单 |
| 内容来源 | 内置技术栈 / 自己保存 / **他人导入**              | 本机实测采集                                              |
| 主要场景 | 起一个新环境、接收同事的配置                      | 换机迁移、重装系统前备份                                  |
| 安装去向 | 整机（环境页）或项目（项目配置页）                | 仅整机                                                    |
| 可分享   | 是，`.zenv.toml` 单文件                           | 是，同一个快照文件                                        |

- **预设**：`[preset]` 元数据 + `[tools]` 工具段，人类可读可手改。别人即便不用 Z.Env，把 `[tools]` 段当普通 `mise.toml` 用也可以。导入时也接受纯 `mise.toml`（无 Z.Env 标记时按文件名命名）。
- **快照**：面向迁移的完整打包。重建时全局 env 直接写入、mise 工具自动安装、brew 软件生成 Brewfile 交由你执行 `brew bundle`，不静默批量装软件。
- 两者的 mise 工具安装走**同一条流式通道**（实时进度 + 单个失败不中断）。

## 托管接入（整机环境 → mise）

安装交给系统包管理器，项目绑定交给 mise，Z.Env 负责两者之间的托管接入：

| 策略      | 适用来源                                         | 机制                                                                 |
| --------- | ------------------------------------------------ | -------------------------------------------------------------------- |
| 直连接管  | nvm / pyenv / asdf / sdkman / volta 等用户级目录 | `mise link` 直连 + 注册校验                                          |
| 托管接管  | brew                                             | 软链指向应用农场 `~/.zenv/managed/`，brew 升级后**自动重连**到新版本 |
| PATH 绑定 | brew / scoop                                     | 项目 `mise.toml` 写入 `[env] _.path`，零链接零侵入                   |

- openjdk 系列自动修正到真实 Home 布局（`libexec/openjdk.jdk/Contents/Home`）
- 所有接入动作带 `mise ls` 注册校验与失败回滚
- 每次扫描对账自愈：目标目录被包管理器升级/删除时自动重连或显式移除，不再有悬空链接

## 设计

- **Tailwind CSS v4 + shadcn 设计令牌**（zinc 中性色 + 蓝色主色，明暗两套）
- **lucide-react** 图标；macOS 液态玻璃侧边栏 + Windows 亚克力
- Radix Dialog 弹窗（焦点圈定 / ESC / 无障碍），CodeMirror 主题自适应
- 环境页区块切换为标准 tablist 语义（roving tabindex 方向键导航 + tabpanel 关联）

## 技术栈

- [Tauri 2](https://v2.tauri.app/) + [Rust](https://www.rust-lang.org/)
- [React 19](https://react.dev/) + [TypeScript](https://www.typescriptlang.org/) + [Vite](https://vitejs.dev/) + [Tailwind CSS v4](https://tailwindcss.com/)
- [mise](https://mise.jdx.dev) · ASDF · GitHub Releases · 官方生态源 · brew/winget/apt/pacman

## 开发

```bash
npm install            # 安装前端依赖
npm run tauri dev      # 启动开发模式（拉起桌面窗口）
npm run build          # 构建前端（tsc + vite）
npm run lint           # ESLint
npm run format         # Prettier 格式化 src（CI 有 format:check 门禁）
npm test               # vitest 单测
npm run check:version  # 校验三处版本号一致（package.json / tauri.conf.json / Cargo.toml）

cd src-tauri
cargo test             # 后端单测
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

### 环境要求

- [Node.js](https://nodejs.org/)（推荐用 [mise](https://mise.jdx.dev) 管理）
- [Rust](https://www.rust-lang.org/) toolchain
- Linux 需系统依赖：`libwebkit2gtk-4.1-dev`、`libxdo-dev`、`libssl-dev` 等

## 代码约定

- 注释统一使用中文；单行／行内用 `//`，模块级说明用 `///`（Rust）。
- Rust 按 `rustfmt` 规范排版；前端经 ESLint（typescript-eslint）与 Prettier 约束。
- 后端按域分模块：`sources/`（版本源）、`managed.rs`（托管接入）、`env_center.rs`（环境变量中心）、
  `discover.rs`（项目发现）、`doctor.rs`（环境体检）、`services.rs`/`caches.rs`（服务与缓存）、
  `preset.rs`（预设解析）、`snapshot.rs`（整机快照）、`history.rs`（操作历史）、
  `cache.rs`/`syscache.rs`（双层缓存）、`net.rs`（网络）、`error.rs`（错误模型）。
- 前端基础组件在 `src/components/ui/`，共享目录数据在 `src/data/`，纯函数工具在 `src/lib/`
  （ TOML 手术、预设库、安装通道互斥判定等，均配单测）。

## CI / 发布

- **ci.yml**：版本一致性校验 → 前端 lint / test / build + prettier 门禁 + npm audit（high 及以上阻断）；
  后端 fmt + clippy（-D warnings）+ test + build。
- **release.yml**：推送 `v*` 标签触发，tauri-action 在 macOS / Windows / Linux 三平台打包，
  生成应用内更新所需的 `latest.json` 与 minisign 签名工件。
- **发布操作**：完整步骤与历次实踩的坑见 [docs/release.md](./docs/release.md)
  （版本号三处同步 → 本地全量门禁 → tag → 草稿资产核对 → Publish → 验证 `latest.json`）。
- 更新包使用 minisign 签名，公钥位于 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`；
  签名私钥经 GitHub Secrets 注入（`TAURI_SIGNING_PRIVATE_KEY`，密码项留空）。

## 迭代计划

迭代单一事实源是 [plan.md](./plan.md)（只放向前看的活计划）：P0–P8 已收口，P9 候选已列
（CI 补 Windows 编译覆盖、doctor v2、PATH 治理、平台缺口、项目页纵深等）。
完整执行档案（动因 / 备选取舍 / 踩坑 / 验证数据）见 [docs/devlog-P0-P8.md](./docs/devlog-P0-P8.md)。

## 相关链接

- [GitHub: 3yearsZhuang](https://github.com/3yearsZhuang)
- [mise 官方文档](https://mise.jdx.dev) · [mise 安装指南](https://mise.jdx.dev/getting-started.html)
- [GitHub: jdx/mise](https://github.com/jdx/mise)

## 开源协议

本项目基于 **Creative Commons Attribution-NonCommercial-ShareAlike 4.0 (CC BY-NC-SA 4.0)** 开源，详见 [LICENSE](./LICENSE)。

> 使用约定：需署名、非商业用途、相同方式共享。完整法律文本见
> [CC BY-NC-SA 4.0 法律文本](https://creativecommons.org/licenses/by-nc-sa/4.0/legalcode)。

部分运行时图标准由 [devicon](https://devicon.dev) / [Simple Icons](https://simpleicons.org) / [iconify](https://iconify.design) 提供，版权归各自作者所有。
