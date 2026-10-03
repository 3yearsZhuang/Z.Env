# Z.Env 开发志：P0–P8（2026-09-29 → 2026-10-02）

> 迭代计划 P0–P8 阶段的完整执行档案（动因 / 备选取舍 / 踩坑 / 验证数据）。
> 2026-10-04 起活计划拆出至 [plan.md](../plan.md)，本文件只增不改；
> 各处文档引用的条目编号（1–37）均指本文条目。
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

### ✅ 26. 本地开发服务管理
- **范围**：brew services（macOS / Linuxbrew，`brew services list --json` 防御式解析，兼容 running(bool) 与旧版
  state 字段，start/stop/restart）；systemd 用户级服务（Linux，`list-unit-files` 仅取 `.service`，单元名白名单校验
  防注入）；Windows 服务暂缺实机，列表处显式说明。
- **入口**：新视图「服务与缓存」。解析为纯函数单测（5 个）。

### ✅ 27. 开发缓存治理
- **展示**：brew --cache / mise / npm / yarn / pip / uv / GOCACHE / cargo registry 八类，递归计体积带 20 万条目
  预算，超限返回下限值并标记 ≥；符号链接不跟随。
- **清理**：只用官方命令（`brew cleanup -s`、`mise cache clear`、`npm cache clean --force`、`yarn cache clean`、
  `pip cache purge`、`uv cache clean`、`go clean -cache`，10 分钟超时）；cargo registry 无标准命令只展示。
- **验证**：dir_size 预算/符号链接/清理路由 3 个单测。

### ✅ 28. 操作历史
- **记录**：`~/.zenv/history.jsonl` 追加式 JSONL（unix 秒 + kind + detail），覆盖安装/卸载（mise 与系统包）、
  切换、托管接入/解除、对账自愈、体检修复、env 写入/删除、服务操作、缓存清理、快照导出/重建；
  成功与失败都留痕（失败详情随行），记录失败静默忽略。
- **展示**：设置页「操作历史」面板，中文类别标签 + 时间，最新在前上限 200。
- **验证**：JSONL 解析（损坏行跳过）/追加/反转截断 3 个单测。

### ✅ 29. 环境快照与迁移（第三梯队）
- **导出**：mise 已激活工具 + 全局 [env]（simple 条目）+ brew 已装清单 → 单个 TOML（引号键 + 基本字符串，
  复用 env_center 转义），保存对话框选路径。
- **重建（v1 安全边界）**：全局 env 直接写入（复用 env_center）；mise 工具逐个 `mise install`（失败不阻断、
  逐条报告）；brew 清单生成 `~/.zenv/Brewfile.snapshot` **不自动执行**，提示 `brew bundle --file` 一条命令。
  执行前前端二次确认，结果多行报告 + 操作历史留痕。
- **实现要点**：快照解析器逐字符扫描基本字符串（`trim_matches('"')` 会误剥值内转义引号——单测抓出后修复）。
- **验证**：段解析/转义还原 roundtrip/Brewfile 行 3 个单测；后端合计 48 passed。
- **达成判定**：23–29 全部落地——新机不打开终端：装软件（brew bundle 一条命令）✓ shell 接入（24 一键）✓
  env（23 中心化 + 快照还原）✓ 项目环境（25 发现 + 一键安装）✓。tagline 已按约定升级为"整机环境管理中心"。
  Windows 服务列表与 Intel Mac 产物为两个已知平台缺口（17 遗留）。

---

> 2026-10-01 追加：信息架构评估——「环境变量」与「运行时工具」同属环境配置域（全局 [env] 与 [tools]
> 本是同一份全局 mise config 的两段；体检与快照也都把二者并列使用），不应分设两个顶级页面。

## P7 — 信息架构：环境页合并（30）

### ✅ 30. 运行时工具 + 环境变量 → 合并为「环境」页
- **结构**：侧边栏 7 → 6 项（系统概览 / 支持列表 / **环境** / 项目配置 / 服务与缓存 / 设置）。
  页内纵向分区：运行时网格（原 ToolsView 原位不动）→ 全局环境变量区（原 EnvView 面板一）→
  系统/用户级变量（**默认折叠**，点开展开 + 搜索）。页面命名 `EnvironmentView.tsx`。
- **摘要条**：页面顶部 env 概况（N 个变量 · M 个同名冲突，冲突走黄色警示），点击锚点滚动到变量区——
  冲突检测从"进 env 页才看到"提升为"进环境页即看到"。
