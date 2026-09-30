# Z.Env 迭代计划

> 基于 2026-09-29 全库 review 制定（前端 React 19 + TS，后端 Rust/Tauri 2，共约 5800 行）。
> 状态标记：✅ 已完成 · 🚧 进行中 · ⬜ 待办。完成一项就更新状态，避免计划腐化。

## P0 — 正确性修复（已收口）

### ✅ 1. GitHub 版本源只取第一页，大量版本被静默截断
- **问题**：`github_tag_versions` 与 `list_remote_versions_github` 均只请求 `tags?per_page=100&page=1`。实测 rust-lang/rust 有 163 个 tag，page 2 的 1.34.1 等版本用户永远看不到；php/php-src（700+ tag）更严重。
- **修复**：翻页拉取（每页 100，最多 3 页，不足一页提前终止）；后续页失败时退回已取到的结果而非整体报错。
- **改动**：`src-tauri/src/sources/github.rs`（原 mise.rs）重写 `github_tag_versions`；`list_remote_versions_github` 收敛为统一入口（node/npm/bun/deno/terraform 复用同一逻辑，顺带修复 bun 的 `bun-v*` tag 此前全被过滤导致列表为空的问题）。

### ✅ 2. 版本列表零缓存，极易触发 GitHub 限流
- **问题**：未认证 GitHub API 限额 60 次/小时，每次展开版本列表都重新请求；翻页后单工具最多 3 次请求，连续看几个工具即 403。
- **修复**：双层 TTL 缓存（30 分钟，P1 升级为内存 + 磁盘），按 `gh-tags:<repo>` 键缓存，GitHub tag 源与官方生态源全部命中；**错误结果不缓存**（瞬时网络失败不会固化 30 分钟）。
- **改动**：`src-tauri/src/cache.rs`。

### ✅ 3. 错误处理靠猜：`contains("\"message\"")` 判定限流
- **问题**：旧代码用 `text.contains("\"message\"")` 猜测出错；抓取层完全不看 HTTP 状态码——限流 403 的 JSON 对象被当数据解析后报"返回格式异常"，用户无法得知真实原因。
- **修复**：拿到真实 HTTP 状态码（P1 起由 ureq 提供，替代 curl `-w` 技巧）；HTTP ≥ 400 时解析服务端 `message`，403 提示限流（60 次/小时）、404 提示仓库不存在；网络层失败（超时/DNS）单独报错。
- **改动**：`src-tauri/src/net.rs`。
- **附带修复**：`dedup()` 原来排在 `sort_by()` 之前，只对"相邻重复"生效等于无效；已改为先排序后去重（`finalize_versions`），翻页合并的重复版本真正被清除。

### ✅ 4. CI / release 跑在被淘汰的 ubuntu-20.04 runner
- **问题**：GitHub 已于 2025-04 移除 ubuntu-20.04 runner，rust 检查 job 与 release 的 Linux 打包现在无法启动。
- **修复**：`ci.yml` rust job → `ubuntu-24.04`；`release.yml` Linux matrix → `ubuntu-22.04`（刻意不用 24.04：AppImage 产物依赖构建机 glibc，22.04 兼容更老的发行版）。

### ✅ 5. 顺带：新版 clippy（1.98）存量警告清零
- CI 升级后用最新 stable clippy `-D warnings` 会挂：6 处 `sort_by` 可换 `sort_by_key`、1 处 `iter().cloned().filter()` 顺序、1 处 `bool::then` 应为 `then_some`。已全部修复。

### ✅ 6. 顺带：为 P1 测试体系开了头（仓库此前 0 测试）
- 纯函数测试随 P1 一并成形，见下。

### P0 验证记录
- 实测 GitHub API：rust-lang/rust page1=100 / page2=63，确认截断存在且翻页生效。
- **本地环境备忘**：本机曾出现 tauri-build 读取"TRAE 沙盒路径"下权限文件失败——根因是 8 月在 TRAE 环境构建时，`target/debug/build/tauri-*/out/tauri-core-*-permission-files` 状态文件记录了当时的绝对路径。若复发，执行 `rm -rf src-tauri/target/debug/build/tauri-* src-tauri/target/debug/.fingerprint/tauri-*` 即可。

