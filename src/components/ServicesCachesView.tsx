import { useEffect, useState } from "react";
import {
  serviceList,
  serviceAction,
  cacheList,
  cacheClean,
  errorMessage,
  type ServiceOverview,
  type CacheInfo,
} from "../api";

/** 服务状态 → 展示文案与语义色 */
function stateView(state: string): { label: string; color?: string } {
  switch (state) {
    case "started":
      return { label: "运行中", color: "var(--success)" };
    case "error":
      return { label: "异常", color: "var(--destructive)" };
    case "stopped":
      return { label: "已停止" };
    default:
      return { label: state };
  }
}

/** 字节数 → 人类可读体积 */
function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export default function ServicesCachesView() {
  const [overview, setOverview] = useState<ServiceOverview | null>(null);
  const [caches, setCaches] = useState<CacheInfo[] | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [cleanBusy, setCleanBusy] = useState<string | null>(null);

  function reload() {
    serviceList()
      .then(setOverview)
      .catch((e) => setError(errorMessage(e)));
  }

  function reloadCaches() {
    cacheList()
      .then(setCaches)
      .catch(() => {});
  }

  useEffect(() => {
    reload();
    reloadCaches();
  }, []);

  async function handleAction(manager: string, name: string, act: string) {
    setBusy(`${manager}::${name}`);
    setError("");
    setNotice("");
    try {
      setNotice(await serviceAction(manager, name, act));
      reload();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleClean(c: CacheInfo) {
    if (!window.confirm(`确认清理 ${c.name} 缓存？将执行官方清理命令，大缓存可能需要几分钟。`)) {
      return;
    }
    setCleanBusy(c.id);
    setError("");
    setNotice("");
    try {
      setNotice(await cacheClean(c.id));
      reloadCaches();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setCleanBusy(null);
    }
  }

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>服务与缓存</h1>
          <p className="view-sub">本地开发服务启停与开发缓存治理</p>
        </div>
      </div>

      {error && (
        <div className="banner warn" onClick={() => setError("")}>
          {error}
        </div>
      )}
      {notice && (
        <div className="banner info" onClick={() => setNotice("")}>
          {notice}
        </div>
      )}

      <section className="panel">
        <h2 className="panel-title">本地服务</h2>
        {(overview?.notes ?? []).map((n) => (
          <p className="setting-desc" key={n} style={{ marginBottom: 8 }}>
            {n}
          </p>
        ))}
        {overview && overview.services.length === 0 && (
          <div className="empty small">
            没有发现可管理的服务（brew services 或 systemd 用户级服务）。
          </div>
        )}
        <div className="setting-list">
          {(overview?.services ?? []).map((s) => {
            const sv = stateView(s.state);
            const busyKey = `${s.manager}::${s.name}`;
            return (
              <div key={busyKey} className="setting-row" style={{ cursor: "default" }}>
                <span className="setting-info" style={{ flex: 1, minWidth: 0 }}>
                  <span
                    className="setting-name"
                    style={{ display: "flex", alignItems: "center", gap: 8 }}
                  >
                    <span
                      style={{
                        width: 8,
                        height: 8,
                        borderRadius: "50%",
                        flexShrink: 0,
                        background: sv.color ?? "var(--muted-foreground)",
                      }}
                    />
                    {s.name}
                    <span className="pill muted">{s.manager}</span>
                  </span>
                  <span className="setting-desc">
                    {sv.label}
                    {s.pid ? ` · PID ${s.pid}` : ""}
                  </span>
                </span>
                <span style={{ display: "flex", gap: 8 }}>
                  {s.state !== "started" && (
                    <button
                      className="btn"
                      disabled={busy !== null}
                      onClick={() => handleAction(s.manager, s.name, "start")}
                    >
                      {busy === busyKey ? "执行中…" : "启动"}
                    </button>
                  )}
                  {s.state === "started" && (
                    <>
                      <button
                        className="btn"
                        disabled={busy !== null}
                        onClick={() => handleAction(s.manager, s.name, "restart")}
                      >
                        重启
                      </button>
                      <button
                        className="btn"
                        disabled={busy !== null}
                        onClick={() => handleAction(s.manager, s.name, "stop")}
                      >
                        停止
                      </button>
                    </>
                  )}
                </span>
              </div>
            );
          })}
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">开发缓存</h2>
        {caches && caches.length === 0 && (
          <div className="empty small">
            没有探测到常见的开发缓存（brew / mise / npm / pip 等）。
          </div>
        )}
        <div className="setting-list">
          {(caches ?? []).map((c) => (
            <div key={c.id} className="setting-row" style={{ cursor: "default" }}>
              <span className="setting-info" style={{ flex: 1, minWidth: 0 }}>
                <span
                  className="setting-name"
                  style={{ display: "flex", alignItems: "center", gap: 8 }}
                >
                  {c.name}
                  <span className="pill muted">
                    {c.exists ? `${c.approx ? "≥ " : ""}${humanSize(c.sizeBytes)}` : "未生成"}
                  </span>
                </span>
                <span className="setting-desc" style={{ wordBreak: "break-all" }}>
                  {c.path}
                </span>
              </span>
              {c.cleanable && c.exists && (
                <button
                  className="btn"
                  disabled={cleanBusy !== null}
                  onClick={() => handleClean(c)}
                >
                  {cleanBusy === c.id ? "清理中…" : "清理"}
                </button>
              )}
            </div>
          ))}
        </div>
        <p className="setting-desc" style={{ marginTop: 8 }}>
          清理只使用各工具的官方命令（如 brew cleanup / npm cache clean）；cargo registry
          无官方一键清理，故只展示体积。
        </p>
      </section>
    </div>
  );
}
