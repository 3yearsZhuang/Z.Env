import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import {
  detectSystemVersions,
  detectToolSources,
  envCenterList,
  errorMessage,
  getEnvInfo,
  listTools,
  managedAdopt,
  uninstallSystemPackage,
  uninstallVersion,
  useVersion,
  ToolSource,
  ToolInfo,
  type EnvCenterSnapshot,
} from "../api";
import InstallDialog from "./InstallDialog";
import DoctorPanel from "./DoctorPanel";
import EnvPanel from "./EnvPanel";
import MachinePresetSection from "./MachinePresetSection";
import SnapshotPanel from "./SnapshotPanel";
import NodeInventoryPanel from "./NodeInventoryPanel";
import ToolCatalogPanel from "./ToolCatalogPanel";
import type { CatalogReadState } from "../lib/catalogEvidence";
const PythonInventoryPanel = lazy(() => import("./PythonInventoryPanel"));
const SoftwareView = lazy(() => import("./SoftwareView"));
import type { Navigate } from "../lib/nav";
import { useMachineInstall } from "../lib/useMachineInstall";
import { KNOWN_RUNTIMES, TOOL_ICON_PATHS } from "../data/catalog";
import "./EnvironmentView.css";

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

interface Props {
  active?: boolean;
  nodeDetailsRequest?: number;
  inventoryChange?: number;
  compatibilityRequest?: number;
  /** 切到别的 tab（预设区块里给出「装到某个项目」的出口） */
  onNavigate?: Navigate;
}

/** 环境页顶部三个区块的顺序（tablist 键盘导航按此环绕） */
const PANE_ORDER = ["runtime", "vars", "preset"] as const;