---

## P1 — 工程质量（本轮已落地）

### ✅ 7. 测试 0 → 1
- **后端 8 个 Rust 测试**（`cargo test`）：`clean_tag_version` 常见 tag 形态（v 前缀/下划线/品牌前缀/非版本 tag）、`parse_tag_page` 翻页解析、`finalize_versions` 排序去重（1.10.0 > 1.9.0 且去重生效）、`normalize_version` 数值分段、内存/磁盘缓存 roundtrip（含损坏 JSON 视为未命中）、`scan_versions` 目录扫描（tempfile fixture：有 bin/ 识别、隐藏目录与无 bin/ 跳过）、项目配置读写 roundtrip（目录写 mise.toml、文件直读、缺失报错）。
- **前端 6 个 vitest**：`errorMessage` 三态（字符串/Error/其他类型）+ catalog 三不变量（categoryOf 兜底"其他"、KNOWN_RUNTIMES 与 SOFTWARE 不重叠、标签列表含兜底项）。
- **CI 接入**：ci.yml frontend job 加 `npm run lint` + `npm test`；rust job 加 `cargo test`。

### ✅ 8. 错误模型升级：`Result<_, String>` → thiserror 枚举
- `error.rs` 定义 `AppError`（MiseNotInstalled / Network / Http{status,message} / Parse / Io / Unsupported / Other）。
- Tauri command 边界经 `From<AppError> for String` 转换，**前端 wire 格式保持不变**（避免一次性大改前端）。
- 用户可感知改进：mise 未安装时（进程 spawn NotFound）返回"mise 未安装或未在 PATH 中找到，请先安装 mise"引导文案，而非原始系统错误。
- **未做**：`env.rs` 的系统包管理命令仍为 String，随错误模型二期迁移。

### ✅ 9. 用 ureq 替换外部 curl + 磁盘级缓存
- **动机**：Windows 10 以下无自带 curl；shelling out 拿不到响应头/状态码细节，也难做重试。
- `net.rs`：ureq 2（rustls，无系统 OpenSSL 依赖）、共享 Agent、10s 超时；`http_get` / `get_json`。
- `go_proxy_versions` / `python_ftp_versions` 纯文本抓取一并迁移，并补上状态码检查（此前 404 页面会被当空数据静默处理）。
- `cache.rs` 双层缓存：内存 + 磁盘（unix `~/.cache/zenv`、Windows `%USERPROFILE%\AppData\Local\zenv`，TTL 同 30 分钟），**应用重启后缓存不再清零**；官方生态源（node/go/python/java）也纳入缓存。

### ✅ 10. 模块拆分
- **后端**：`mise.rs` 1199 → 837 行，拆为 `sources/{mod,github,official,asdf}.rs` + `cache.rs` + `net.rs` + `error.rs`；lib.rs 声明 7 个模块。
- **前端数据层**：硬编码的图标映射、工具清单、分类标签、软件目录抽到 `src/data/catalog.ts`（ToolsView 789 → 495 行）；SoftwareView / EasterEgg 改从 catalog 导入。
- **未做（延后）**：ToolsView 组件级拆分（需跑起应用做视觉验证）、lib.rs 22 个 command 按域分组（现有顺序已按域排列，收益低）。

### ✅ 11. 前端 lint 基建
- ESLint 9 flat config + typescript-eslint 8 + browser globals（`eslint.config.js`）；Prettier 3（`npm run format`，**未设 CI 门禁**——避免一次性全库重排版，作为独立提交后续处理）。
- CI frontend job 接入 `npm run lint`。
- 顺带修复存量 lint 错误 1 处（SoftwareView `un && un()` → `un?.()`，消除 no-unused-expressions）。