- **实现**：EnvView 两面板抽为 `EnvPanel.tsx`（快照与 reload 由页面级持有传入，单一数据源）；
  纯前端信息架构调整，34 个后端 command 零改动；侧栏副标题与导航图标同步（Layers）。
- **备选已否**：左右分栏主从布局（两侧重内容撑不住宽度）；"全局配置卡片 + CodeMirror 弹窗编辑"
  （最贴 mise 本质但表单易用性降级，留作后续增强）。
- **验证**：lint/test/build 全绿；浏览器实测合并页（导航 6 项、运行时网格、变量区表单、
  系统 env 折叠态、摘要条无后端时优雅隐藏）。

### ✅ 31. 环境页深度整合：只展示已安装，运行时并入全局环境区块
- **不再展示未安装**：移除全量候选网格与分类 chips/紧凑模式——本页只列 mise 已装 + 其他渠道已托管；
  新环境安装入口归位「支持列表」（空态引导文案指路），已装工具保留「+ 安装其他版本」（InstallDialog）。
- **已安装运行时显示在「全局环境变量」区块上方**：顶部摘要条升级为全局环境概况
  （X 个运行时 · Y 个变量 · M 个冲突，点击滚动到变量区）。
- **增强「设为全局」**：单版本且未设全局的工具，卡片头部直接给「设为全局」按钮（免展开，最高频场景）；
  展开后的逐版本按钮保持。
- **增强「一键接管」**：有真实路径的外部渠道来源（nvm/pyenv/asdf/brew 等；winget/apt/pacman 系统版本
  path 为空接不了，不显示按钮）直接在卡片/版本行提供「一键接管」，调 managedAdopt 自动选策略并校验回滚，
  免开 InstallDialog；唯一来源时上浮到卡片头部。
- **实现**：EnvironmentView 重写（-紧凑/筛选/渠道弹窗 ~90 行，+快捷能力/空态引导）；EnvPanel 不变；
  纯前端调整，后端零改动。
- **验证**：lint/test/build 全绿；浏览器实测（空态引导、变量区、折叠、无后端降级）；
  已装卡片与快捷按钮需真机数据，待用户桌面端验收。

### ✅ 32. 环境页功能对齐定位：环境健康区常驻（体检从设置页迁入）
- **动因**：合并若只动显示不改功能，"管理中心"仍是空话——最关键缺口是"装好 ≠ 生效"：
  shell 未接入 mise 时，运行时与全局 env 在终端里统统无效，而体检/修复能力原本埋在设置页。
- **改动**：doctor 巡检抽为 `DoctorPanel` 组件（chips 一行总览 + 异常项详情行 + fixable 就地修复，
  挂载即自动巡检、修复后自动重跑），挂到环境页摘要条之下、运行时区之上；
  设置页移除「环境体检」面板（应用更新/环境快照/操作历史保留）。
- **效果**：环境页现在是完整的职能闭环——健康（巡检+修复）→ 运行时（设为全局/接管/卸载）→
  变量（增删改/冲突）；"这台机器的环境活没活、怎么救活"在一页内完成。
- **验证**：lint/test/build 全绿；浏览器实测健康区渲染与错误路径；真实巡检结果待桌面端验收。

### ✅ 33. 环境页收口为整机配置中心：全局环境页回并 + 配置区按钮切换
- **动因**：把「全局环境」单开一页后，侧边栏出现「环境 / 全局环境」两个高度相似的入口；
  而原代码本就有注释「环境变量区块：与运行时同属"环境"配置域，共用一页」——拆页实为推翻原设计意图。
  此外页面过长，配置区需要显式切换而非一路下滚。
- **改动**：撤掉「全局环境」页（侧边栏 7 → 6 项），把预设装整机、整机快照、全局环境变量三块收回「环境」页；
  配置区改为顶部按钮切换「运行时 / 环境变量 / 环境预设」。三块**常驻挂载 + 显隐切换**——
  条件渲染会在切走时卸载组件，把进行中的流式安装日志一起丢掉。
- **复用抽取**：`PresetLibrary`（卡片网格 + 导入/导出）、`useUserPresets`（localStorage 预设库；
  两页互斥挂载，故挂载时读一次即可，无需跨页同步）、`useMachineInstall`（流式整机安装 + 日志）、
  `data/presets.ts`、`lib/nav.ts`（Tab 类型，避免各页面用裸 string 互相漂移）、`lib/presetFileIO.ts`。
