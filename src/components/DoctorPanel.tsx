import { useEffect, useState } from "react";
import { doctorRun, doctorFix, errorMessage, type DoctorCheck } from "../api";

/** 检查等级 → 圆点颜色与文案 */
function levelView(level: DoctorCheck["level"]): { color: string; label: string } {
  switch (level) {
    case "ok":
      return { color: "var(--success)", label: "正常" };
    case "warn":
      return { color: "var(--warning)", label: "注意" };
    case "fail":
      return { color: "var(--destructive)", label: "异常" };
  }
}

/** 环境健康区：doctor 巡检的紧凑呈现——chips 一行总览，异常项给详情与就地修复。
 *  挂载即自动巡检；修复成功后自动重跑刷新。 */
export default function DoctorPanel() {
  const [checks, setChecks] = useState<DoctorCheck[] | null>(null);
  const [running, setRunning] = useState(false);
  const [msg, setMsg] = useState("");
  const [fixingId, setFixingId] = useState<string | null>(null);

  function run() {
    setRunning(true);
    setMsg("");
    doctorRun()
      .then(setChecks)
      .catch((e) => setMsg(`体检失败：${errorMessage(e)}`))
      .finally(() => setRunning(false));
  }

  useEffect(run, []);

  async function handleFix(id: string) {
    setFixingId(id);
    setMsg("");
    try {
      setMsg(await doctorFix(id));
      setChecks(await doctorRun());
    } catch (e) {
      setMsg(`修复失败：${errorMessage(e)}`);
    } finally {
      setFixingId(null);
    }
  }

  const issues = (checks ?? []).filter((c) => c.level !== "ok");

  return (
    <section className="panel">
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <h2 className="panel-title" style={{ margin: 0 }}>
          环境健康
        </h2>
        <span style={{ flex: 1 }} />
        <button className="btn-ghost" onClick={run} disabled={running}>
          {running ? "巡检中…" : "重新巡检"}
        </button>
      </div>

      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 10 }}>
        {(checks ?? []).map((c) => {
          const lv = levelView(c.level);
          return (
            <span
              key={c.id}
              className="pill muted"
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                ...(c.level !== "ok" ? { color: lv.color, borderColor: lv.color } : {}),
              }}
              title={c.detail}
            >
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: lv.color,
                  flexShrink: 0,
                }}
              />
              {c.title} · {lv.label}
            </span>
          );
        })}
        {running && !checks && <span className="setting-desc">正在巡检整机环境…</span>}
      </div>

      {checks && issues.length === 0 && (
        <p className="setting-desc" style={{ marginTop: 8 }}>
          {checks.length} 项检查全部正常：mise 可用、PATH 干净、shell 已接入、托管接入有效。
        </p>
      )}

      {issues.map((c) => {
        const lv = levelView(c.level);
        return (
          <div
            key={`issue-${c.id}`}
            className="setting-row"
            style={{ cursor: "default", alignItems: "flex-start" }}
          >
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
                    background: lv.color,
                  }}
                />
                {c.title}
              </span>
              <span className="setting-desc" style={{ wordBreak: "break-all" }}>
                {c.detail}
                {c.hint ? ` · 建议：${c.hint}` : ""}
              </span>
            </span>
            {c.fixable && (
              <button className="btn" disabled={fixingId !== null} onClick={() => handleFix(c.id)}>
                {fixingId === c.id ? "修复中…" : "一键修复"}
              </button>
            )}
          </div>
        );
      })}

      {msg && (
        <p className="setting-desc" style={{ marginTop: 8, whiteSpace: "pre-wrap" }}>
          {msg}
        </p>
      )}
    </section>
  );
}
