import { useCallback, useEffect, useState } from "react";
import {
  detectSystemVersions,
  detectToolSources,
  envCenterList,
  errorMessage,
  getEnvInfo,
  listTools,
  managedAdopt,
  managedReconcile,
  uninstallSystemPackage,
  uninstallVersion,
  useVersion,
  ReconcileEvent,
  SystemPkg,
  ToolSource,
  ToolInfo,
  type EnvCenterSnapshot,
} from "../api";
import InstallDialog from "./InstallDialog";
import EnvPanel from "./EnvPanel";
import { KNOWN_RUNTIMES, TOOL_ICON_PATHS } from "../data/catalog";

/** 工具徽标：先走显式映射，再按 /tools/{name}.svg 通用查找，都没有则回退名称缩写 */
function ToolBadge({ name }: { name: string }) {
  const [failed, setFailed] = useState(false);
  const key = name.toLowerCase();
  // 支持形如 "nodejs"、"node.js" 的别名
  const explicit = TOOL_ICON_PATHS[key] || TOOL_ICON_PATHS[key.replace(/[\W_]+/g, "")] || null;
  const src = !failed && (explicit || `/tools/${key}.svg`);
  if (src && !failed) {
    return (
      <span className="tool-badge icon">
        <img
          src={src}
          alt={name}
          className="tool-badge-img"
          draggable={false}
          onError={() => setFailed(true)}
        />
      </span>
    );
  }
  return <span className="tool-badge">{name.slice(0, 2).toUpperCase()}</span>;
}

/** 可接管的来源：有真实目录路径（系统包管理器版本 path 为空，走不了接入） */
function adoptable(s: ToolSource): boolean {
  return Boolean(s.path) && s.manager !== "system";
}

