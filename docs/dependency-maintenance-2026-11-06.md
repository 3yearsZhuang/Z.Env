# 48：开发依赖维护与完整审计门禁

日期：2026-11-06（Asia/Shanghai）。基线为 `5688ef8`；本切片只维护开发依赖、必要传递依赖与审计门禁，不更新运行时生产依赖或 Rust 依赖。

## 动因

基线完整 `npm audit` 报告 1 high / 9 moderate，生产依赖审计为 0：

- `undici@8.11.2` 来自开发依赖 `jsdom@30.1.2`；修复线为 8.12.2，涉及异常解压、TLS 配置、缓存重定向等通告。
- `vitest/@vitest/mocker@4.1.11` 存在 mock 注册代码注入和 Windows 路径绕过问题，修复线为 4.1.12。
- `@typescript-eslint/typescript-estree` 使用的 `minimatch@10.2.6` 存在 glob 展开放大风险，修复线为 11.0.1。报告中的其他 typescript-eslint 项由这条传递依赖传播，不是九个独立漏洞。

CI 此前只阻断生产依赖 high，完整审计允许失败；发布流程也只检查生产依赖。开发工具和 CI 使用的依赖同样需要可执行的门禁。

## 落地

| 依赖 | 基线 → 当前锁定 | 维护方式 |
| --- | --- | --- |
| Vitest 与七项 `@vitest/*` 内部包 | 4.1.11 → 4.1.12 | 同主版本补丁更新，直接约束升至 `^4.1.12` |
| jsdom → undici | 8.11.2 → 8.12.4 | 保留 jsdom 30.1.2 和原范围，只更新该传递依赖 |
| typescript-estree → minimatch | 10.2.6 → 11.0.1 | 仅对 `@typescript-eslint/typescript-estree` 添加精确 `overrides` |
| minimatch → nonrecursive-glob-match | 新增 1.1.0 | 修复版本所需的开发传递依赖 |

typescript-eslint 8.71.0 保持原样；核实时最新 8.72.1 仍声明 minimatch `^10.2.2`，10.x 最新版仍是有通告的 10.2.6，因此单纯兼容范围更新不能消除这项报告。覆盖只作用于 typescript-estree，不影响 ESLint 的 minimatch 3.x。minimatch 11.0.1 保留该调用方使用的 `minimatch(path, pattern, { dot: true })` 接口，也支持 CI 使用的 Node 22。后续上游范围纳入修复版本后应移除覆盖，并重新运行解析与前端门禁。

- CI 和发布流程都执行 `npm audit --audit-level=high`，包含开发依赖；high / critical 阻断，不再使用 `continue-on-error` 放行完整审计。
- 生产依赖审计仍单独执行，方便识别发布产物与开发环境的归属。
- `package.json` 的生产依赖约束、锁文件中已有生产及可选平台条目保持原样；没有全量 `npm audit fix` 或生产升级。

## 验证记录

本地在独立工作区使用 Node 22.23.3 / npm 10.9.9，按锁文件重新安装：

| 检查 | 结果 |
| --- | --- |
| `npm ci` | 通过；独立依赖目录，不复用主工作区 |
| `npm test` | 43 文件 / 287 项通过，Vitest 4.1.12 |
| lint / format / build | ESLint、Prettier、TypeScript 与 Vite 生产构建通过 |
| version / docs | 版本 0.7.1 一致；35 个 Markdown、256 个相对链接通过 |
| 完整 / 生产依赖审计 | 均为 0 漏洞 |
| 锁图范围 | 比较基线确认所有既有生产及可选平台条目未变化 |
| minimatch 调用方兼容 | 真实 typescript-eslint project service 验证精确匹配、隐藏文件通配匹配、未匹配文件拒绝，均通过；临时项目已清理 |
| `git diff --check` | 通过 |

当前锁图的完整和生产审计均为 0。审计结果按查询时间记录，未来发布仍须重新查询；本切片未执行 Rust 检查、桌面实机验收或发布工作流，远端门禁随独立 PR 运行。

通告依据：[minimatch glob 放大](https://github.com/advisories/GHSA-g3hv-75wg-m249)、[Vitest mock 注入](https://github.com/advisories/GHSA-qxg6-89mg-rqv8)、[Windows 路径绕过](https://github.com/advisories/GHSA-xp26-3454-mjgp)、[undici 异常解压](https://github.com/advisories/GHSA-xjj6-f33q-929c)。
