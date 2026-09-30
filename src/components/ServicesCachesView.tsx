import { useEffect, useState } from "react";
import { serviceList, serviceAction, errorMessage, type ServiceOverview } from "../api";

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

export default function ServicesCachesView() {
  const [overview, setOverview] = useState<ServiceOverview | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  function reload() {
    serviceList()
      .then(setOverview)
      .catch((e) => setError(errorMessage(e)));
  }

  useEffect(reload, []);

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
    </div>
  );
}