### P1 验证记录
- 后端：`cargo fmt --check` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo test` **8 passed** ✅
- 前端：`tsc --noEmit` ✅ · `eslint` 0 错误 ✅ · `vitest` **6 passed** ✅ · `npm run build` ✅（43 模块）

---

### ✅ 追加（实机验证期修复）：命令全部转后台线程 + 系统包探测 TTL 缓存
- **问题**（实机验证发现）：Tauri 同步 command 默认在主线程执行，`brew list` 等子进程调用（实测数十秒）会冻结整个窗口并饿死 `system_stats` 轮询——表现为打开「支持列表/运行时工具」明显卡顿、本机信息刷不出来。
- **修复 1**：`lib.rs` 全部 21 个同步命令标记 `#[tauri::command(async)]` 转后台线程执行，函数签名与前端调用零改动。
- **修复 2**：新增 `syscache.rs`——brew/winget/apt/pacman 探测结果进程内 TTL 缓存（10 分钟），安装/卸载后主动失效该管理器缓存；TTL 内重复打开页面"已安装"徽标秒出。
- 验证：clippy 清零 · `cargo test` 10 passed · fmt 干净 · dev 热重编译（13.20s）并重启生效。

---

### ✅ 追加：UI 设计系统统一 — 阶段一（令牌化换肤）
- **采纳**：Tailwind CSS v4（@tailwindcss/vite 插件）+ shadcn 设计令牌体系（zinc 中性色 + 蓝色主色，明/暗两套）+ lucide-react 图标。
- **改造**：新增 `src/index.css` 集中定义设计令牌；`styles.css`（2200+ 行既有样式）的旧变量全部映射到新令牌——全部视图**零 JSX 改动**自动跟随新配色；App 壳的 emoji 图标（◉⌂▤▰⚙☀☾）替换为 lucide 图标；auto 主题由 matchMedia 实时解析并写入 `<html>.dark`。
- **消除**：蓝/紫渐变混杂、卡片渐变底、不一致的徽标配色 → 单一主色 + 语义色令牌。
- **验证**：build/eslint 通过；浏览器实测深浅两套主题（首页 + 支持列表页全量渲染 + 主题切换）。
- **阶段二（⬜）**：各视图与弹窗逐个迁移到 shadcn/ui 真组件（Dialog 无障碍优先），复用本阶段令牌。

---

### ✅ 追加：托管接入层（整机环境愿景落地）
- **产品定位升级**：安装走 brew/winget 等系统包管理器，项目绑定走 mise，Z.Env 做中间的**托管接入层**——把「一次性软链」升级为「应用维护、可自愈的接入」。
- **三策略**（`src-tauri/src/managed.rs`）：A 直连接管（nvm/pyenv/asdf 等用户级稳定目录）；B 托管接管（brew：软链指向应用农场 `~/.zenv/managed/<tool>/<version>`，mise 只认农场路径；openjdk 系列自动修正到真实 Home 布局）；C PATH 绑定（ ProjectsView「整机环境绑定」把 brew opt / scoop current 等稳定路径写入项目 mise.toml 的 `[env] _.path`，零链接零侵入）。
- **对账自愈**：运行时页每次刷新触发 reconcile——目标目录被 brew 升级/删除时，自动重连到同来源最新版本（横幅提示「已自动重连」）或显式移除接入，悬空链接不再是静默故障。
- **接入校验回滚**：所有 link 后跑 `mise ls` 验证注册结果，失败自动回滚并给出插件级原因。
- **UI**：InstallDialog 接管列表放开 brew（走托管模式，按钮文案区分）；解除接管同步清理农场；ToolsView 对账横幅；ProjectsView 绑定弹窗（Radix Dialog）。
- 验证：clippy 清零 · `cargo test` **13 passed**（含农场清单 roundtrip / openjdk 布局修正 / system 拒绝三类新测试）· build/lint 全绿 · dev 重编译重启生效。

---

## P2 — 功能与打磨（本轮已收口）