export default function EnvironmentView() {
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [sources, setSources] = useState<ToolSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [installFor, setInstallFor] = useState<ToolInfo | null>(null);
  // 托管接入对账事件：brew 升级/卸载后自动重连或移除的提示
  const [reconcile, setReconcile] = useState<ReconcileEvent[]>([]);
  // 全局环境变量快照：顶部摘要条与底部变量区同源
  const [envSnap, setEnvSnap] = useState<EnvCenterSnapshot | null>(null);

  const loadEnvSnap = useCallback(() => {
    envCenterList()
      .then(setEnvSnap)
      .catch(() => {});
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [toolsRes, sourcesRes] = await Promise.all([listTools(), detectToolSources()]);
      // 并入系统包管理器（winget/apt/pacman）已装的本机工具；brew 已由 detect_tool_sources
      // 的 Cellar 扫描提供（含版本与路径），这里排除以避免重复。
      let systemRes: ToolSource[] = [];
      const env = await getEnvInfo().catch(() => null);
      if (env) {
        const supported = new Set([...KNOWN_RUNTIMES]);
        const availPkg = env.pkg.filter((p) => p.available && p.name !== "brew").map((p) => p.name);
        const perMgr = await Promise.all(
          availPkg.map((pm) =>
            detectSystemVersions(pm)
              .then((pkgs) => ({ pm, pkgs }))
              .catch(() => ({ pm, pkgs: [] as SystemPkg[] })),
          ),
        );
        systemRes = perMgr.flatMap(({ pm, pkgs }) =>
          pkgs
            .filter((p) => supported.has(p.name))
            .map((p) => ({
              tool: p.name,
              version: p.version ?? "",
              manager: pm,
              path: "",
            })),
        );
      }
      setTools(toolsRes);
      setSources([...sourcesRes, ...systemRes]);
      // 托管接入对账：brew 升级/卸载后自动重连或清理失效链接，事件展示给用户
      managedReconcile()
        .then((events) => setReconcile(events))
        .catch(() => {});
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    loadEnvSnap();
  }, [loadEnvSnap]);

  async function handleActivate(tool: string, version: string, global: boolean) {
    setBusy(`${tool}@${version}`);
    setNotice(null);
    try {
      await useVersion(tool, version, global);
      setNotice(global ? `已把 ${tool} 全局默认设为 ${version}` : "已在当前项目启用该版本");
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleUninstall(tool: string, version: string) {
    setBusy(`${tool}@${version}`);
    setNotice(null);
    try {
      await uninstallVersion(tool, version);
      setNotice("卸载完成");
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  // "卸载"二次确认：保存待删除键
  const [confirmUninstall, setConfirmUninstall] = useState<string | null>(null);

  /** 一键接管：外部渠道（nvm/pyenv/asdf/brew 等）版本直接接入 mise，后端自动选策略并校验回滚 */
  async function handleAdopt(s: ToolSource) {
    setBusy(`${s.tool}@${s.version}`);
    setNotice(null);
    try {
      await managedAdopt(s.tool, s.version, s.manager, s.path);
      setNotice(`已接管 ${s.tool}@${s.version}（${s.manager} → mise），后续升级由对账自愈看护`);
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  /** 原生卸载其他渠道（brew/系统包管理器）安装的环境 */
  async function uninstallExternal(tool: string, s: ToolSource) {
    const key = `${s.manager}::${tool}::${s.version}`;
    if (confirmUninstall !== key) {
      setConfirmUninstall(key); // 第一次点击进入确认态
      return;
    }
    setConfirmUninstall(null);
    setBusy(`${tool}@${s.version}`);
    setNotice(null);
    try {
      await uninstallSystemPackage(s.manager, s.tool);
      setNotice(`已卸载 ${tool}（${s.manager}）`);
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  // 其他工具托管 -> 按工具聚合
  const externalByTool = new Map<string, ToolSource[]>();
  for (const s of sources) {
    const list = externalByTool.get(s.tool) || [];
    list.push(s);
    externalByTool.set(s.tool, list);
  }
  // 展示卡片：mise 已装 + 仅其他渠道托管；排序 mise 已装在前，组内按名称
  const cards: ToolInfo[] = [
    ...tools.filter((t) => t.versions.length > 0),
    ...[...externalByTool.keys()]
      .filter((n) => !tools.some((t) => t.name === n && t.versions.length > 0))
      .sort()
      .map((name) => ({ name, versions: [], active_versions: [] })),
  ].sort((a, b) => {
    const r = (b.versions.length > 0 ? 1 : 0) - (a.versions.length > 0 ? 1 : 0);
    return r !== 0 ? r : a.name.localeCompare(b.name);
  });

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>环境</h1>
          <p className="view-sub">
            已安装的运行时与全局环境变量；新环境在「支持列表」安装后自动出现在这里
          </p>
        </div>
      </div>

      {envSnap && (
        <div
          className={`banner ${envSnap.conflicts.length > 0 ? "warn" : "info"}`}
          style={{ cursor: "pointer" }}
          onClick={() =>
            document.getElementById("env-panel")?.scrollIntoView({ behavior: "smooth" })
          }
        >
          全局环境：{cards.length} 个运行时 · {envSnap.entries.length} 个环境变量
          {envSnap.conflicts.length > 0
            ? ` · ${envSnap.conflicts.length} 个同名冲突 · 点击查看`
            : ""}
        </div>
      )}

      {error && (
        <div className="banner error" onClick={() => setError(null)}>
          ⚠ {error}
        </div>
      )}
      {reconcile.length > 0 && (
        <div className="banner info" onClick={() => setReconcile([])}>
          {reconcile.map((e, i) => (
            <div key={i}>{e.message}</div>
          ))}
        </div>
      )}
      {notice && (
        <div className="banner success" onClick={() => setNotice(null)}>
          ✓ {notice}
        </div>
      )}

      {loading && cards.length === 0 ? (
        <div className="empty">正在读取环境…</div>
      ) : cards.length === 0 ? (
        <div className="empty">
          本机还没有已安装的运行时。前往「支持列表」选择安装，装好后会自动出现在这里。
        </div>
      ) : (
        <div className="tool-grid">
          {cards.map((tool) => {
            const isOpen = expanded === tool.name;
            const activeSet = new Set(tool.active_versions);
            const ext = externalByTool.get(tool.name) ?? [];
            const adoptableExt = ext.filter(adoptable);
            const isInstalled = tool.versions.length > 0;
            const isManaged = !isInstalled && ext.length > 0;
            // 头部快捷能力：单版本未设全局 → 设为全局；唯一可接管来源 → 一键接管
            const quickSetGlobal = tool.versions.length === 1 && tool.active_versions.length === 0;
            const quickAdopt = !isInstalled && adoptableExt.length === 1;
            return (
              <div className={`tool-card ${isOpen ? "open" : ""}`} key={tool.name}>
                <div style={{ display: "flex", alignItems: "stretch" }}>
                  <button
                    className="tool-card-head"
                    style={{ flex: 1, minWidth: 0 }}
                    onClick={() => setExpanded(isOpen ? null : tool.name)}
                  >
                    <ToolBadge name={tool.name} />
                    <span className="tool-name">{tool.name}</span>
                    <span
                      className={`pill state ${isInstalled ? "active" : isManaged ? "managed" : "uninstalled"}`}
                    >
                      {isInstalled ? "已安装" : isManaged ? "仅托管" : "未安装"}
                    </span>
                    {tool.active_versions.length > 0 && (
                      <span className="pill active">▲ {tool.active_versions.join(", ")}</span>
                    )}
                    <span className="tool-count">{tool.versions.length} 个版本</span>
                    <span className="chevron">{isOpen ? "▾" : "▸"}</span>
                  </button>
                  {quickSetGlobal && (
                    <button
                      className="btn xs primary"
                      style={{ margin: "auto 8px" }}
                      disabled={busy === `${tool.name}@${tool.versions[0].version}`}
                      onClick={() => handleActivate(tool.name, tool.versions[0].version, true)}
                      title="把唯一已装版本设为全局默认"
                    >
                      设为全局
                    </button>
                  )}
                  {quickAdopt && (
                    <button
                      className="btn xs"
                      style={{ margin: "auto 8px" }}
                      disabled={busy === `${tool.name}@${adoptableExt[0].version}`}
                      onClick={() => handleAdopt(adoptableExt[0])}
                      title={`${adoptableExt[0].manager} 的 ${tool.name}@${adoptableExt[0].version} 接入 mise`}
                    >
                      一键接管
                    </button>
                  )}
                </div>

                {isOpen && (
                  <div className="tool-card-body">
                    <div className="version-list">
                      {tool.versions.length === 0 && (
                        <div className="empty small">
                          {ext.length
                            ? `由 ${[...new Set(ext.map((s) => s.manager))].join(", ")} 托管 · ${
                                KNOWN_RUNTIMES.includes(tool.name)
                                  ? "尚未用 mise 安装"
                                  : "无法用 mise 安装"
                              }`
                            : "尚未安装版本"}
                        </div>
                      )}
                      {tool.versions.map((v) => {
                        const isActive = activeSet.has(v.version);
                        const busyKey = `${tool.name}@${v.version}`;
                        return (
                          <div className="version-row" key={v.version}>
                            <div className="version-info">
                              <span className="version-no">{v.version}</span>
                              {isActive && <span className="pill active small">当前</span>}
                              {v.requested_version && v.requested_version !== v.version && (
                                <span className="pill muted small">{v.requested_version}</span>
                              )}
                            </div>
                            <div className="version-actions">
                              <button
                                className={`btn xs ${!isActive ? "primary" : ""}`}
                                disabled={busy === busyKey}
                                onClick={() => handleActivate(tool.name, v.version, true)}
                                title={isActive ? "此版本已是全局默认" : "设为全局默认版本"}
                              >
                                {isActive ? "全局" : "设为全局"}
                              </button>
                              <button
                                className="btn xs danger"
                                disabled={busy === busyKey && tool.versions.length === 1}
                                onClick={() => handleUninstall(tool.name, v.version)}
                              >
                                卸载
                              </button>
                            </div>
                          </div>
                        );
                      })}
                      {/* 其他渠道已装版本：按版本去重展示；可接管的给一键接管，卸载按钮按管理器去重 */}
                      {[
                        ...(() => {
                          const by = new Map<string, ToolSource[]>();
                          ext.forEach((s) => {
                            const k = s.version;
                            const arr = by.get(k) || [];
                            arr.push(s);
                            by.set(k, arr);
                          });
                          return by;
                        })().entries(),
                      ].map(([version, list]) => {
                        const adoptTargets = list.filter(adoptable);
                        const busyKey = `${tool.name}@${version}`;
                        const uniqUninstall = [
                          ...new Map(
                            list
                              .filter((s) =>
                                ["brew", "winget", "apt", "pacman"].includes(s.manager),
                              )
                              .map((s) => [s.manager, s]),
                          ).values(),
                        ];
                        return (
                          <div className="version-row" key={`${version}-ext`}>
                            <div className="version-info">
                              <span className="version-no">{version || "?"}</span>
                              {[...new Set(list.map((s) => s.manager))].map((mgr) => (
                                <span className="pill channel small" key={mgr}>
                                  {mgr}
                                </span>
                              ))}
                              <span className="pill muted small">其他渠道</span>
                            </div>
                            <div className="version-actions">
                              {adoptTargets.map((s) => (
                                <button
                                  className="btn xs"
                                  key={`adopt-${s.manager}`}
                                  disabled={busy === busyKey}
                                  onClick={() => handleAdopt(s)}
                                  title={`把 ${s.manager} 的该版本接入 mise（自动选策略并校验）`}
                                >
                                  一键接管
                                </button>
                              ))}
                              {uniqUninstall.map((s) => {
                                const ukey = `${s.manager}::${tool.name}::${s.version}`;
                                return (
                                  <button
                                    className="btn xs danger"
                                    key={s.manager}
                                    disabled={busy === busyKey}
                                    onClick={() => uninstallExternal(tool.name, s)}
                                    title={
                                      confirmUninstall === ukey
                                        ? "再次点击确认卸载"
                                        : `通过 ${s.manager} 卸载`
                                    }
                                  >
                                    {confirmUninstall === ukey
                                      ? "确认卸载？"
                                      : s.manager === "brew"
                                        ? "卸载"
                                        : `卸载(${s.manager})`}
                                  </button>
                                );
                              })}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                    {KNOWN_RUNTIMES.includes(tool.name) && (
                      <button className="btn add" onClick={() => setInstallFor(tool)}>
                        + 安装其他版本
                      </button>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* 环境变量区块：与运行时同属"环境"配置域，共用一页 */}
      <EnvPanel snap={envSnap} reload={loadEnvSnap} />

      {installFor && (
        <InstallDialog
          tool={installFor}
          external={externalByTool.get(installFor.name)}
          onClose={() => setInstallFor(null)}
          onDone={async () => {
            await refresh();
            setNotice("安装完成");
          }}
        />
      )}
    </div>
  );
}
