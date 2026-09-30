import { useEffect, useState } from "react";
import {
  envCenterList,
  envCenterRemove,
  envCenterSet,
  errorMessage,
  type EnvCenterSnapshot,
} from "../api";

/** 系统 env 列表无搜索词时的最大渲染行数（超出提示细化搜索） */
const SYSTEM_ENV_RENDER_LIMIT = 80;

export default function EnvView() {
  const [snap, setSnap] = useState<EnvCenterSnapshot | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);

  // 新增表单
  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");

  // 行内编辑
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");

  // 系统 env 搜索
  const [sysFilter, setSysFilter] = useState("");

  useEffect(() => {
    envCenterList()
      .then(setSnap)
      .catch((e) => setError(errorMessage(e)));
  }, []);

  const reload = () => {
    envCenterList()
      .then(setSnap)
      .catch((e) => setError(errorMessage(e)));
  };

  async function handleSet(key: string, value: string) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setNotice(await envCenterSet(key, value));
      setEditing(null);
      reload();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleRemove(key: string) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setNotice(await envCenterRemove(key));
      reload();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function handleAdd() {
    const k = newKey.trim();
    if (!k) return;
    handleSet(k, newValue).then(() => {
      setNewKey("");
      setNewValue("");
    });
  }

  const sysFiltered = (snap?.systemEnv ?? []).filter((v) =>
    sysFilter
      ? v.key.toLowerCase().includes(sysFilter.toLowerCase()) ||
        v.value.toLowerCase().includes(sysFilter.toLowerCase())
      : true,
  );

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>环境变量</h1>
          <p className="view-sub">整机环境变量中心 —— 全局 mise [env] 与系统级变量的统一视角</p>
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

      {snap && (
        <p className="setting-desc" style={{ margin: "4px 0 12px" }}>
          配置文件：{snap.configPath || "（无法定位）"}
          {!snap.exists && "（尚未创建，添加第一个变量时自动生成）"}
        </p>
      )}

      {snap && snap.conflicts.length > 0 && (
        <div className="banner warn">
          检测到 {snap.conflicts.length} 个同名冲突（系统值与全局 mise 值不同，终端实际生效取决于
          shell 是否接入 mise）：
          {snap.conflicts.map((c) => (
            <div key={c.key} style={{ marginTop: 4 }}>
              <code>{c.key}</code>：mise={c.miseValue} · 系统={c.systemValue}
            </div>
          ))}
        </div>
      )}

      <section className="panel">
        <h2 className="panel-title">全局环境变量（mise [env]）</h2>
        <div className="form-row" style={{ marginBottom: 12 }}>
          <input
            className="input"
            placeholder="变量名（如 HTTP_PROXY）"
            value={newKey}
            onChange={(e) => setNewKey(e.target.value)}
          />
          <input
            className="input grow"
            placeholder="值"
            value={newValue}
            onChange={(e) => setNewValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          />
          <button className="btn primary" onClick={handleAdd} disabled={busy || !newKey.trim()}>
            添加
          </button>
        </div>

        {snap && snap.entries.length === 0 && (
          <div className="empty small">
            全局 [env] 还没有变量。添加后，接入 mise 的所有终端与项目都会继承这些变量。
          </div>
        )}

        <div className="setting-list">
          {(snap?.entries ?? []).map((e) => (
            <div key={e.key} className="setting-row" style={{ cursor: "default" }}>
              <span className="setting-info" style={{ flex: 1, minWidth: 0 }}>
                <span className="setting-name">
                  <code>{e.key}</code>
                  {e.kind === "complex" && (
                    <span className="pill muted" style={{ marginLeft: 8 }}>
                      高级值
                    </span>
                  )}
                </span>
                {editing === e.key ? (
                  <span className="form-row" style={{ marginTop: 6 }}>
                    <input
                      className="input grow"
                      value={editValue}
                      autoFocus
                      onChange={(ev) => setEditValue(ev.target.value)}
                      onKeyDown={(ev) => {
                        if (ev.key === "Enter") handleSet(e.key, editValue);
                        if (ev.key === "Escape") setEditing(null);
                      }}
                    />
                  </span>
                ) : (
                  <span className="setting-desc" style={{ wordBreak: "break-all" }}>
                    {e.value}
                  </span>
                )}
              </span>
              {e.kind === "simple" && (
                <span style={{ display: "flex", gap: 8 }}>
                  {editing === e.key ? (
                    <>
                      <button
                        className="btn primary"
                        disabled={busy}
                        onClick={() => handleSet(e.key, editValue)}
                      >
                        保存
                      </button>
                      <button className="btn" onClick={() => setEditing(null)}>
                        取消
                      </button>
                    </>
                  ) : (
                    <>
                      <button
                        className="btn"
                        disabled={busy}
                        onClick={() => {
                          setEditing(e.key);
                          setEditValue(e.value);
                        }}
                      >
                        编辑
                      </button>
                      <button className="btn" disabled={busy} onClick={() => handleRemove(e.key)}>
                        删除
                      </button>
                    </>
                  )}
                </span>
              )}
            </div>
          ))}
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">系统 / 用户级环境变量（只读）</h2>
        <div className="form-row" style={{ marginBottom: 12 }}>
          <input
            className="input grow"
            placeholder="搜索变量名或值…"
            value={sysFilter}
            onChange={(e) => setSysFilter(e.target.value)}
          />
          <span className="pill muted">{sysFiltered.length} 项</span>
        </div>
        <div className="setting-list">
          {sysFiltered.slice(0, SYSTEM_ENV_RENDER_LIMIT).map((v) => (
            <div key={v.key} className="setting-row" style={{ cursor: "default" }}>
              <span className="setting-info" style={{ flex: 1, minWidth: 0 }}>
                <span className="setting-name">
                  <code>{v.key}</code>
                </span>
                <span className="setting-desc" style={{ wordBreak: "break-all" }}>
                  {v.value.length > 200 ? v.value.slice(0, 200) + "…" : v.value}
                </span>
              </span>
            </div>
          ))}
          {sysFiltered.length > SYSTEM_ENV_RENDER_LIMIT && (
            <div className="empty small">
              仅显示前 {SYSTEM_ENV_RENDER_LIMIT} 项，请细化搜索关键词。
            </div>
          )}
          {sysFiltered.length === 0 && <div className="empty small">没有匹配的系统变量。</div>}
        </div>
      </section>
    </div>
  );
}
