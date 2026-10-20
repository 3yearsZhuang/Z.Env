import { useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronRight,
  Copy,
  Download,
  ExternalLink,
  LoaderCircle,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import {
  CAT_KEYS_ALL,
  KNOWN_RUNTIMES,
  SOFTWARE,
  TOOL_ICON_PATHS,
  categoryOf,
} from "../data/catalog";
import {
  detectSystemInstalled,
  errorMessage,
  getEnvInfo,
  installSystemPackage,
  listTools,
  packageCatalog,
  type ProviderPackage,
  onSysInstallProgress,
  type ToolInfo,
} from "../api";
import InstallDialog from "./InstallDialog";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";
import "./SoftwareView.css";

type LoadState = { status: "loading" | "ready" | "error"; error?: string };
type CopyFeedback = { ok: boolean; message: string; command: string };

function useCopyFeedback() {
  const [feedback, setFeedback] = useState<CopyFeedback | null>(null);
  const attempt = useRef(0);
  const copy = async (pkg: ProviderPackage) => {
    const id = ++attempt.current;
    const command = pkgCommand(pkg);
    setFeedback(null);
    try {
      await navigator.clipboard.writeText(command);
      if (id === attempt.current)
        setFeedback({
          ok: true,
          message: `已复制 ${pkg.tool} 的 ${pkg.manager} 安装命令`,
          command,
        });
    } catch {
      if (id === attempt.current)
        setFeedback({ ok: false, message: "复制失败，请手动选择下方命令复制", command });
    }
  };
  return { feedback, copy, dismiss: () => setFeedback(null) };
}

function CopyNotice({
  feedback,
  onDismiss,
}: {
  feedback: CopyFeedback | null;
  onDismiss: () => void;
}) {
  if (!feedback) return null;
  return (
    <div
      className={`support-notice ${feedback.ok ? "support-notice-success" : "support-notice-error"}`}
      role={feedback.ok ? "status" : "alert"}
    >
      {feedback.ok ? (
        <Check size={16} aria-hidden="true" />
      ) : (
        <AlertCircle size={16} aria-hidden="true" />
      )}
      <div className="support-notice-copy">
        <span>{feedback.message}</span>
        {!feedback.ok && <code className="support-copy-command">{feedback.command}</code>}
      </div>
      <button
        type="button"
        className="support-icon-button"
        onClick={onDismiss}
        aria-label="关闭复制提示"
      >
        <X size={15} aria-hidden="true" />
      </button>
    </div>
  );
}

function RetryNotice({
  title,
  error,
  onRetry,
  disabled = false,
}: {
  title: string;
  error?: string;
  onRetry: () => void;
  disabled?: boolean;
}) {
  return (
    <div className="support-notice support-notice-error" role="alert">
      <AlertCircle size={16} aria-hidden="true" />
      <div className="support-notice-copy">
        <strong>{title}</strong>
        {error && <span>{error}</span>}
      </div>
      <button type="button" className="support-text-button" disabled={disabled} onClick={onRetry}>
        <RefreshCw size={13} aria-hidden="true" />
        重试
      </button>
    </div>
  );
}

/** 官方下载和包管理器命令都保留在渠道弹窗中。 */
export function ChannelDialog({
  name,
  url,
  desc,
  available,
  packages,
  channelState = { status: "ready" },
  onRetry,
  onClose,
}: {
  name: string;
  url?: string;
  desc?: string;
  available: Set<string>;
  packages: ProviderPackage[];
  channelState?: LoadState;
  onRetry?: () => void;
  onClose: () => void;
}) {
  const { feedback, copy, dismiss } = useCopyFeedback();
  const channels = packages.filter((pkg) => pkg.tool === name && available.has(pkg.manager));
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-[460px] p-0">
        <div className="dialog-head">
          <DialogTitle>{name}</DialogTitle>
        </div>
        <div className="dialog-body">
          <DialogDescription className="dialog-tip">
            {desc || "选择适合你的安装渠道。"}
          </DialogDescription>
          {url && (
            <a className="btn support-official-link" href={url} target="_blank" rel="noreferrer">
              前往官方下载
              <ExternalLink size={14} aria-hidden="true" />
            </a>
          )}
          <div className="field-label">通过包管理器安装</div>
          <p className="support-channel-hint">复制命令后，在终端中执行。</p>
          {channelState.status === "loading" && (
            <p className="support-channel-hint" role="status">
              正在检测可用安装渠道…
            </p>
          )}
          {channelState.status === "error" && (
            <RetryNotice
              title="安装渠道暂不可用"
              error={channelState.error}
              onRetry={() => onRetry?.()}
            />
          )}
          {channelState.status === "ready" &&
            (channels.length ? (
              <div className="support-channel-list">
                {channels.map((pkg) => (
                  <button
                    type="button"
                    key={`${pkg.manager}:${pkg.kind}:${pkg.id}`}
                    className="support-channel-command"
                    onClick={() => void copy(pkg)}
                    aria-label={`复制 ${name} 的 ${pkg.manager} 安装命令`}
                  >
                    <span className="support-channel-name">{pkg.manager}</span>
                    <code>{pkgCommand(pkg)}</code>
                    <Copy size={15} aria-hidden="true" />
                  </button>
                ))}
              </div>
            ) : (
              <p className="support-channel-empty">
                未检测到适用于此软件的包管理器。
                {url ? "可通过上方官网安装。" : "请先安装可用的包管理器。"}
              </p>
            ))}
          <CopyNotice feedback={feedback} onDismiss={dismiss} />
        </div>
        <div className="dialog-foot">
          <button type="button" className="btn-ghost" onClick={onClose}>
            关闭
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

interface Item {
  name: string;
  kind: "runtime" | "download";
  cat: string;
  url?: string;
  desc?: string;
}

function SoftIcon({ name }: { name: string }) {
  const [failed, setFailed] = useState(false);
  return (
    <span className="support-tool-icon" aria-hidden="true">
      {failed ? (
        name.slice(0, 2).toUpperCase()
      ) : (
        <img
          src={TOOL_ICON_PATHS[name] || `/tools/${name}.svg`}
          alt=""
          draggable={false}
          onError={() => setFailed(true)}
        />
      )}
    </span>
  );
}

/** 命令展示与实际执行使用同一份规范包标识。 */
function pkgCommand(pkg: ProviderPackage): string {
  if (pkg.manager === "brew") return `brew install --${pkg.kind} ${pkg.id}`;
  if (pkg.manager === "winget") return `winget install --id ${pkg.id} --exact`;
  if (pkg.manager === "apt") return `sudo apt-get install -y ${pkg.id}`;
  return `sudo pacman -S --noconfirm ${pkg.id}`;
}

export default function SoftwareView({
  active = true,
  onShowNode,
  onInventoryChanged,
}: {
  active?: boolean;
  onShowNode?: () => void;
  onInventoryChanged?: () => void;
}) {
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState("全部");
  const [kind, setKind] = useState("all");
  const [installFor, setInstallFor] = useState<ToolInfo | null>(null);
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [runtimeState, setRuntimeState] = useState<LoadState>({ status: "loading" });
  const [runtimeAttempt, setRuntimeAttempt] = useState(0);
  const [packages, setPackages] = useState<ProviderPackage[]>([]);
  const [catalogState, setCatalogState] = useState<LoadState>({ status: "loading" });
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  const [available, setAvailable] = useState<Set<string>>(new Set());
  const [envState, setEnvState] = useState<LoadState>({ status: "loading" });
  const [managerStates, setManagerStates] = useState<Record<string, LoadState>>({});
  const [systemAttempt, setSystemAttempt] = useState(0);
  const [sysInstalled, setSysInstalled] = useState<Record<string, Set<string>>>({});
  const [channel, setChannel] = useState<Item | null>(null);
  const [sysJob, setSysJob] = useState<{
    manager: string;
    name: string;
    jobId: string;
    status: "run" | "ok" | "err";
    line: string;
  } | null>(null);
  const running = useRef(false);
  const mounted = useRef(true);
  const progressUnlisten = useRef<(() => void) | undefined>(undefined);
  const { feedback, copy, dismiss } = useCopyFeedback();
  const busy = sysJob?.status === "run";

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      progressUnlisten.current?.();
    };
  }, []);

  useEffect(() => {
    let alive = true;
    setCatalogState({ status: "loading" });
    packageCatalog()
      .then((result) => {
        if (!alive) return;
        setPackages(result);
        setCatalogState({ status: "ready" });
      })
      .catch((error) => {
        if (alive) setCatalogState({ status: "error", error: errorMessage(error) });
      });
    return () => {
      alive = false;
    };
  }, [catalogAttempt]);

  useEffect(() => {
    if (!active) return;
    let alive = true;
    setRuntimeState({ status: "loading" });
    listTools()
      .then((result) => {
        if (!alive) return;
        setTools(result);
        setRuntimeState({ status: "ready" });
      })
      .catch((error) => {
        if (alive) setRuntimeState({ status: "error", error: errorMessage(error) });
      });
    return () => {
      alive = false;
    };
  }, [active, runtimeAttempt]);

  useEffect(() => {
    if (!active) return;
    let alive = true;
    setEnvState({ status: "loading" });
    setManagerStates({});
    getEnvInfo()
      .then(async (env) => {
        if (!alive) return;
        const managers = env.pkg.filter((pkg) => pkg.available).map((pkg) => pkg.name);
        setAvailable(new Set(managers));
        setManagerStates(
          Object.fromEntries(managers.map((manager) => [manager, { status: "loading" }])),
        );
        setEnvState({ status: "ready" });
        await Promise.all(
          managers.map(async (manager) => {
            try {
              const names = await detectSystemInstalled(manager);
              if (!alive) return;
              setSysInstalled((previous) => ({
                ...previous,
                [manager]: new Set(names.map((name) => name.toLowerCase())),
              }));
              setManagerStates((previous) => ({ ...previous, [manager]: { status: "ready" } }));
            } catch (error) {
              if (alive)
                setManagerStates((previous) => ({
                  ...previous,
                  [manager]: { status: "error", error: errorMessage(error) },
                }));
            }
          }),
        );
      })
      .catch((error) => {
        if (alive) setEnvState({ status: "error", error: errorMessage(error) });
      });
    return () => {
      alive = false;
    };
  }, [active, systemAttempt]);

  const doInstall = async (pkg: ProviderPackage) => {
    if (
      running.current ||
      catalogState.status !== "ready" ||
      envState.status !== "ready" ||
      managerStates[pkg.manager]?.status !== "ready" ||
      sysInstalled[pkg.manager]?.has(pkg.id.toLowerCase())
    )
      return;
    running.current = true;
    const jobId = crypto.randomUUID();
    const base = { manager: pkg.manager, name: pkg.id, jobId };
    setSysJob({ ...base, status: "run", line: "正在准备安装…" });
    let unlisten: (() => void) | undefined;
    try {
      try {
        unlisten = await onSysInstallProgress((progress) => {
          if (mounted.current && progress.jobId === jobId)
            setSysJob((previous) =>
              previous?.jobId === jobId && previous.status === "run"
                ? { ...previous, line: progress.line }
                : previous,
            );
        });
        progressUnlisten.current = unlisten;
      } catch {
        if (mounted.current)
          setSysJob({ ...base, status: "run", line: "实时日志暂不可用，正在等待安装结果…" });
      }
      if (!mounted.current) return;
      await installSystemPackage(pkg.manager, pkg.id, pkg.kind, jobId);
      if (!mounted.current) return;
      onInventoryChanged?.();
      setSysJob({ ...base, status: "ok", line: "安装完成，正在重新检测本机状态。" });
      setSystemAttempt((value) => value + 1);
      setRuntimeAttempt((value) => value + 1);
    } catch (error) {
      if (mounted.current) setSysJob({ ...base, status: "err", line: errorMessage(error) });
    } finally {
      running.current = false;
      unlisten?.();
      progressUnlisten.current = undefined;
    }
  };

  const items = useMemo<Item[]>(
    () => [
      ...KNOWN_RUNTIMES.map((name): Item => ({ name, kind: "runtime", cat: categoryOf(name) })),
      ...SOFTWARE.map((item): Item => ({ ...item, kind: "download", cat: categoryOf(item.name) })),
    ],
    [],
  );
  const packagesByTool = useMemo(() => {
    const byTool = new Map<string, ProviderPackage[]>();
    packages.forEach((pkg) => byTool.set(pkg.tool, [...(byTool.get(pkg.tool) || []), pkg]));
    return byTool;
  }, [packages]);
  const toolsByName = useMemo(() => new Map(tools.map((tool) => [tool.name, tool])), [tools]);
  const searchWords = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const filtered = items.filter((item) => {
    if (cat !== "全部" && item.cat !== cat) return false;
    if (kind !== "all" && item.kind !== kind) return false;
    const searchable = [
      item.name,
      item.desc || "",
      item.cat,
      ...(packagesByTool.get(item.name) || []).flatMap((pkg) => [pkg.manager, pkg.id]),
    ]
      .join(" ")
      .toLowerCase();
    return searchWords.every((word) => searchable.includes(word));
  });
  const clearFilters = () => {
    setQuery("");
    setCat("全部");
    setKind("all");
  };
  const refreshing =
    runtimeState.status === "loading" ||
    envState.status === "loading" ||
    Object.values(managerStates).some((state) => state.status === "loading");
  const refresh = () => {
    setRuntimeAttempt((value) => value + 1);
    setSystemAttempt((value) => value + 1);
    if (catalogState.status === "error") setCatalogAttempt((value) => value + 1);
  };
  const channelState: LoadState =
    catalogState.status === "error"
      ? catalogState
      : envState.status === "error"
        ? envState
        : catalogState.status === "loading" || envState.status === "loading"
          ? { status: "loading" }
          : { status: "ready" };

  return (
    <div className="view support-view">
      <div className="view-head support-heading">
        <div>
          <div className="view-eyebrow">SOFTWARE CATALOG</div>
          <h2>兼容渠道</h2>
          <p className="view-sub">发现开发工具，选择合适的安装渠道。</p>
        </div>
        <button
          type="button"
          className="btn-ghost support-refresh"
          onClick={refresh}
          disabled={busy || refreshing}
        >
          <RefreshCw
            size={15}
            className={refreshing ? "support-spinning" : undefined}
            aria-hidden="true"
          />
          {refreshing ? "检测中" : "刷新状态"}
        </button>
      </div>

      {catalogState.status === "error" && (
        <RetryNotice
          title="安装渠道加载失败"
          error={catalogState.error}
          onRetry={() => setCatalogAttempt((value) => value + 1)}
          disabled={busy}
        />
      )}
      {runtimeState.status === "error" && (
        <RetryNotice
          title="运行时安装状态检测失败"
          error={runtimeState.error}
          onRetry={() => setRuntimeAttempt((value) => value + 1)}
          disabled={busy}
        />
      )}
      {envState.status === "error" && (
        <RetryNotice
          title="包管理器检测失败"
          error={envState.error}
          onRetry={() => setSystemAttempt((value) => value + 1)}
          disabled={busy}
        />
      )}
      {Object.entries(managerStates)
        .filter(([, state]) => state.status === "error")
        .map(([manager, state]) => (
          <RetryNotice
            key={manager}
            title={`${manager} 安装状态检测失败`}
            error={state.error}
            onRetry={() => setSystemAttempt((value) => value + 1)}
            disabled={busy}
          />
        ))}
      <CopyNotice feedback={feedback} onDismiss={dismiss} />

      {sysJob && (
        <div
          className={`support-job support-job-${sysJob.status}`}
          role={sysJob.status === "err" ? "alert" : "status"}
        >
          {busy ? (
            <LoaderCircle size={18} className="support-spinning" aria-hidden="true" />
          ) : sysJob.status === "ok" ? (
            <Check size={18} aria-hidden="true" />
          ) : (
            <AlertCircle size={18} aria-hidden="true" />
          )}
          <div className="support-notice-copy">
            <strong>
              {sysJob.manager} · {sysJob.name}
              <span className="support-job-label">
                {busy ? "安装中" : sysJob.status === "ok" ? "安装完成" : "安装失败"}
              </span>
            </strong>
            <code>{sysJob.line}</code>
            {busy && <span>安装完成后可继续安装其他工具。</span>}
          </div>
          <button
            type="button"
            className="support-icon-button"
            onClick={() => setSysJob(null)}
            disabled={busy}
            aria-label="关闭安装结果"
          >
            <X size={16} aria-hidden="true" />
          </button>
        </div>
      )}

      <div className="support-toolbar">
        <label className="support-search-field">
          <Search size={16} aria-hidden="true" />
          <input
            className="input support-search"
            type="search"
            aria-label="搜索支持的软件或运行时"
            placeholder="搜索名称、用途或安装渠道…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <select
          className="input support-kind-filter"
          aria-label="工具类型"
          value={kind}
          onChange={(event) => setKind(event.target.value)}
        >
          <option value="all">全部类型</option>
          <option value="runtime">运行时</option>
          <option value="download">软件</option>
        </select>
        <span className="support-result-count" role="status">
          {filtered.length} / {items.length} 个工具
        </span>
      </div>
      <div className="filter-chips support-chips" role="group" aria-label="工具分类">
        {["全部", ...CAT_KEYS_ALL].map((category) => (
          <button
            type="button"
            key={category}
            className={`chip ${cat === category ? "active" : ""}`}
            onClick={() => setCat(category)}
            aria-pressed={cat === category}
          >
            {category}
          </button>
        ))}
      </div>

      <div className="support-grid">
        {filtered.map((item) => {
          const showNodeDetails =
            item.kind === "runtime" && ["node", "nodejs"].includes(item.name) && onShowNode;
          const tool = toolsByName.get(item.name);
          const isInstalled =
            runtimeState.status === "ready" && tool?.versions.some((version) => version.installed);
          const runtimeLabel = showNodeDetails
            ? "查看本机实例"
            : runtimeState.status === "loading"
              ? "检测中"
              : runtimeState.status === "error"
                ? "状态未知"
                : isInstalled
                  ? "已安装"
                  : "可安装";
          const channels = (packagesByTool.get(item.name) || []).filter((pkg) =>
            available.has(pkg.manager),
          );
          return (
            <article className="support-card" key={item.name} aria-label={item.name}>
              <button
                type="button"
                className="support-card-main"
                onClick={() =>
                  item.kind === "download"
                    ? setChannel(item)
                    : showNodeDetails
                      ? onShowNode?.()
                      : setInstallFor(
                          tool || { name: item.name, versions: [], active_versions: [] },
                        )
                }
                disabled={
                  busy ||
                  (item.kind === "runtime" && !showNodeDetails && runtimeState.status !== "ready")
                }
                aria-label={
                  item.kind === "download"
                    ? `查看 ${item.name} 的安装渠道`
                    : showNodeDetails
                      ? `查看 ${item.name} 的本机实例`
                      : `查看 ${item.name} 的运行时版本`
                }
              >
                <SoftIcon name={item.name} />
                <span className="support-card-copy">
                  <span className="support-card-name">{item.name}</span>
                  <span className="support-card-description">
                    {showNodeDetails
                      ? "查看所有来源的安装实例、路径与验证结果"
                      : item.desc || "通过 mise 管理运行时版本"}
                  </span>
                </span>
                <ChevronRight className="support-card-arrow" size={16} aria-hidden="true" />
              </button>
              <div className="support-card-meta">
                <span>{item.cat}</span>
                {item.kind === "runtime" ? (
                  <span className={`support-badge ${isInstalled ? "support-badge-installed" : ""}`}>
                    {runtimeLabel}
                  </span>
                ) : (
                  <span className="support-badge">官方下载</span>
                )}
              </div>
              {channelState.status !== "ready" ? (
                <div className="support-card-state">
                  {channelState.status === "loading" ? "正在加载安装渠道…" : "安装渠道待重试"}
                </div>
              ) : channels.length > 0 ? (
                <div className="support-package-list">
                  {channels.map((pkg) => {
                    const state =
                      envState.status !== "ready" ? envState : managerStates[pkg.manager];
                    const isHere =
                      state?.status === "ready" &&
                      sysInstalled[pkg.manager]?.has(pkg.id.toLowerCase());
                    const installing =
                      busy && sysJob.manager === pkg.manager && sysJob.name === pkg.id;
                    const label = installing
                      ? "安装中"
                      : state?.status === "error"
                        ? "检测失败"
                        : state?.status !== "ready"
                          ? "检测中"
                          : isHere
                            ? "已安装"
                            : "安装";
                    return (
                      <div className="support-package" key={`${pkg.manager}:${pkg.kind}:${pkg.id}`}>
                        <button
                          type="button"
                          className={`support-package-install ${isHere ? "support-package-installed" : ""}`}
                          disabled={busy || isHere || state?.status !== "ready"}
                          onClick={() => void doInstall(pkg)}
                          aria-label={`${pkg.manager} ${label} ${item.name}`}
                          title={pkgCommand(pkg)}
                        >
                          {installing ? (
                            <LoaderCircle
                              size={12}
                              className="support-spinning"
                              aria-hidden="true"
                            />
                          ) : isHere ? (
                            <Check size={12} aria-hidden="true" />
                          ) : (
                            <Download size={12} aria-hidden="true" />
                          )}
                          <span>{pkg.manager}</span>
                          <span className="support-package-state">{label}</span>
                        </button>
                        <button
                          type="button"
                          className="support-package-copy"
                          onClick={() => void copy(pkg)}
                          aria-label={`复制 ${item.name} 的 ${pkg.manager} 安装命令`}
                          title="复制安装命令"
                        >
                          <Copy size={13} aria-hidden="true" />
                        </button>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="support-card-state">
                  {item.kind === "runtime" ? "使用 mise 选择版本安装" : "查看官方安装渠道"}
                </div>
              )}
            </article>
          );
        })}
      </div>
      {filtered.length === 0 && (
        <div className="support-empty">
          <Search size={27} aria-hidden="true" />
          <h2>没有找到匹配的工具</h2>
          <p>试试其他名称、用途，或清除当前筛选条件。</p>
          <button type="button" className="btn-ghost" onClick={clearFilters}>
            清除筛选
          </button>
        </div>
      )}
      {installFor && (
        <InstallDialog
          tool={installFor}
          onClose={() => setInstallFor(null)}
          onDone={() => {
            setInstallFor(null);
            setRuntimeAttempt((value) => value + 1);
            onInventoryChanged?.();
          }}
        />
      )}
      {channel && (
        <ChannelDialog
          name={channel.name}
          url={channel.url}
          desc={channel.desc}
          available={available}
          packages={packages}
          channelState={channelState}
          onRetry={() => {
            if (catalogState.status === "error") setCatalogAttempt((value) => value + 1);
            if (envState.status === "error") setSystemAttempt((value) => value + 1);
          }}
          onClose={() => setChannel(null)}
        />
      )}
    </div>
  );
}
