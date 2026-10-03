# 发布 Runbook

推送 `v*` 标签触发 release.yml，三平台打包并产出应用内更新元数据。以下按序执行；
「已知坑」是历次发版实踩的记录，开工前先过一遍。

## 前置配置（一次性，均已就位，排障时核对）

- **签名 Secret**：`TAURI_SIGNING_PRIVATE_KEY` 为本机 `~/.tauri/zenv.key` 的内容（密钥无密码），
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 留空——未配置的 Secret 在 Actions 中解析为空串，正好匹配。
  公钥位于 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。
- **仓库 Actions 默认 workflow 权限为 write**：GITHUB_TOKEN 需要建 Release；若报
  "Resource not accessible by integration" 先查这里（曾被改为 read 导致 v0.7.0 首跑失败）。
- **`tauri.conf.json` 的 `bundle.createUpdaterArtifacts: true`**：Tauri 2 默认不产出更新器工件，
  缺了它就没有 `.app.tar.gz/.sig`，tauri-action 报 "Signature not found for the updater JSON"
  跳过 `latest.json`，应用内更新无元数据。

## 步骤

1. **三处版本号同步升位**：`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml`
   （Cargo.lock 随之更新）。`npm run check:version` 门禁校验，CI 首步执行。
2. **本地先跑全量 CI 门禁**——本地 clippy 缓存会残留旧 lint 结果，未改的文件可能只在 CI
   全新环境下报错，推送前强制全量：
   ```bash
   cargo clean -p mise-gui && cargo clippy --all-targets -- -D warnings
   cargo fmt --check && cargo test
   npm run check:version && npm run lint && npm test && npm run format:check
   ```
3. **打标签触发构建**：`git tag vX.Y.Z && git push origin vX.Y.Z`。若 run 中途修东西重跑，
   删 tag 重打强推即可。
4. **盯 run 到全绿**：macOS / Windows / Linux 三平台打包。macOS arm64 runner 可能长时间排队
   （v0.6.7 曾排队 24h 后超时取消），发布窗口避开拥挤时段，必要时重跑。
5. **核对草稿 Release 资产**：Release 以**草稿**产出，应得 14 个资产——
   macOS aarch64 dmg + `.app.tar.gz(+.sig)`；Windows nsis/msi（各带 `.sig`）；
   Linux AppImage/deb/rpm（各带 `.sig`）；`latest.json` 含 9 个平台条目
   （darwin-aarch64 / windows-x86_64 / linux-x86_64 及安装器变体）。
6. **手动 Publish**：确认资产无误后 Publish 为 Latest。
7. **验证更新元数据**：`/releases/latest/download/latest.json` 应返回刚发布的版本号与 9 个
   平台条目。**草稿状态拉不到**——`/releases/latest/` 端点只解析已发布版本。
8. **验证应用内更新**（建议）：上一个已发布版本的应用内「检查更新」→ 应报新版本 →
   下载（带 % 进度）→ 换包自动重启 → 设置页版本号已变更。0.7.0 → 0.7.1 已做过一次全链路实测
   （2026-10-02）。注意验证链要求**上一个版本也是带 updater 的已发布版本**——0.6.7 这类
   updater 诞生前的版本无法自更新，需先手动安装新版再验证。

## 已知坑速查

| 坑                             | 症状                                            | 处置                                                                              |
| ------------------------------ | ----------------------------------------------- | --------------------------------------------------------------------------------- |
| Tauri npm 包与 Rust crate 错位 | release 构建 fail                               | 两侧同步升级（`@tauri-apps/api` ↔ `tauri`、各 plugin ↔ 对应 crate，对齐同 minor） |
| workflow 默认权限为 read       | "Resource not accessible by integration"        | 仓库设置 → Actions → 默认 workflow 权限改回 write                                 |
| 未开 createUpdaterArtifacts    | "Signature not found..."，latest.json 缺失      | `tauri.conf.json` 显式开启                                                        |
| Windows 专属编译错误           | CI（仅 ubuntu）全绿，release 的 Windows job 挂  | 本机无法交叉 check（ring 要 MSVC）；根治靠 P9 的 CI Windows cargo check           |
| 本地 clippy 缓存旧结果         | 本地全绿、CI lint 挂（未改文件报新 lint）       | `cargo clean -p mise-gui` 后全量重 lint                                           |
| tauri-build 沙盒路径残留       | 本机构建读 TRAE 沙盒权限文件失败                | `rm -rf src-tauri/target/debug/build/tauri-* src-tauri/target/debug/.fingerprint/tauri-*` |
| macOS arm64 runner 排队        | run 长时间 pending 直至超时                     | 换发布窗口，删 tag 重跑                                                            |

## 遗留缺口

- **Intel Mac 产物未覆盖**（17 遗留①）：release.yml 的 macOS matrix 装了 x86_64 target 但
  构建未使用，产物只有 aarch64——后续加 `--target` 或出 universal 二进制。
- **Windows 服务列表暂缺**：待实机验证后提供，UI 处已显式说明。
