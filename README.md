# Z.Env

跨平台 **mise** 图形化管理器 —— 用可视化界面统合开发环境的版本管理、系统资源监控与项目环境预设。

> 原名 EnvForge，现更名为 **Z.Env**。

## ✨ 功能

**系统概览**
- CPU / 内存 / 磁盘 / 网络 / 电量 / GPU 实时占用（正方形亚克力卡片）
- 历史使用率曲线、网络上下行双环
- 硬件型号（CPU、内存、磁盘、GPU）首次采样时读取并缓存
- 顶部环境横幅：系统与各包管理器（brew/winget/apt/pacman）版本一览
- 刷新间隔自选（0.1 / 0.5 / 2 / 5 秒 / 关闭）

**支持列表**
- 全量软件/运行时，与「运行时工具」共用同一套分类标签（TOOL_CATS）
- mise 源可安装的运行时 + 仅官方下载的软件（附安装弹窗）
- 系统包管理器 **原生安装**：brew / winget / apt / pacman，点击即执行并流式显示进度
- 已安装状态来自真实探测（macOS 走 `brew list`），跨启动保持准确

**运行时工具**
- 本机环境一览：可由 mise 安装，或已通过其他渠道（nvm/pyenv/asdf、**brew 等**）托管
- 聚合展示所有已装版本（含其他渠道版本，带渠道标签）
- 一键安装、卸载、切换全局版本，实时进度条 + 可展开 CLI 输出
- **多远程源**：mise 官方、ASDF 插件、GitHub Releases、官方生态源（Node/Go/Python/Java）
- 其他渠道已装环境可**原生卸载**（brew/winget/apt/pacman，二次确认）
- 官方运行时图标 + 分类筛选 + 紧凑模式（支持 200+ 运行时）

**项目配置**
- 选择项目目录，一键生成对应语言环境的 `mise.toml`
- 内置常用技术栈预设，支持自定义与保存为用户预设

**彩蛋 🥚**
- 在「设置 → 版本」号上连续点击 6 次解锁
- 含「运行时覆盖自检 / 日志系统 / 打赏支持」

**其他**
- 深浅色主题（自动 / 浅色 / 深色）、多档响应式、毛玻璃 / 亚克力质感

## 技术栈

- [Tauri 2](https://v2.tauri.app/) + [Rust](https://www.rust-lang.org/)
- [React 19](https://react.dev/) + [TypeScript](https://www.typescriptlang.org/) + [Vite](https://vitejs.dev/)
- [mise](https://mise.jdx.dev) · ASDF · GitHub Releases · 官方生态源

## 开发

```bash
npm install        # 安装前端依赖
npm run tauri dev  # 启动开发模式（拉起桌面窗口）
npm run build      # 仅构建前端（tsc + vite）
```

### 环境要求

- [Node.js](https://nodejs.org/)（推荐用 [mise](https://mise.jdx.dev) 管理）
- [Rust](https://www.rust-lang.org/) toolchain
- Linux 需系统依赖：`libwebkit2gtk-4.1-dev`、`libxdo-dev`、`libssl-dev` 等

## 代码约定

- 注释统一使用中文；单行／行内用 `//`，模块级说明用 `///`（Rust）或简洁 `//` 头注释（前端）。
- Rust 代码按 `rustfmt` 规范排版（CI 会 `cargo fmt --check`）。
- 前端类型由 `tsc` 校验；生产构建即类型检查。

## 持续集成 / 自动打包

CI 与发布流水线见 `.github/workflows/`：

- **ci.yml**：前端类型检查 + 构建 + npm 依赖安全审计；后端 `cargo fmt` + `clippy -D warnings` + `cargo build`。
- **release.yml**：推送 `v*` 标签或手动触发，用 `tauri-action` 在 **macOS / Windows / Linux** 三平台自动打包（dmg / msi-nsis / appimage+deb）并发布 GitHub Release。

```bash
git tag v0.6.7 && git push origin v0.6.7   # 触发自动打包
```

## 相关链接

- [GitHub: 3yearsZhuang](https://github.com/3yearsZhuang)
- [mise 官方文档](https://mise.jdx.dev)
- [mise 安装指南](https://mise.jdx.dev/getting-started.html)
- [GitHub: jdx/mise](https://github.com/jdx/mise)

## 开源协议

本项目基于 **Creative Commons Attribution-NonCommercial-ShareAlike 4.0 (CC BY-NC-SA 4.0)** 开源，详见 [LICENSE](./LICENSE)。

> 使用约定：需署名、非商业用途、相同方式共享。完整法律文本见
> [CC BY-NC-SA 4.0 法律文本](https://creativecommons.org/licenses/by-nc-sa/4.0/legalcode)。

部分运行时图标准由 [devicon](https://devicon.dev) / [Simple Icons](https://simpleicons.org) / [iconify](https://iconify.design) 提供，版权归各自作者所有。