- **安装通道统一**：新增 `preset_parse_tools` 命令（前端无 TOML 解析器，装整机前由后端解析 `[tools]`）；
  快照重建由同步 `install_version` 改为 `install_version_streaming`，与「预设装整机」共用
  `mise:install-progress` 事件通道——两者都有实时进度、都单个失败不中断。
- **顺带修复**：`styles.css` 的 `--border: var(--border)` 是自引用，构成 CSS 自定义属性循环使该变量
  在计算值阶段失效；因 styles.css 后加载赢得层叠，全站 53 处 `var(--border)` 边框**全部不渲染**
  （按钮 / 输入框 / 卡片 / 面板均无描边）。删除该行，`--border` 交由 index.css 按明暗主题提供。
- **UI 统一**：预设区块改用 `.panel` + `.panel-title`（与同页其他区块一致）；页面分工提示条复用
  `.banner.info` 配色（原先只有 3px 蓝色左边条、其余全灰）；`.banner` 内边距 14 → 18px，
  使横幅文字与面板标题落到同一条左基线。
- **验证**：cargo test 61 通过；vitest 22 通过；tsc / eslint / prettier / build 全绿；
  桌面端实测标签切换、预设导入预览、快照重建进度与边框恢复。

### ✅ 34. 环境预设可分享与自助管理：导入/导出 + 页内新建/修改
- **动因**：预设原本只活在「项目配置」页的 localStorage 里，既出不去也进不来，更没有页内编辑——
  想分享一套环境只能口头描述。同时预设与「整机快照」职责重叠，用户分不清该用哪个。
- **格式**：`.zenv.toml` = `[preset]` 元数据 + `[tools]` 工具段，人类可读可手改；首行带格式版本
  （未来 v2 会明确提示升级而非静默解析错）。`[tools]` 键一律加引号——`npm:prettier` 这类 mise
  后端名含冒号，裸键不是合法 TOML（已用真实 mise 2026.9.3 验证解析）。
  导入兼容纯 `mise.toml`（无 Z.Env 标记时按文件名命名）。
- **导入/导出**：三个导出入口（内置卡片 / 我的预设卡片 / 编辑器当前配置）；导入后预览工具清单与
  本机已装状态，再选安装去向。
- **页内新建/修改**：「新建预设」与「导入预设」并列在标题右侧——不放在「我的预设」标题下，
  因为后者在无预设时整块不渲染，新用户会看不到入口。卡片 ✎：内置卡片是「以它为模板新建」，
  我的预设是「修改」。修改**只替换 `[tools]` 段**（`replaceToolsSection`），保住 `[env]` /
  `_.path` 等既有配置——用户预设可能是从编辑器整份存下来的，整份重写会静默丢内容。
- **落库收敛**：`applyPresetEdit` 一次算完新建/同名覆盖/改名三种情况，调用方只写一次——
  `save` 与 `remove` 各自闭包同一份快照，连调会互相覆盖。修改既有条目时**原地替换**，
  卡片顺序不变。
- **职责划清**：预设是**配方**（我要装什么），整机快照是**照片**（本机现状打包）；README 补对照表。
  两者共用同一条流式安装通道。
- **验证**：42 个前端单测覆盖格式解析、段落替换与预设库三种落库路径；cargo test 61 通过；
  十条 CI 检查项本地全绿。桌面端截图实测发现并修掉三处只有真机才暴露的排版问题：
  `.dialog-head` 横排导致标题换行、描述压住关闭按钮；整窗 `overflow-y-auto` 让「取消 / 保存」
  被截出视野（改为头尾固定、只滚内容区）；卡片嵌入面板后边界不明显（根因是 `--border`
  自引用使全站边框失效，已在 33 修复）。

---

> 2026-10-02 规划：34 合并（PR #1）的预合并审查留下一项并发正确性缺陷与三个小项；
> 17 遗留的「更新器端到端验证」也一直悬着。下一轮先收口健壮性与发布，再开产品纵深（P9 候选见 37 后）。

## P8 — 发布与健壮性收口（35 → 36 → 37）

### ✅ 35. 安装通道并发治理：预设装整机 × 快照重建互斥
- **问题**（PR #1 审查 Follow-up）：MachinePresetSection 与 SnapshotPanel 共用 `mise:install-progress`
  事件通道，两入口并发触发时进度日志互相串台，且并发两条 `mise install` 可能互踩
  （同一工具被同时安装，mise 侧无并发保护）。
