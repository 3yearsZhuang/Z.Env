# 前端体系收敛与开销优化

日期：2026-11-05。对应提交 `b7cc04a` → `513f785`；本轮为尚未发布的前端改动。

## 动因

MiuiX 重设计以「追加覆盖」方式落地后，前端出现四处结构性成本：

- **样式三层叠加**：`styles.css` 790 个选择器里只有 649 个唯一，141 条规则是覆盖式重复；
  97 条选择器在重设计后已无引用。圆角 26 种取值、字号 25 种取值，卡片间距靠逐页补丁维持。
- **令牌二次定义**：`styles.css` 在 `index.css` 之后重定义 `--accent`/`--radius`，
  导致 Tailwind 语义类（`bg-accent`、`rounded-md`）解析成另一套值，属潜伏错配。
- **空转开销**：首页固定 2 秒轮询，窗口最小化照跑；每轮后端都会起一次 `pmset -g batt`
  子进程（实测 6.8ms），即每小时 1800 次；`get_env_info` 被三个页面各自采集一次。
- **半成品设计层**：`components/ui/` 四个原语里 `button`/`input`/`badge` 零消费者，
  141 处按钮仍走 `.btn` 家族；21 个组件中 10 个没有测试。

## 落地

| 方向 | 提交 | 内容 |
| --- | --- | --- |
| 死代码 | `b7cc04a` | 删除 111 条无引用规则与 9 处选择器列表中的死项；`styles.css` 3984 → 3305 行 |
| 覆盖合并 | `546f770` | 45 组重复选择器按「最后一次出现」合并，保留 `!important` 与旧语法兜底对 |
| 令牌与刻度 | `1926492` | `index.css` 成为唯一令牌层（颜色/圆角 6 档/字号 9 档/间距 4 档/字体栈/头图），`styles.css` 不再二次定义；`border-radius`、`font-size` 全库无裸 px；选中态改用 `primary`/`primary-foreground` 修正对比度 |
| 采样与编辑器开销 | `e910bb8` | 首页采样自调度：聚焦 2s、失焦 10s、隐藏停止并在回前台补采；后端新增 `SlowMetrics`（电量 TTL 30s、磁盘 TTL 5s）；`TomlEditor` 去掉 `basicSetup` 全家桶改为最小组装，主题吃令牌，删除 `useDarkMode` |
| 原语与依赖 | `9b6077a` | 删除零消费者的 `ui/button`、`ui/input`、`ui/badge` 与 `@radix-ui/react-slot`、`class-variance-authority`、`@codemirror/language-data`、`@tauri-apps/plugin-opener` |
| 提示与无障碍 | `7b7a449` | 六处提示条改用统一 `Notice`（`role="alert"`/`role="status"`、真按钮关闭）；彩蛋弹窗补 `role="dialog"`、`aria-modal`、初始焦点、Tab 循环与 Esc；图标补 `loading="lazy"` |
| 测试与采集复用 | `513f785` | `readEnvInfo` 进程级缓存（并发合并、失败不缓存、`force` 绕过）；补 EnvPanel 5 项、PresetLibrary 3 项、SettingsView 4 项、Dashboard 3 项、TomlEditor 2 项、EasterEgg 3 项、Notice 3 项、缓存 3 项 |
| 状态块统一 | 本条提交 | `.empty`/`.empty.small`/`.project-empty`/`.ops-empty`/`.support-empty` 五套几何 + 两套旋转动画收敛为 `StateBlock`（lg/sm/md + bordered/tall）与 `.state-spinning`，19 处调用点迁移，删除 11 条各自写死的规则 |

安全约束：删规则与合并规则都先用脚本比对「仍被引用的类是否失去唯一定义」（0 个）与
「每个选择器的有效声明表是否变化」（0 处），保证等价后再提交。

## 验证记录

- `npm test`：39 个测试文件、258 项测试通过；`npm run lint`、`npm run format:check`、
  `npm run build`（TypeScript + Vite）通过。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`：290 项通过
  （含本轮新增 4 项 `SlowMetrics` 用例）。
- 体积：主入口 JS 237.19 kB（gzip 75.97）；`TomlEditor` 异步块 405.57 → 298.48 kB
  （gzip 132.22 → 97.53）；主 CSS 76.87 → 64.50 kB（gzip 15.39 → 13.14）。
- 视觉回归用 headless Chromium（`Google Chrome for Testing`，1440/900/640 × 明暗）加载
  只链构建产物的静态夹具，比对 50 个代表元素 × 14 项计算样式：差异仅限刻度归一
  （圆角/字号及其派生的高度、行高，≤2px）与选中态配色修正；无新增溢出、无缺失元素。
- 未覆盖边界：探针是静态夹具而非真实 Tauri 窗口，未做三平台实机与打包产物验收；
  项目配方的执行编排、Windows/Linux 布局与其余工具渠道不在本轮范围。
