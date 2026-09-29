# Z.Env

跨平台 **mise** 图形化管理器与整机环境管理中心 —— 安装交给系统包管理器，项目绑定交给
mise，可视化界面统合系统资源监控、运行时管理与项目环境配置。

> 原名 EnvForge，现更名为 **Z.Env**。

## ✨ 功能

**系统概览**
- CPU / 内存 / 磁盘 / 网络 / 电量 / GPU 实时占用（正方形亚克力卡片）
- 历史使用率曲线、网络上下行双环
- 硬件型号首次采样缓存；顶部环境横幅（系统 + git + 各包管理器）
- 刷新间隔自选（0.1 / 0.5 / 2 / 5 秒 / 关闭）

**支持列表**
- 全量软件/运行时，与「运行时工具」共用同一套分类标签
- mise 可安装运行时 + 官方下载软件；系统包管理器（brew/winget/apt/pacman）一键原生安装
- 已安装状态真实探测 + 10 分钟 TTL 缓存，安装/卸载后自动失效

**运行时工具**
- 聚合本机所有运行时：mise 管理、其他渠道托管（nvm/pyenv/asdf/brew 等）带渠道标签
- 多远程源版本查询（mise / 官方生态源 / GitHub Tags 翻页 / ASDF），双层缓存防限流
- 一键安装（流式进度 + CLI 输出）、卸载、切换全局版本
- **托管接入**：brew 等来源可接入 mise 并长期保持健康（见下文）

**项目配置**
- CodeMirror 编辑器查看/编辑项目 `mise.toml`（TOML 高亮，明暗自适应）
- 内置技术栈预设 + 自定义用户预设；一键安装全套环境
- 整机环境绑定：把 brew/scoop 运行时以 PATH 方式写入项目 mise.toml（零链接、零侵入）

**设置**
- 深浅色主题（跟随系统/浅色/深色）、开机自启、应用内检查更新与自动重启
- 彩蛋 🥚：版本号连点 6 次解锁

## 托管接入（整机环境 → mise）

安装交给系统包管理器，项目绑定交给 mise，Z.Env 负责两者之间的托管接入：

| 策略 | 适用来源 | 机制 |
|---|---|---|
| 直连接管 | nvm / pyenv / asdf / sdkman / volta 等用户级目录 | `mise link` 直连 + 注册校验 |
| 托管接管 | brew | 软链指向应用农场 `~/.zenv/managed/`，brew 升级后**自动重连**到新版本 |
| PATH 绑定 | brew / scoop | 项目 `mise.toml` 写入 `[env] _.path`，零链接零侵入 |

- openjdk 系列自动修正到真实 Home 布局（`libexec/openjdk.jdk/Contents/Home`）
- 所有接入动作带 `mise ls` 注册校验与失败回滚
- 每次扫描对账自愈：目标目录被包管理器升级/删除时自动重连或显式移除，不再有悬空链接

## 设计

- **Tailwind CSS v4 + shadcn 设计令牌**（zinc 中性色 + 蓝色主色，明暗两套）
- **lucide-react** 图标；macOS 液态玻璃侧边栏 + Windows 亚克力
- Radix Dialog 弹窗（焦点圈定 / ESC / 无障碍），CodeMirror 主题自适应

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
npm test               # vitest 单测
npm run check:version  # 校验三处版本号一致
```

### 环境要求

- [Node.js](https://nodejs.org/)（推荐用 [mise](https://mise.jdx.dev) 管理）
- [Rust](https://www.rust-lang.org/) toolchain
- Linux 需系统依赖：`libwebkit2gtk-4.1-dev`、`libxdo-dev`、`libssl-dev` 等

## 代码约定

- 注释统一使用中文；单行／行内用 `//`，模块级说明用 `///`（Rust）。
- Rust 按 `rustfmt` 规范排版；前端经 ESLint（typescript-eslint）与 Prettier 约束。
- 后端按域分模块：`sources/`（版本源）、`managed.rs`（托管接入）、`cache.rs`/`syscache.rs`
  （双层缓存）、`net.rs`（网络）、`error.rs`（错误模型）。
- 前端基础组件在 `src/components/ui/`，共享目录数据在 `src/data/`。

## CI / 发布

- **ci.yml**：版本一致性校验 → 前端 lint / test / build + npm audit（high 及以上阻断）；
  后端 fmt + clippy（-D warnings）+ test + build。
- **release.yml**：推送 `v*` 标签触发，tauri-action 在 macOS / Windows / Linux 三平台打包，
  并生成应用内更新所需的 `latest.json`。

### 应用内更新配置

更新包使用 minisign 签名，公钥位于 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。
发布前需在 GitHub Secrets 配置：

- `TAURI_SIGNING_PRIVATE_KEY`：签名私钥内容（本地生成于 `~/.tauri/zenv.key`，无密码）
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：留空

```bash
git tag v0.7.0 && git push origin v0.7.0   # 触发自动打包
```

## 相关链接

- [GitHub: 3yearsZhuang](https://github.com/3yearsZhuang)
- [mise 官方文档](https://mise.jdx.dev) · [mise 安装指南](https://mise.jdx.dev/getting-started.html)
- [GitHub: jdx/mise](https://github.com/jdx/mise)

## 开源协议

本项目基于 **Creative Commons Attribution-NonCommercial-ShareAlike 4.0 (CC BY-NC-SA 4.0)** 开源，详见 [LICENSE](./LICENSE)。

> 使用约定：需署名、非商业用途、相同方式共享。完整法律文本见
> [CC BY-NC-SA 4.0 法律文本](https://creativecommons.org/licenses/by-nc-sa/4.0/legalcode)。

部分运行时图标准由 [devicon](https://devicon.dev) / [Simple Icons](https://simpleicons.org) / [iconify](https://iconify.design) 提供，版权归各自作者所有。