- **落地**：互斥判定抽为纯函数 `installChannelGate(selfBusy, otherBusy, other)`（`lib/installChannel.ts`，
  5 个单测），两组件直接消费同一份判定。`useMachineInstall` 实例上提到 EnvironmentView 页面级
  （`installApi` 注入 MachinePresetSection）——预设安装的 busy/日志与 tab 切换彻底解耦；
  快照重建开始/结束经 `onRestoreBusyChange` 上报页面级 `restoreBusy`。双向互斥：
  重建占用时预设卡片禁用并提示（PresetLibrary 新增 `useDisabled`/`useDisabledHint`，
  项目配置页不传不受影响）、openInstall 二次守门；预设安装占用时快照按钮禁用 + 提示文案。
  导出快照不动安装通道，不参与互斥。MachinePresetSection 弹窗补 `showClose={!installBusy}`
  （对齐 InstallDialog 惯例）。日志串台的根因是并发订阅，互斥后同一时刻只有一路订阅，随之消失。
- **验证**：vitest **53 passed**（新增 installChannel 5 + upsertPreset 3）；tsc / eslint / prettier /
  build 全绿。两路并发的真机表现待桌面端验收。

### ✅ 36. 审查遗留小项清零
- `.preset-head` 在 styles.css 857 与 987 两处逐字重复——删 987 处（后者缺 `margin-bottom`，
  前者本就赢得全部生效属性，删除零视觉差异）。
- `useUserPresets.save` 同名重存把条目挪到列表末尾——新增纯函数 `upsertPreset`
  （同名**原地替换**、新名字追加），`save` 与导入收录 `remember` 一并改走
  （后者有同样的跳位问题），与 `applyPresetEdit` 行为对齐；3 个单测。
- EnvironmentView tab-bar 补齐无障碍：roving tabindex（只有激活 tab 在 Tab 序列）+
  方向键/Home/End 环绕切换并移动焦点，`id`/`aria-controls` 与三个区块的
  `role="tabpanel"`/`aria-labelledby` 全部接线。
- **验证**：vitest 53 passed；tsc / eslint / prettier / build 全绿。键盘走查待桌面端验收。

### ✅ 37. v0.7.1 发布：验证应用内更新端到端
- **动因**：17 遗留②——latest.json 与签名工件已产出，但「检查更新 → 下载 → 自动重启」
  全链路从未实跑；且 33 修的全站边框、34 的预设格式兼容都值得尽快随版本出去。
- **前置**：v0.7.0 草稿先 Publish（它是 `/releases/latest/download/latest.json` 的解析目标，
  草稿状态拉不到，updater 全链路无法验证）；0.6.x 旧草稿未动。
- **实战踩坑**：首跑 Windows 构建失败——`action()` 的 systemd 分支全平台编译，而它调用的
  `validate_unit_name` 挂了 `#[cfg(not(windows))]`，Windows 报 E0425 not found。CI 的 rust job
  只在 ubuntu 跑，**Windows 专属编译错误只有 release 三平台构建才暴露**；本机交叉
  `cargo check --target windows` 也走不通（ring 的 C 编译要 MSVC 工具链）。修复：该纯字符串
  校验函数去平台门（services.rs），顺带清掉 Windows 专属路径三处警告（env.rs 重复赋值、
  system.rs 死变量）。其余 cfg 门全库排查一遍，均已成对/内联，无同类坑。
- **发布**：强推 tag 重跑（e8a2c66）三平台全绿，14 资产与 0.7.0 同构、`.sig` 齐全；
  已 Publish 为 Latest，`latest.json` 实测返回 version 0.7.1 + 9 平台条目
  （darwin-aarch64/windows-x86_64/linux-x86_64 及安装器变体）。
- **验证**：run 37034754049 success；17 遗留②④清零，①Intel Mac 产物挪入 P9 平台缺口。
- **端到端实测通过（2026-10-02）**：本机原装 0.6.7（updater 诞生前的版本，发布资产无 `.sig`、
  二进制未编译端点，无法自更新）——先从已发布的 v0.7.0 Release 下载 dmg 全新安装替换，
  再走应用内更新：检查更新报「发现新版本 0.7.1（当前 0.7.0）」→ 下载并安装（带 % 进度）→
  bundle 换包 + 自动重启（pid 更替）→ 设置页版本 0.7.1、面板显示「已是最新版本」，
  界面同步呈现 30 号迭代后的六项导航。更新器全链路（端点 → 元数据 → 下载 → 校验 →
  换包 → 重启）至此正式闭环。