export default function EnvironmentView({
  onNavigate,
  active = true,
  nodeDetailsRequest = 0,
  inventoryChange = 0,
  compatibilityRequest = 0,
}: Props) {
  const [catalogRead, setCatalogRead] = useState<CatalogReadState>({
    snapshot: null,
    loading: false,
    error: null,
  });
  const [pythonRead, setPythonRead] = useState<CatalogReadState>({
    snapshot: null,
    loading: false,
    error: null,
  });
  const [detailTool, setDetailTool] = useState<"node" | "python">("node");
  const [pythonVisited, setPythonVisited] = useState(false);
  const [pythonFocus, setPythonFocus] = useState(0);
  const [compatibilityOpen, setCompatibilityOpen] = useState(false);
  const [compatibilityVisited, setCompatibilityVisited] = useState(false);
  const [catalogNodeFocus, setCatalogNodeFocus] = useState(0);
  const consumedCompatibility = useRef(0);
  const openCompatibility = () => {
    setCompatibilityOpen(true);
    setCompatibilityVisited(true);
  };
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [sources, setSources] = useState<ToolSource[]>([]);
  const [systemSources, setSystemSources] = useState<ToolSource[]>([]);
  const [loading, setLoading] = useState(false);
  const [managedExpanded, setManagedExpanded] = useState(false);
  const [inventoryRevision, setInventoryRevision] = useState(0);
  const refreshGeneration = useRef(0);
  const consumedNodeIntent = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [installFor, setInstallFor] = useState<ToolInfo | null>(null);
  // 全局环境变量快照：供环境变量区块使用；快照重建会改写 [env]，需重新加载
  const [envSnap, setEnvSnap] = useState<EnvCenterSnapshot | null>(null);
  // 顶部区块切换：三块常驻挂载、按需显隐，避免切换时丢掉进行中的安装日志与流式进度
  const [pane, setPane] = useState<"runtime" | "vars" | "preset">("runtime");
  useEffect(() => {
    if (active && nodeDetailsRequest > consumedNodeIntent.current) {
      consumedNodeIntent.current = nodeDetailsRequest;
      setPane("runtime");
      setDetailTool("node");
    }
  }, [active, nodeDetailsRequest]);
  useEffect(() => {
    if (active && compatibilityRequest > consumedCompatibility.current) {
      consumedCompatibility.current = compatibilityRequest;
      setPane("runtime");
      setCompatibilityOpen(true);
      setCompatibilityVisited(true);
    }
  }, [active, compatibilityRequest]);
  // 安装通道唯一实例挂在页面级：「装到整机」与「快照重建」共用 busy 状态实现互斥，
  // 也保证切 tab 不会丢掉进行中的流式安装
  const machineInstall = useMachineInstall();
  // 快照重建占用安装通道中（由 SnapshotPanel 上报）
  const [restoreBusy, setRestoreBusy] = useState(false);
  // tablist roving tabindex：只有激活 tab 在 Tab 序列里，方向键/Home/End 切换并移动焦点
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  function onTabKeyDown(
    e: React.KeyboardEvent<HTMLButtonElement>,
    key: (typeof PANE_ORDER)[number],
  ) {
    const idx = PANE_ORDER.indexOf(key);
    let next: (typeof PANE_ORDER)[number] | undefined;
    if (e.key === "ArrowRight" || e.key === "ArrowDown")
      next = PANE_ORDER[(idx + 1) % PANE_ORDER.length];
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp")
      next = PANE_ORDER[(idx + PANE_ORDER.length - 1) % PANE_ORDER.length];
    else if (e.key === "Home") next = PANE_ORDER[0];
    else if (e.key === "End") next = PANE_ORDER[PANE_ORDER.length - 1];
    if (!next) return;
    e.preventDefault();
    setPane(next);
    tabRefs.current[next]?.focus();
  }

  const loadEnvSnap = useCallback(() => {
    envCenterList()
      .then(setEnvSnap)
      .catch((e) => setError(errorMessage(e)));
  }, []);

  const refresh = useCallback(async () => {
    const generation = ++refreshGeneration.current;
    const current = () => generation === refreshGeneration.current;
    setLoading(true);
    setError(null);
    const failed = (label: string, error: unknown) => {
      if (current())
        setError((previous) =>
          [previous, `${label}：${errorMessage(error)}`].filter(Boolean).join("；"),
        );
    };
    // 兼容入口独立发布成功来源，mise 失败不吞掉已读取的外部运行时。
    await Promise.allSettled([
      listTools()
        .then((result) => {
          if (current()) setTools(result);
        })
        .catch((error) => failed("mise 读取失败，保留上次数据", error)),
      detectToolSources()
        .then((result) => {
          if (current()) setSources(result);
        })
        .catch((error) => failed("外部来源读取失败，保留上次数据", error)),
      getEnvInfo()
        .then(async (env) => {
          const supported = new Set([...KNOWN_RUNTIMES]);
          const managers = env.pkg
            .filter((pkg) => pkg.available && pkg.name !== "brew")
            .map((pkg) => pkg.name);
          if (current())
            setSystemSources((previous) =>
              previous.filter((source) => managers.includes(source.manager)),
            );
          await Promise.allSettled(
            managers.map(async (manager) => {
              try {
                const packages = await detectSystemVersions(manager);
                if (!current()) return;
                const result: ToolSource[] = packages
                  .filter((pkg) => supported.has(pkg.tool ?? pkg.name))
                  .map((pkg) => ({
                    tool: pkg.tool ?? pkg.name,
                    packageId: pkg.name,
                    version: pkg.version ?? "",
                    manager,
                    path: "",
                  }));
                setSystemSources((previous) => [
                  ...previous.filter((source) => source.manager !== manager),
                  ...result,
                ]);
              } catch (error) {
                failed(`${manager} 读取失败，保留上次数据`, error);
              }
            }),
          );
        })
        .catch((error) => failed("渠道信息读取失败，保留上次数据", error)),
    ]);
    if (current()) setLoading(false);
  }, []);

  useEffect(() => {
    if (active && pane === "runtime" && managedExpanded) void refresh();
    return () => {
      refreshGeneration.current += 1;
    };
  }, [refresh, active, pane, managedExpanded]);

  useEffect(() => {
    if (active) void loadEnvSnap();
  }, [loadEnvSnap, active]);

  async function handleActivate(tool: string, version: string, global: boolean) {
    setBusy(`${tool}@${version}`);
    setNotice(null);
    try {
      await useVersion(tool, version, global);
      setInventoryRevision((revision) => revision + 1);
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
      setInventoryRevision((revision) => revision + 1);
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
      setInventoryRevision((revision) => revision + 1);
      setNotice(
        `已接管 ${s.tool}@${s.version}（${s.manager} → mise）；来源变化时请在环境健康中显式修复`,
      );
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
      await uninstallSystemPackage(s.manager, s.packageId ?? s.tool);
      setInventoryRevision((revision) => revision + 1);
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
  for (const s of [...sources, ...systemSources]) {
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
          <h1>本机工具</h1>
          <p className="view-sub">
            查看工具来源、安装实例与验证结果，也可以管理全局环境变量、预设和快照。 Node.js
            来源清单独立于 mise；项目要求和终端实际使用的版本需分别验证。
          </p>
        </div>
      </div>

      {/* 顶部切换：三个区块互斥显示，切换时互不干扰 */}
      <div className="tab-bar" role="tablist" aria-label="环境页区块">
        {(
          [
            ["runtime", "工具与来源"],
            ["vars", "环境变量"],
            ["preset", "环境预设"],
          ] as const
        ).map(([key, label]) => (
          <button
            key={key}
            ref={(el) => {
              tabRefs.current[key] = el;
            }}
            className={`tab-btn${pane === key ? " active" : ""}`}
            role="tab"
            id={`env-tab-${key}`}
            aria-selected={pane === key}
            aria-controls={`env-pane-${key}`}
            tabIndex={pane === key ? 0 : -1}
            onKeyDown={(e) => onTabKeyDown(e, key)}
            onClick={() => setPane(key)}
          >
            {label}
          </button>
        ))}
      </div>

      {/* 运行时：摘要 + 环境健康 + 已装清单。三块都常驻挂载、按需显隐——
          条件渲染会在切换时卸载组件，进行中的预设安装日志与流式进度会一起丢掉 */}
      <div
        role="tabpanel"
        id="env-pane-runtime"
        aria-labelledby="env-tab-runtime"
        style={{ display: pane === "runtime" ? undefined : "none" }}
      >
        <ToolCatalogPanel
          active={active && pane === "runtime"}
          read={catalogRead}
          pythonRead={pythonRead}
          onShowPython={() => {
            setDetailTool("python");
            setPythonVisited(true);
            setPythonFocus((n) => n + 1);
          }}
          onShowNode={() => {
            setDetailTool("node");
            setCatalogNodeFocus((n) => n + 1);
          }}
          onShowCompatibility={openCompatibility}
        />
        <div hidden={detailTool !== "node"}>
          <NodeInventoryPanel
            active={active && pane === "runtime" && detailTool === "node"}
            focusRequest={nodeDetailsRequest + catalogNodeFocus}
            onReadState={setCatalogRead}
            refreshRequest={inventoryRevision + inventoryChange}
          />
        </div>
        {pythonVisited && (
          <div hidden={detailTool !== "python"}>
            <Suspense fallback={<p role="status">正在加载 Python 清单…</p>}>
              <PythonInventoryPanel
                active={active && pane === "runtime" && detailTool === "python"}
                focusRequest={pythonFocus}
                refreshRequest={inventoryRevision + inventoryChange}
                onReadState={setPythonRead}
              />
            </Suspense>
          </div>
        )}
        <details
          className="runtime-management"
          open={compatibilityOpen}
          onToggle={(event) => {
            setCompatibilityOpen(event.currentTarget.open);
            if (event.currentTarget.open) setCompatibilityVisited(true);
          }}
        >
          <summary>更多工具与兼容渠道</summary>
          <p className="runtime-management-note">
            这里保留旧版工具与系统软件渠道，尚未全部接入统一来源、计划和项目验证。渠道信息不代表当前版本可安装或项目已生效。
          </p>
          {compatibilityVisited && (
            <Suspense fallback={<p role="status">正在加载兼容渠道…</p>}>
              <SoftwareView
                active={active && pane === "runtime" && compatibilityOpen}
                onInventoryChanged={() => setInventoryRevision((n) => n + 1)}
                onShowNode={() => {
                  setDetailTool("node");
                  setCatalogNodeFocus((n) => n + 1);
                }}
              />
            </Suspense>
          )}
        </details>
        <details
          className="runtime-management"
          open={managedExpanded}
          onToggle={(event) => setManagedExpanded(event.currentTarget.open)}
        >
          <summary>其他运行时与管理操作</summary>
          <p className="runtime-management-note">
            Node.js
            的来源、版本和路径以上方清单为准。下方保留各渠道的安装、全局配置与接入操作；配置成功不代表项目或终端已生效。
          </p>
          <div className="runtime-management-toolbar">
            <button className="btn-ghost" disabled={loading} onClick={() => void refresh()}>
              {loading ? "读取管理列表中…" : "刷新管理列表"}
            </button>
          </div>
          {cards.length > 0 && (
            <div
              className={`banner ${envSnap && envSnap.conflicts.length > 0 ? "warn" : "info"}`}
              style={{ cursor: envSnap && envSnap.conflicts.length > 0 ? "pointer" : "default" }}
              onClick={() => envSnap && envSnap.conflicts.length > 0 && setPane("vars")}
            >
              本机已装 {cards.length} 个运行时
              {envSnap && envSnap.conflicts.length > 0
                ? ` · ${envSnap.conflicts.length} 个环境变量同名冲突 · 点击查看`
                : ""}
            </div>
          )}

          {/* 环境健康：装好 ≠ 生效——mise 可用性/shell 接入/PATH/托管在此巡检并可就地修复 */}
          {managedExpanded && (
            <DoctorPanel
              onFixed={() => {
                setInventoryRevision((revision) => revision + 1);
                void refresh();
              }}
            />
          )}

          {error && (
            <div className="banner error" role="alert">
              <span>⚠ {error}</span>
              <button
                className="btn-ghost"
                onClick={() => setError(null)}
                aria-label="关闭管理操作提示"
              >
                关闭
              </button>
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
              {error
                ? "读取未完成，安装情况未知。可重试读取管理列表。"
                : "未从这些管理入口发现运行时记录。可展开本页的「更多工具与兼容渠道」核对其他来源。"}
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
                const quickSetGlobal =
                  tool.versions.length === 1 && tool.active_versions.length === 0;
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
        </details>
      </div>

      <div
        role="tabpanel"
        id="env-pane-vars"
        aria-labelledby="env-tab-vars"
        style={{ display: pane === "vars" ? undefined : "none" }}
      >
        <EnvPanel snap={envSnap} reload={loadEnvSnap} />
      </div>

      <div
        role="tabpanel"
        id="env-pane-preset"
        aria-labelledby="env-tab-preset"
        style={{ display: pane === "preset" ? undefined : "none" }}
      >
        {/* 整机搭建：选一套配方装到整机，不依赖任何项目 */}
        <MachinePresetSection
          onNavigate={onNavigate}
          installApi={machineInstall}
          channelBusy={restoreBusy}
        />
        {/* 整机迁移：本机现状的打包与还原（含全局 env 与 brew 清单） */}
        <SnapshotPanel
          onRestored={() => {
            setInventoryRevision((revision) => revision + 1);
            loadEnvSnap();
            void refresh();
          }}
          machineInstallBusy={machineInstall.installBusy}
          onRestoreBusyChange={setRestoreBusy}
        />
      </div>

      {installFor && (
        <InstallDialog
          tool={installFor}
          external={externalByTool.get(installFor.name)}
          onClose={() => setInstallFor(null)}
          onDone={async () => {
            setInventoryRevision((revision) => revision + 1);
            await refresh();
            setNotice("安装完成");
          }}
        />
      )}
    </div>
  );
}
