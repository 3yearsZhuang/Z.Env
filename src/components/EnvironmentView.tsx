import { useCallback, useEffect, useState } from "react";
import {
  detectSystemVersions,
  detectToolSources,
  envCenterList,
  errorMessage,
  getEnvInfo,
  listTools,
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
import { ChannelDialog } from "./SoftwareView";
import {
  CAT_KEYS_ALL,
  KNOWN_RUNTIMES,
  SOFTWARE,
  TOOL_ICON_PATHS,
  categoryOf,
} from "../data/catalog";

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

export default function EnvironmentView() {
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [sources, setSources] = useState<ToolSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [installFor, setInstallFor] = useState<ToolInfo | null>(null);
  const [catFilter, setCatFilter] = useState<string>("全部");
  const [compact, setCompact] = useState(false);
  // 本机可用的包管理器（供“仅第三方”工具的渠道弹窗使用）
  const [pkgAvail, setPkgAvail] = useState<Set<string>>(new Set());
  const [softChannel, setSoftChannel] = useState<{
    name: string;
    url?: string;
    desc?: string;
  } | null>(null);
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
        setPkgAvail(new Set(env.pkg.filter((p) => p.available).map((p) => p.name)));
        const supported = new Set([...KNOWN_RUNTIMES, ...SOFTWARE.map((s) => s.name)]);
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
      setNotice(global ? "已设为全局默认版本" : "已在当前项目启用该版本");
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

  // “卸载”二次确认：保存待删除键
  const [confirmUninstall, setConfirmUninstall] = useState<string | null>(null);

  /** 打开安装入口：mise 可管理的运行时走 mise 安装弹窗；
   *  仅第三方（下载类）走与“支持列表”等效的渠道弹窗。 */
  function openInstall(tool: ToolInfo) {
    if (KNOWN_RUNTIMES.includes(tool.name)) {
      setInstallFor(tool);
    } else {
      const soft = SOFTWARE.find((s) => s.name === tool.name);
      setSoftChannel({ name: tool.name, url: soft?.url, desc: soft?.desc });
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

  // 已由 mise 托管
  const installedSet = new Set(tools.map((t) => t.name));
  // 其他工具托管 -> 按工具聚合
  const externalByTool = new Map<string, ToolSource[]>();
  for (const s of sources) {
    const list = externalByTool.get(s.tool) || [];
    list.push(s);
    externalByTool.set(s.tool, list);
  }
  // 候选展示项：未由 mise 托管的已知运行时，加上“其他渠道(brew 等)已装”的工具
  //（即使用户只把它当软件下载项，如 git，只要本机通过 brew 装了也要显示）。
  const others = Array.from(
    new Set([
      ...KNOWN_RUNTIMES.filter((n) => !installedSet.has(n)),
      ...[...externalByTool.keys()].filter((n) => !installedSet.has(n)),
    ]),
  ).sort();
  // 未安装的候选也按统一卡片展示（0 个版本），允许安装。
  // 排序：已安装 > 仅托管 > 未安装，组内按名称。
  const rank = (t: ToolInfo) =>
    t.versions.length > 0 ? 0 : externalByTool.get(t.name)?.length ? 1 : 2;
  const merged: ToolInfo[] = [
    ...tools,
    ...others.map((name) => ({ name, versions: [], active_versions: [] })),
  ].sort((a, b) => {
    const r = rank(a) - rank(b);
    return r !== 0 ? r : a.name.localeCompare(b.name);
  });
  // 分类筛选（共享标签系统，含“其他”）
  const filtered =
    catFilter === "全部" ? merged : merged.filter((t) => categoryOf(t.name) === catFilter);
  // 紧凑模式状态色
  const statusOf = (t: ToolInfo) =>
    t.versions.length > 0
      ? "installed"
      : externalByTool.get(t.name)?.length
        ? "managed"
        : "uninstalled";
  const statusLabel = (t: ToolInfo) =>
    t.versions.length > 0 ? "已安装" : externalByTool.get(t.name)?.length ? "仅托管" : "未安装";

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>环境</h1>
          <p className="view-sub">
            运行时与全局环境变量：可由 mise 安装，或已通过其他渠道（nvm/pyenv/asdf、brew 等）托管
          </p>
        </div>
        <button className="btn-ghost" onClick={() => setCompact((c) => !c)} title="切换紧凑模式">
          {compact ? "▦ 列表" : "▣ 紧凑"}
        </button>
      </div>

      {envSnap && (
        <div
          className={`banner ${envSnap.conflicts.length > 0 ? "warn" : "info"}`}
          style={{ cursor: "pointer" }}
          onClick={() =>
            document.getElementById("env-panel")?.scrollIntoView({ behavior: "smooth" })
          }
        >
          全局环境变量：{envSnap.entries.length} 个
          {envSnap.conflicts.length > 0
            ? ` · ${envSnap.conflicts.length} 个同名冲突 · 点击查看`
            : " · 点击管理"}
        </div>
      )}

      <div className="toolbar">
        <div className="filter-chips">
          <button
            className={`chip ${catFilter === "全部" ? "active" : ""}`}
            onClick={() => setCatFilter("全部")}
          >
            全部
          </button>
          {CAT_KEYS_ALL.map((c) => (
            <button
              key={c}
              className={`chip ${catFilter === c ? "active" : ""}`}
              onClick={() => setCatFilter(c)}
            >
              {c}
            </button>
          ))}
        </div>
      </div>

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

      {loading && merged.length === 0 ? (
        <div className="empty">正在读取环境…</div>
      ) : compact ? (
        <div className="tool-grid compact">
          {CAT_KEYS_ALL.map((cat) => {
            const grp = filtered.filter((t) => categoryOf(t.name) === cat);
            if (grp.length === 0) return null;
            return (
              <div className="compact-group" key={cat}>
                <span className="compact-group-label">{cat}</span>
                <div className="compact-row">
                  {grp.map((tool) => (
                    <button
                      key={tool.name}
                      className={`tc-chip ${statusOf(tool)}`}
                      onClick={() => openInstall(tool)}
                      title={`${tool.name} · ${statusLabel(tool)}`}
                    >
                      <ToolBadge name={tool.name} />
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <>
          <div className="tool-grid">
            {filtered.map((tool) => {
              const isOpen = expanded === tool.name;
              const activeSet = new Set(tool.active_versions);
              const ext = externalByTool.get(tool.name);
              const isInstalled = tool.versions.length > 0;
              const isManaged = !isInstalled && Boolean(ext?.length);
              return (
                <div className={`tool-card ${isOpen ? "open" : ""}`} key={tool.name}>
                  <button
                    className="tool-card-head"
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

                  {isOpen && (
                    <div className="tool-card-body">
                      <div className="version-list">
                        {tool.versions.length === 0 && (
                          <div className="empty small">
                            {externalByTool.get(tool.name)?.length
                              ? `由 ${[
                                  ...new Set(externalByTool.get(tool.name)!.map((s) => s.manager)),
                                ].join(", ")} 托管 · ${
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
                        {/* 其他渠道已装版本：按版本去重展示，渠道标签与卸载按钮按管理器去重，避免同版本/同名重复 */}
                        {[
                          ...(() => {
                            const by = new Map<string, ToolSource[]>();
                            externalByTool.get(tool.name)?.forEach((s) => {
                              const k = s.version;
                              const arr = by.get(k) || [];
                              arr.push(s);
                              by.set(k, arr);
                            });
                            return by;
                          })().entries(),
                        ].map(([version, list]) => {
                          const mgrKey = [...new Set(list.map((s) => s.manager))].join(",");
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
                            <div className="version-row" key={`${mgrKey}-${version}`}>
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
                      <button className="btn add" onClick={() => openInstall(tool)}>
                        + 安装其他版本
                      </button>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </>
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
      {softChannel && (
        <ChannelDialog
          name={softChannel.name}
          url={softChannel.url}
          desc={softChannel.desc}
          available={pkgAvail}
          onClose={() => setSoftChannel(null)}
        />
      )}
    </div>
  );
}