### ✅ 12. 应用内自动更新
- `tauri-plugin-updater` 全套：Rust 插件注册 + capabilities 权限 + `tauri.conf.json` 配置端点（GitHub Releases latest.json）与签名公钥；`release.yml` 注入 `TAURI_SIGNING_PRIVATE_KEY*`。
- 设置页「应用更新」：检查更新 → 下载（带进度%）→ 自动重启（plugin-process）。
- **需要你完成**：① 签名私钥已生成于本机 `~/.tauri/zenv.key`（无密码），把私钥内容配置为 GitHub Secrets 的 `TAURI_SIGNING_PRIVATE_KEY`（`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 留空）；② latest.json 在首个带 updater 的版本发布后才会存在。

### ✅ 13. mise.toml 编辑体验
- ProjectsView 接入 CodeMirror（@uiw/react-codemirror + TOML 语法高亮），主题跟随应用明暗切换，替代纯文本 textarea。

### ✅ 14. 安全与配置收紧
- CSP 收紧为 `default-src 'self'` 白名单（connect-src 保留 ipc 与 dev HMR，style-src 保留 inline 供 React 内联样式）。
- `npm audit` 移除 `|| true`：high 及以上漏洞阻断 CI。
- 新增 `scripts/check-version.mjs`（`npm run check:version`）校验三处版本一致性，CI 前端 job 首步执行；package.json name 更名 `zenv`（Cargo 包名保持——仅影响二进制文件名，改名风险大于收益）。

### ⏸ 15. i18n — 暂缓（按计划自身标准处置）
- 计划判定标准为"按受众需求决定"；当前产品面向中文用户，全量抽词改动大而收益为零。面向国际用户时再引入 react-i18next。

### ✅ 16. 桌面应用标配 + 存量 quirk
- 窗口状态记忆（plugin-window-state）、系统托盘（左键显示/聚焦主窗口，右键菜单：打开/退出）、开机自启（plugin-autostart + 设置页开关）。
- quirk 修复：`clean_tag_version` 拒绝数字前前缀超过 6 字符的 tag（rust-lang/rust 的 `release-0.7` 不再被误判为 `0.7`），附单元测试。
- ToolsView 组件级拆分：**保持现状**（令牌化后样式已统一，纯拆分无视觉收益而回归风险为负），后续按需进行。

### P2 验证记录
- Rust：clippy `-D warnings` 清零 · `cargo test` 10 passed · fmt 干净；dev 热重编译（39.11s，444 构建单元，含 4 个新插件）并重启。
- 前端：build 通过（CodeMirror 后 bundle >500kB，代码分割列为后续优化项）· eslint 清零 · vitest 6 passed · `check:version` 通过。
- 浏览器实测：Radix Dialog 渠道弹窗正常（遮罩/居中/标题/关闭）、明暗两套主题正常。

---

> 2026-10-01 规划：P0–P2 已收口，以下为后续三个阶段，按序号落地（P5 在 P4 后择一推进）。

## P3 — 发布闭环（最高优先：先证明管线，再打磨功能）

### ✅ 17. 签名 Secret + 首个 tag v0.7.0 → 三平台打包
- **Secret**：`TAURI_SIGNING_PRIVATE_KEY` 已注入本地 `~/.tauri/zenv.key` 内容；密码 Secret 留空（未配置的 Secret 在 Actions 中解析为空串，密钥本身无密码）。
- **版本**：三处 0.6.7 → 0.7.0（`check:version` 校验），推送 tag 触发 release.yml。
- **实战踩坑（三连，均已修复）**：
  1. **Tauri npm 包与 Rust crate 版本错位直接 fail**——`@tauri-apps/api` 2.12.0 vs Rust `tauri` 2.11.5 等 4 对，npm 锁文件先行升级而 Cargo.lock 停留旧版。修复：双侧对齐同 minor（tauri 2.12.0 / process 2.4.0 / autostart 2.6.0 / updater 2.13.1，连带 window-vibrancy 0.8.1、wry 0.57）。后续升级 Tauri 必须**两侧同步**。
  2. **仓库 Actions 默认 workflow 权限为 read**——GITHUB_TOKEN 无权 create-release，报 "Resource not accessible by integration"（八月 v0.6.6/0.6.7 能建草稿，是权限块加入 P2 之前、且当时默认值为 write）。修复：仓库设置默认 workflow 权限改回 write。
  3. **Tauri 2 默认不产出更新器工件**——需在 `tauri.conf.json` 显式 `bundle.createUpdaterArtifacts: true`，否则无 `.app.tar.gz/.sig`，tauri-action 报 "Signature not found for the updater JSON" 跳过 `latest.json`，应用内更新无元数据。
- **验证记录**（run 36753147265，completed success）：草稿 Release 共 14 个资产——macOS aarch64 dmg + `.app.tar.gz(+.sig)`、Windows nsis/msi（各带 `.sig`）、Linux AppImage/deb/rpm（各带 `.sig`）、`latest.json` 含 9 个平台条目（darwin-aarch64 / windows-x86_64 / linux-x86_64 及安装器变体），签名全部就位。CI（含新 prettier 门禁）在 main 全绿。
- **遗留**：① release.yml macOS matrix 安装了 x86_64 target 但构建未使用，产物只有 aarch64——Intel Mac 覆盖为既有缺口，后续加 `--target` 或 universal；② 应用内更新端到端待 0.7.1 发布时验证；③ macOS arm64 runner 曾排队 24h（v0.6.7 run 超时取消），发布窗口留意；④ 草稿 Release 待用户审阅后手动 Publish。

## P4 — 工程欠账（18 → 19 → 20，21 按需）

### ✅ 18. env.rs 错误模型二期
- `detect_system_installed` / `detect_system_versions` / `uninstall_system_package` / `install_system_package` 及私有 `run_limited`：`Result<_, String>` → `Result<_, AppError>`。
- 变体映射：不支持的包管理器 → `Unsupported`、spawn/等待失败 → `Io`、执行失败/超时 → `Other`；command 边界仍经 `From<AppError> for String`，前端 wire 格式不变。
- 新增 Unsupported 映射单测。

### ✅ 19. Prettier 门禁
- 全库格式化独立提交（14 文件）先行，随后 `format:check` 脚本接入 CI frontend job。

### ✅ 20. bundle 代码分割
- CodeMirror（@uiw/react-codemirror + StreamLanguage + TOML mode）抽为 `TomlEditor.tsx` 独立模块，React.lazy + Suspense 懒加载，整体落入异步 chunk。
- **验证**：主 bundle 741kB → 328kB（-55%），编辑器 chunk 396kB，生产构建不再触发 500kB 告警。

### 21. shadcn/ui 阶段二（⬜ 按需）
- 各视图与弹窗逐个迁移到真组件（Dialog 无障碍优先）。令牌化后纯迁移视觉收益有限，穿插在功能迭代里做，不单独立项推进。

## P5 — 产品纵深（已选定先做 22）

### ✅ 22. 环境体检 doctor（v1 已落地）
- **后端 `doctor.rs`** 五项只读检查：mise 可用性（缺失记 Fail）、PATH 重复条目、PATH 失效目录、shell 集成（activate 钩子或 mise shims 二选一，pyenv 同名结构不误判；Windows 跳过）、托管接入健康度（指引到运行时页对账自愈）。只报告不动状态，每项给修复建议。
- **前端**：设置页「环境体检」面板，语义色圆点分级（success/warning/destructive）。
- **测试**：PATH 解析/重复/失效、shell 集成判定 4 个纯函数单测，后端合计 18 passed。
- **后续**：实机视觉验证；可修复项的一键修复动作（v1 有意不做）；磁盘体积类检查（`~/.zenv` 农场大小）。
- **备选**（22 之后按反馈排序）：项目发现（扫描常用目录识别技术栈并提示补齐）、本地开发服务管理（brew services 等）、缓存治理（brew/mise/各语言缓存体积与清理）。
- **i18n 维持暂缓**：面向国际用户时再引入 react-i18next（沿用 P2 判定）。

---

> 2026-10-01 追加：**定位自检**——对照"整机环境管理中心"，现状=「跨平台 mise 图形化管理器 + 托管接入层（起步）」。
> 强项：运行时管理、软件安装、项目级配置已中心化；缺位：整机环境变量、本地服务、项目发现、缓存治理、
> shell/PATH 只能看不能修、无操作审计/迁移。**判定标准（裸机测试）**：新机装好 Z.Env + mise 后不打开终端，
> 能否装齐软件、配好 shell 接入、跑起项目环境——当前答案为否。以下 P6 最小闭环即冲此判定。

## P6 — 整机环境管理中心：最小闭环（23 → 24 → 25）

### ✅ 23. 整机环境变量中心
- **全局 env 管理**：全局 mise config（`MISE_GLOBAL_CONFIG_FILE` > `MISE_CONFIG_DIR` > `~/.config/mise/config.toml`，
  Windows `%APPDATA%\mise\config.toml`）的 `[env]` 段增删改；**文本手术式编辑**——只重写 `[env]` 段内目标行，
  保留用户其余内容与注释，零新依赖（不引 toml crate）；表/数组等复杂值只读展示并提示手动编辑。
- **冲突检测**：系统/用户级 env（进程环境）与全局 mise env 同名且值不同者列出（PATH 除外），说明生效条件取决于 shell 接入。
- **入口**：新视图「环境变量」。段解析/编辑为纯函数，单测覆盖。
- **验证**：`env_center.rs` 9 个纯函数单测（段界/转义/替换/追加段/复杂值跨行/roundtrip）；后端 34 passed；
  浏览器实测新视图渲染（表单、空态、错误路径、系统 env 搜索面板）。

### ✅ 24. doctor 接修复动作（化验单 → 处方）
- `DoctorCheck` 增加 `fixable`；`doctor_fix` command：
  - `managed-health` → 复用 `managed::reconcile()` 对账自愈，返回事件摘要；
  - `shell-integration` → 按 `$SHELL` 把 `mise activate` 行**追加**到对应 rc（zsh/bash/fish；写前复检，绝不改写既有内容）；
  - 其余项返回"暂不支持自动修复"并保留 hint（PATH 类问题不动用户配置）。
- **前端**：warn/fail 且 fixable 项显示「一键修复」，成功后自动重跑体检刷新结果。
- **验证**：fix 路由拒绝只读项、shell 映射（zsh/bash/fish/未知）2 个新单测；后端 29 passed（该轮）。

### ✅ 25. 项目发现
- **后端 `discover.rs`**：扫描常用根目录（存在者：~/Documents ~/Desktop ~/Projects ~/Code ~/code ~/Dev ~/dev ~/work ~/repos，各深 3 层；家目录本身 1 层），
  跳过隐藏目录与 node_modules/target/vendor 等重目录，目录预算 4000、结果上限 200；
  识别 `.git` 与技术栈指纹（package.json/.nvmrc/pyproject.toml/requirements.txt/.python-version/go.mod/Gemfile/composer.json/pom.xml/build.gradle/.tool-versions）；
  `mise.toml` 解析 `[tools]` 键（含带引号键）、`.tool-versions` 解析工具名。
- **输出**：名称/路径/是否已有 mise.toml/工具清单/**缺失工具**（对照 `mise ls` 已装）。
- **前端**：ProjectsView 顶部「发现的项目」区（进页自动扫描 + 重新扫描），点击载入既有编辑器流程，缺失工具走既有"保存并一键安装"。
- **验证**：5 个单测（指纹识别、node_modules/隐藏目录跳过、预算耗尽、引号键、缺失对比）；后端 34 passed；
  浏览器实测发现区渲染（计数、重新扫描、扫描失败错误路径）。
- **达成判定**：23–25 落地后裸机测试从"否"变为"基本是"（装软件 ✓ / shell 接入 ✓ 一键 / env ✓ 中心化 /
  项目 ✓ 自动发现）；本地服务与迁移仍在第二、三梯队，全部完成后 tagline 升级为"整机环境管理中心"。

### ⬜ 第二梯队（25 之后按反馈排序）
- 本地开发服务管理（brew services / systemd user units / Windows 服务只读）；开发缓存治理（体积展示 + 一键清理）；操作历史面板。

### ⬜ 第三梯队（称号杀手锏）
- 环境快照与迁移：导出整机环境清单（软件 + mise 配置 + 全局 env），换机一键重建。
