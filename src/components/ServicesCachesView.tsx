import { useCallback, useEffect, useRef, useState } from "react";
import {
  Activity,
  AlertCircle,
  CheckCircle2,
  Database,
  HardDrive,
  Info,
  LoaderCircle,
  Play,
  RefreshCw,
  RotateCw,
  Server,
  Square,
  Trash2,
} from "lucide-react";
import {
  serviceList,
  serviceAction,
  cacheList,
  cacheClean,
  errorMessage,
  type CacheInfo,
} from "../api";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";
import "./ServicesCachesView.css";

const SERVICE_ACTIONS = { start: "启动", restart: "重启", stop: "停止" } as const;
type ServiceAction = keyof typeof SERVICE_ACTIONS;

// 与 caches.rs 中的官方清理命令保持一致；仅用于确认文案，执行仍由后端控制。
const CLEAN_COMMANDS: Record<string, string> = {
  brew: "brew cleanup -s",
  mise: "mise cache clear",
  npm: "npm cache clean --force",
  yarn: "yarn cache clean",
  pip: "python3 -m pip cache purge",
  uv: "uv cache clean",
  gobuild: "go clean -cache",
};

function stateView(state: string): { label: string; tone: string } {
  switch (state) {
    case "started":
      return { label: "运行中", tone: "success" };
    case "error":
      return { label: "异常", tone: "error" };
    case "stopped":
      return { label: "已停止", tone: "muted" };
    case "unknown":
      return { label: "状态未知", tone: "muted" };
    case "scheduled":
      return { label: "等待启动", tone: "muted" };
    default:
      return { label: state || "状态未知", tone: "muted" };
  }
}

function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

/** 独立维护读取状态；刷新时保留旧数据，忽略卸载或过期请求的返回。 */
function useResource<T>(fetcher: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const mounted = useRef(false);

  const reload = useCallback(async () => {
    if (!mounted.current) return;
    const request = ++generation.current;
    setLoading(true);
    setError("");
    try {
      const next = await fetcher();
      if (mounted.current && request === generation.current) setData(next);
    } catch (e) {
      if (mounted.current && request === generation.current) setError(errorMessage(e));
    } finally {
      if (mounted.current && request === generation.current) setLoading(false);
    }
  }, [fetcher]);

  useEffect(() => {
    mounted.current = true;
    void reload();
    return () => {
      mounted.current = false;
      generation.current += 1;
    };
  }, [reload]);

  return { data, loading, error, reload };
}

function LoadError({
  title,
  message,
  hasData,
  onRetry,
  retryLabel,
  disabled,
}: {
  title: string;
  message: string;
  hasData: boolean;
  onRetry: () => void;
  retryLabel: string;
  disabled: boolean;
}) {
  return (
    <div className="ops-feedback ops-feedback-error" role="alert">
      <AlertCircle size={17} aria-hidden="true" />
      <div>
        <strong>{title}</strong>
        <p>{message}</p>
        {hasData && <p>当前展示上次读取的结果，请重试以获取最新状态。</p>}
      </div>
      <button className="btn ops-button" disabled={disabled} onClick={onRetry}>
        <RefreshCw size={14} aria-hidden="true" />
        {retryLabel}
      </button>
    </div>
  );
}

export default function ServicesCachesView() {
  const services = useResource(serviceList);
  const caches = useResource(cacheList);
  const [serviceError, setServiceError] = useState("");
  const [serviceNotice, setServiceNotice] = useState("");
  const [cacheNotice, setCacheNotice] = useState("");
  const [busy, setBusy] = useState<{ key: string; action: ServiceAction } | null>(null);
  const [cleanBusy, setCleanBusy] = useState(false);
  const [cleanTarget, setCleanTarget] = useState<CacheInfo | null>(null);
  const [cleanError, setCleanError] = useState("");
  const serviceOperation = useRef(false);
  const cacheOperation = useRef(false);
  const cleanTrigger = useRef<HTMLButtonElement | null>(null);
  const cacheRefresh = useRef<HTMLButtonElement | null>(null);
  const cancelButton = useRef<HTMLButtonElement | null>(null);

  const serviceItems = services.data?.services ?? [];
  const runningCount = serviceItems.filter((service) => service.state === "started").length;
  const errorCount = serviceItems.filter((service) => service.state === "error").length;
  const existingCaches = (caches.data ?? []).filter((cache) => cache.exists);
  const totalBytes = existingCaches.reduce((sum, cache) => sum + cache.sizeBytes, 0);
  const approx = existingCaches.some((cache) => cache.approx);
  const cleanableCount = existingCaches.filter((cache) => cache.cleanable).length;

  async function handleAction(manager: string, name: string, action: ServiceAction) {
    if (serviceOperation.current || services.loading) return;
    serviceOperation.current = true;
    setBusy({ key: `${manager}::${name}`, action });
    setServiceError("");
    setServiceNotice("");
    try {
      await serviceAction(manager, name, action);
      setServiceNotice(`${name} 的${SERVICE_ACTIONS[action]}命令已完成。`);
      await services.reload();
    } catch (e) {
      setServiceError(`${name} ${SERVICE_ACTIONS[action]}失败：${errorMessage(e)}`);
    } finally {
      serviceOperation.current = false;
      setBusy(null);
    }
  }

  function requestClean(cache: CacheInfo, trigger: HTMLButtonElement) {
    if (cacheOperation.current || caches.loading) return;
    cleanTrigger.current = trigger;
    setCleanTarget(cache);
    setCleanError("");
  }

  async function handleClean() {
    if (!cleanTarget || cacheOperation.current) return;
    cacheOperation.current = true;
    setCleanBusy(true);
    setCleanError("");
    setCacheNotice("");
    try {
      await cacheClean(cleanTarget.id);
      setCacheNotice(`${cleanTarget.name} 的清理命令已完成。`);
      await caches.reload();
      setCleanTarget(null);
    } catch (e) {
      setCleanError(errorMessage(e));
    } finally {
      cacheOperation.current = false;
      setCleanBusy(false);
    }
  }

  return (
    <div className="view ops-view">
      <div className="view-head">
        <div>
          <h1>服务与缓存</h1>
          <p className="view-sub">管理开发服务，查看缓存占用，让本地环境保持轻盈。</p>
        </div>
      </div>

      <div className="ops-summary" aria-label="服务与缓存概览">
        <div className="ops-stat">
          <span className="ops-stat-icon">
            <Activity size={19} aria-hidden="true" />
          </span>
          <div>
            <span className="ops-stat-label">运行中的服务</span>
            <strong>
              {services.data ? runningCount : "—"}
              <small>{services.data ? ` / ${serviceItems.length}` : ""}</small>
            </strong>
            <span className="ops-stat-caption">
              {services.data
                ? errorCount > 0
                  ? `${errorCount} 项服务异常`
                  : "已发现的本地服务"
                : services.loading
                  ? "正在读取服务状态"
                  : "服务状态暂不可用"}
            </span>
          </div>
        </div>
        <div className="ops-stat">
          <span className="ops-stat-icon">
            <HardDrive size={19} aria-hidden="true" />
          </span>
          <div>
            <span className="ops-stat-label">缓存占用</span>
            <strong>{caches.data ? `${approx ? "≥ " : ""}${humanSize(totalBytes)}` : "—"}</strong>
            <span className="ops-stat-caption">
              {caches.data
                ? `${existingCaches.length} 项已生成缓存${approx ? " · 体积为统计下限" : ""}`
                : caches.loading
                  ? "正在统计缓存体积"
                  : "缓存数据暂不可用"}
            </span>
          </div>
        </div>
        <div className="ops-stat">
          <span className="ops-stat-icon">
            <Trash2 size={19} aria-hidden="true" />
          </span>
          <div>
            <span className="ops-stat-label">支持清理</span>
            <strong>
              {caches.data ? cleanableCount : "—"}
              <small> 项缓存</small>
            </strong>
            <span className="ops-stat-caption">通过工具的官方命令清理</span>
          </div>
        </div>
      </div>

      <section className="ops-panel" aria-labelledby="ops-services-title">
        <div className="ops-panel-head">
          <div className="ops-section-title">
            <Server size={19} aria-hidden="true" />
            <div>
              <h2 id="ops-services-title">本地服务</h2>
              <p>管理 Homebrew 与 systemd 用户级服务。</p>
            </div>
          </div>
          <button
            className="btn ops-button"
            onClick={() => services.reload()}
            disabled={services.loading || busy !== null}
            aria-label="刷新服务"
          >
            <RefreshCw
              size={14}
              className={services.loading ? "ops-spin" : ""}
              aria-hidden="true"
            />
            {services.loading ? "读取中" : "刷新"}
          </button>
        </div>

        {services.error && (
          <LoadError
            title="服务读取失败"
            message={services.error}
            hasData={services.data !== null}
            onRetry={() => services.reload()}
            retryLabel="重试服务"
            disabled={services.loading || busy !== null}
          />
        )}
        {serviceError && (
          <div className="ops-feedback ops-feedback-error" role="alert">
            <AlertCircle size={17} aria-hidden="true" />
            <p>{serviceError}</p>
          </div>
        )}
        {serviceNotice && (
          <div className="ops-feedback ops-feedback-success" role="status">
            <CheckCircle2 size={17} aria-hidden="true" />
            <p>{serviceNotice}</p>
          </div>
        )}
        {(services.data?.notes ?? []).map((note) => (
          <div className="ops-note" key={note}>
            <Info size={15} aria-hidden="true" />
            <p>{note}</p>
          </div>
        ))}

        <div aria-busy={services.loading}>
          {services.loading && !services.data && (
            <div className="ops-empty" role="status">
              <LoaderCircle className="ops-spin" size={23} aria-hidden="true" />
              <strong>正在读取本地服务</strong>
              <p>首次探测服务管理器可能需要一些时间。</p>
            </div>
          )}
          {services.data && serviceItems.length === 0 && (
            <div className="ops-empty">
              <Server size={26} aria-hidden="true" />
              <strong>暂无可管理的服务</strong>
              <p>安装并配置 Homebrew 或 systemd 用户级服务后，可在这里管理。</p>
            </div>
          )}
          {serviceItems.length > 0 && (
            <ul className="ops-list" aria-label="本地服务列表">
              {serviceItems.map((service) => {
                const state = stateView(service.state);
                const key = `${service.manager}::${service.name}`;
                const rowBusy = busy?.key === key;
                const running = service.state === "started";
                return (
                  <li className="ops-row" key={key}>
                    <span className={`ops-item-icon ops-item-icon-${state.tone}`}>
                      <Server size={18} aria-hidden="true" />
                    </span>
                    <div className="ops-item-info">
                      <div className="ops-item-heading">
                        <h3>{service.name}</h3>
                        <span className={`ops-badge ops-badge-${state.tone}`}>
                          <span className="ops-status-dot" />
                          {state.label}
                        </span>
                      </div>
                      <p className="ops-item-meta">
                        {service.manager === "brew" ? "Homebrew" : service.manager}
                        {service.pid ? ` · PID ${service.pid}` : ""}
                      </p>
                    </div>
                    <div className="ops-actions">
                      {rowBusy ? (
                        <span className="ops-operation" role="status">
                          <LoaderCircle size={15} className="ops-spin" aria-hidden="true" />
                          正在{SERVICE_ACTIONS[busy.action]}…
                        </span>
                      ) : (
                        <>
                          {running ? (
                            <>
                              <button
                                className="btn ops-button"
                                disabled={busy !== null || services.loading}
                                onClick={() =>
                                  handleAction(service.manager, service.name, "restart")
                                }
                                aria-label={`重启 ${service.name}`}
                              >
                                <RotateCw size={14} aria-hidden="true" />
                                重启
                              </button>
                              <button
                                className="btn ops-button"
                                disabled={busy !== null || services.loading}
                                onClick={() => handleAction(service.manager, service.name, "stop")}
                                aria-label={`停止 ${service.name}`}
                              >
                                <Square size={13} aria-hidden="true" />
                                停止
                              </button>
                            </>
                          ) : (
                            <button
                              className="btn ops-button ops-button-start"
                              disabled={busy !== null || services.loading}
                              onClick={() => handleAction(service.manager, service.name, "start")}
                              aria-label={`启动 ${service.name}`}
                            >
                              <Play size={14} aria-hidden="true" />
                              启动
                            </button>
                          )}
                        </>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </section>

      <section className="ops-panel" aria-labelledby="ops-caches-title">
        <div className="ops-panel-head">
          <div className="ops-section-title">
            <Database size={19} aria-hidden="true" />
            <div>
              <h2 id="ops-caches-title">开发缓存</h2>
              <p>查看磁盘占用，按需清理可重新生成的缓存。</p>
            </div>
          </div>
          <button
            ref={cacheRefresh}
            className="btn ops-button"
            onClick={() => caches.reload()}
            disabled={caches.loading || cleanBusy}
            aria-label="刷新缓存"
          >
            <RefreshCw size={14} className={caches.loading ? "ops-spin" : ""} aria-hidden="true" />
            {caches.loading ? "统计中" : "刷新"}
          </button>
        </div>

        {caches.error && (
          <LoadError
            title="缓存读取失败"
            message={caches.error}
            hasData={caches.data !== null}
            onRetry={() => caches.reload()}
            retryLabel="重试缓存"
            disabled={caches.loading || cleanBusy}
          />
        )}
        {cacheNotice && (
          <div className="ops-feedback ops-feedback-success" role="status">
            <CheckCircle2 size={17} aria-hidden="true" />
            <p>{cacheNotice}</p>
          </div>
        )}

        <div aria-busy={caches.loading}>
          {caches.loading && !caches.data && (
            <div className="ops-empty" role="status">
              <LoaderCircle className="ops-spin" size={23} aria-hidden="true" />
              <strong>正在统计开发缓存</strong>
              <p>缓存较多时可能需要一些时间。</p>
            </div>
          )}
          {caches.data && caches.data.length === 0 && (
            <div className="ops-empty">
              <Database size={26} aria-hidden="true" />
              <strong>暂无开发缓存</strong>
              <p>尚未探测到 Homebrew、mise、npm、pip 等工具的缓存。</p>
            </div>
          )}
          {caches.data && caches.data.length > 0 && (
            <ul className="ops-list" aria-label="开发缓存列表">
              {caches.data.map((cache) => (
                <li className="ops-row" key={cache.id}>
                  <span className="ops-item-icon">
                    <Database size={18} aria-hidden="true" />
                  </span>
                  <div className="ops-item-info">
                    <div className="ops-item-heading">
                      <h3>{cache.name}</h3>
                      {!cache.exists && <span className="ops-badge ops-badge-muted">未生成</span>}
                      {cache.exists && !cache.cleanable && (
                        <span className="ops-badge ops-badge-muted">仅查看</span>
                      )}
                    </div>
                    <p className="ops-item-meta ops-path" title={cache.path}>
                      {cache.path}
                    </p>
                  </div>
                  <div className="ops-cache-actions">
                    <span
                      className="ops-size"
                      title={cache.approx ? "体积为统计下限，实际占用可能更大" : undefined}
                    >
                      {cache.exists
                        ? `${cache.approx ? "≥ " : ""}${humanSize(cache.sizeBytes)}`
                        : "—"}
                    </span>
                    {cache.cleanable && cache.exists ? (
                      <button
                        className="btn ops-button"
                        disabled={cleanBusy || caches.loading}
                        onClick={(event) => requestClean(cache, event.currentTarget)}
                        aria-label={`清理 ${cache.name} 缓存`}
                      >
                        <Trash2 size={14} aria-hidden="true" />
                        清理
                      </button>
                    ) : (
                      <span className="ops-readonly">
                        {cache.exists ? "无官方清理命令" : "无需清理"}
                      </span>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="ops-panel-foot">
          <Info size={15} aria-hidden="true" />
          <p>清理前可查看具体命令。体积标注「≥」表示统计下限，实际释放空间以清理结果为准。</p>
        </div>
      </section>

      <Dialog
        open={cleanTarget !== null}
        onOpenChange={(open) => {
          if (!open && !cacheOperation.current) setCleanTarget(null);
        }}
      >
        <DialogContent
          className="ops-clean-dialog"
          showClose={!cleanBusy}
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            cancelButton.current?.focus();
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            if (cleanTrigger.current?.isConnected && !cleanTrigger.current.disabled) {
              cleanTrigger.current.focus();
            } else {
              cacheRefresh.current?.focus();
            }
          }}
        >
          <div className="ops-dialog-head">
            <span className="ops-stat-icon">
              <Trash2 size={21} aria-hidden="true" />
            </span>
            <DialogTitle>清理 {cleanTarget?.name} 缓存</DialogTitle>
            <DialogDescription>
              将运行工具的官方清理命令。后续使用时，工具可能需要重新下载或生成缓存。
            </DialogDescription>
          </div>
          {cleanTarget && (
            <div className="ops-dialog-body">
              <div className="ops-clean-summary">
                <span>当前缓存占用</span>
                <strong>
                  {cleanTarget.approx ? "≥ " : ""}
                  {humanSize(cleanTarget.sizeBytes)}
                </strong>
              </div>
              <p className="ops-path ops-clean-path">{cleanTarget.path}</p>
              <span className="ops-command-label">将执行的官方命令</span>
              <code className="ops-command">
                {CLEAN_COMMANDS[cleanTarget.id] ?? "由工具提供的官方清理命令"}
              </code>
              {cleanTarget.id === "pip" && (
                <p className="ops-dialog-hint">Windows 使用 python -m pip cache purge。</p>
              )}
              <p className="ops-dialog-hint">大缓存可能需要几分钟，清理期间请保持应用开启。</p>
              {cleanError && (
                <div className="ops-feedback ops-feedback-error" role="alert">
                  <AlertCircle size={17} aria-hidden="true" />
                  <div>
                    <strong>清理失败</strong>
                    <p>{cleanError}</p>
                  </div>
                </div>
              )}
              {cleanBusy && (
                <p className="ops-operation" role="status">
                  <LoaderCircle size={16} className="ops-spin" aria-hidden="true" />
                  正在清理 {cleanTarget.name} 缓存…
                </p>
              )}
            </div>
          )}
          <div className="ops-dialog-foot">
            <button
              ref={cancelButton}
              className="btn ops-button"
              disabled={cleanBusy}
              onClick={() => setCleanTarget(null)}
            >
              取消
            </button>
            <button className="btn primary ops-button" disabled={cleanBusy} onClick={handleClean}>
              {cleanBusy ? (
                <LoaderCircle size={14} className="ops-spin" aria-hidden="true" />
              ) : (
                <Trash2 size={14} aria-hidden="true" />
              )}
              {cleanBusy ? "清理中…" : cleanError ? "重试清理" : "确认清理"}
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
