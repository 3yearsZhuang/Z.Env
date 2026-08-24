import { useCallback, useEffect, useRef, useState } from "react";
import {
  systemStats,
  SystemStats,
  checkMise,
  MiseStatus,
  getEnvInfo,
  EnvInfo,
  errorMessage,
} from "../api";

/** 保留的历史采样点数量 */
const MAX_HISTORY = 60;

/** 环形使用率指示器（SVG） */
function Ring({
  percent,
  color,
  size = 110,
}: {
  percent: number;
  color: string;
  size?: number;
}) {
  const r = 42;
  const c = 2 * Math.PI * r;
  const clamped = Math.min(100, Math.max(0, percent));
  const dash = (clamped / 100) * c;
  return (
    <svg viewBox="0 0 110 110" width={size} height={size}>
      <circle cx="55" cy="55" r={r} className="gauge-track" />
      <circle
        cx="55"
        cy="55"
        r={r}
        className="gauge-value"
        style={{ stroke: color, strokeDasharray: `${dash} ${c}` }}
      />
      <text x="55" y="59" textAnchor="middle" className="gauge-num">
        {Math.round(clamped)}%
      </text>
    </svg>
  );
}

/** 单值历史曲线（0~100，用于 CPU / 内存 / 磁盘使用率） */
function UsageChart({ data, color }: { data: number[]; color: string }) {
  const W = 720;
  const H = 140;
  const PAD = 8;
  const N = Math.max(2, data.length);

  const toX = (i: number) => PAD + (i / (N - 1)) * (W - PAD * 2);
  const toY = (v: number) =>
    H - PAD - (Math.min(100, Math.max(0, v)) / 100) * (H - PAD * 2);

  const line = data
    .map((v, i) => `${i === 0 ? "M" : "L"} ${toX(i).toFixed(1)} ${toY(v).toFixed(1)}`)
    .join(" ");
  const area = `${line} L ${toX(N - 1).toFixed(1)} ${(H - PAD).toFixed(1)} L ${PAD} ${(H - PAD).toFixed(1)} Z`;
  const gid = `area-${color.replace("#", "")}`;

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="chart" preserveAspectRatio="none">
      <defs>
        <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.3" />
          <stop offset="100%" stopColor={color} stopOpacity="0.02" />
        </linearGradient>
      </defs>
      {[25, 50, 75].map((g) => (
        <line key={g} x1={PAD} x2={W - PAD} y1={toY(g)} y2={toY(g)} className="chart-grid" />
      ))}
      <path d={area} fill={`url(#${gid})`} />
      <path d={line} fill="none" className="chart-line" style={{ stroke: color }} />
    </svg>
  );
}

/** 双值历史曲线（用于网络 下行/上行，自动按最大值缩放） */
function NetChart({ rx, tx }: { rx: number[]; tx: number[] }) {
  const W = 720;
  const H = 140;
  const PAD = 8;
  const base = rx.length >= 2 ? rx : [0, 0];
  const other = tx.length >= 2 ? tx : [0, 0];
  const N = Math.max(2, base.length, other.length);
  const maxVal = Math.max(1, ...base, ...other);

  const toX = (i: number) => PAD + (i / (N - 1)) * (W - PAD * 2);
  const toY = (v: number) =>
    H - PAD - (Math.min(maxVal, Math.max(0, v)) / maxVal) * (H - PAD * 2);

  const rxLine = base
    .map((v, i) => `${i === 0 ? "M" : "L"} ${toX(i).toFixed(1)} ${toY(v).toFixed(1)}`)
    .join(" ");
  const txLine = other
    .map((v, i) => `${i === 0 ? "M" : "L"} ${toX(i).toFixed(1)} ${toY(v).toFixed(1)}`)
    .join(" ");

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="chart" preserveAspectRatio="none">
      {[0.25, 0.5, 0.75].map((g) => (
        <line
          key={g}
          x1={PAD}
          x2={W - PAD}
          y1={toY(maxVal * g)}
          y2={toY(maxVal * g)}
          className="chart-grid"
        />
      ))}
      <path d={rxLine} fill="none" className="chart-line" style={{ stroke: "#4f8cff" }} />
      <path d={txLine} fill="none" className="chart-line" style={{ stroke: "#3fb97f" }} />
    </svg>
  );
}

/** 格式化字节/秒为可读速率 */
function fmtRate(bytesPerSec: number): string {
  if (bytesPerSec >= 1048576) return `${(bytesPerSec / 1048576).toFixed(2)} MB/s`;
  if (bytesPerSec >= 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${Math.round(bytesPerSec)} B/s`;
}

/** 网络双环指示器：外环=下行，内环=上行 */
function NetRing({
  rx,
  tx,
  max,
  size = 64,
}: {
  rx: number;
  tx: number;
  max: number;
  size?: number;
}) {
  const base = Math.max(1, max);
  const pct = (v: number) => Math.min(1, Math.max(0, v / base)) * 100;
  const R_OUT = 54;
  const R_IN = 38;
  const c_out = 2 * Math.PI * R_OUT;
  const c_in = 2 * Math.PI * R_IN;
  const rxPct = pct(rx);
  const txPct = pct(tx);
  return (
    <svg viewBox="0 0 120 120" width={size} height={size}>
      {/* 外环（下行）轨道 */}
      <circle cx="60" cy="60" r={R_OUT} className="gauge-track" strokeWidth="10" />
      <circle
        cx="60"
        cy="60"
        r={R_OUT}
        className="gauge-value"
        strokeWidth="10"
        style={{
          stroke: "#4f8cff",
          strokeDasharray: `${(rxPct / 100) * c_out} ${c_out}`,
        }}
      />
      {/* 内环（上行）轨道 */}
      <circle cx="60" cy="60" r={R_IN} className="gauge-track" strokeWidth="9" />
      <circle
        cx="60"
        cy="60"
        r={R_IN}
        className="gauge-value"
        strokeWidth="9"
        style={{
          stroke: "#3fb97f",
          strokeDasharray: `${(txPct / 100) * c_in} ${c_in}`,
        }}
      />
    </svg>
  );
}

/** 一张统一正方形资源卡片 */
function Card({
  title,
  meta,
  model,
  display,
  body,
}: {
  title: string;
  meta?: string;
  model?: string;
  display: React.ReactNode;
  body: React.ReactNode;
}) {
  return (
    <section className="usage-card">
      <div className="usage-top">
        {display}
        <div className="usage-top-info">
          <div className="usage-title">{title}</div>
          {meta && <div className="usage-meta">{meta}</div>}
        </div>
      </div>
      <div className="usage-body">{body}</div>
      {model && <div className="usage-model" title={model}>{model}</div>}
    </section>
  );
}

export default function Dashboard() {
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [mise, setMise] = useState<MiseStatus | null>(null);
  const [env, setEnv] = useState<EnvInfo | null>(null);
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [memoryHistory, setMemoryHistory] = useState<number[]>([]);
  const [diskHistory, setDiskHistory] = useState<number[]>([]);
  const [rxHistory, setRxHistory] = useState<number[]>([]);
  const [txHistory, setTxHistory] = useState<number[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshMs, setRefreshMs] = useState(2000);
  const timerRef = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const s = await systemStats();
      setStats(s);

      // 磁盘总使用率
      const diskTotal = s.disks.reduce((a, d) => a + d.totalGb, 0);
      const diskUsed = s.disks.reduce((a, d) => a + (d.totalGb - d.availableGb), 0);
      const diskPercent = diskTotal > 0 ? (diskUsed / diskTotal) * 100 : 0;

      // 网络总速率
      const rx = s.interfaces.reduce((a, i) => a + i.rxRate, 0);
      const tx = s.interfaces.reduce((a, i) => a + i.txRate, 0);

      setCpuHistory((prev) => [...prev, s.cpuUsage].slice(-MAX_HISTORY));
      setMemoryHistory((prev) => [...prev, s.memoryPercent].slice(-MAX_HISTORY));
      setDiskHistory((prev) => [...prev, diskPercent].slice(-MAX_HISTORY));
      setRxHistory((prev) => [...prev, rx].slice(-MAX_HISTORY));
      setTxHistory((prev) => [...prev, tx].slice(-MAX_HISTORY));

      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    checkMise()
      .then(setMise)
      .catch(() => setMise({ installed: false, version: null }));
    getEnvInfo()
      .then(setEnv)
      .catch(() => setEnv(null));
    refresh();
    if (refreshMs > 0) {
      timerRef.current = window.setInterval(refresh, refreshMs);
    } else {
      timerRef.current = null;
    }
    return () => {
      if (timerRef.current) window.clearInterval(timerRef.current);
    };
  }, [refresh, refreshMs]);

  // 磁盘 / 网络当前汇总值
  const diskTotal = stats?.disks.reduce((a, d) => a + d.totalGb, 0) ?? 0;
  const diskUsed = stats?.disks.reduce((a, d) => a + (d.totalGb - d.availableGb), 0) ?? 0;
  const diskPercent = diskTotal > 0 ? (diskUsed / diskTotal) * 100 : 0;
  const curRx = stats?.interfaces.reduce((a, i) => a + i.rxRate, 0) ?? 0;
  const curTx = stats?.interfaces.reduce((a, i) => a + i.txRate, 0) ?? 0;
  // 双环的 100% 基准：取历史峰值，避免固定值导致两条环始终低调或溢出
  const netMax = Math.max(1, ...rxHistory, ...txHistory) * 1.1;

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>系统概览</h1>
          <p className="view-sub">
            {stats?.hostName || ""}
            {stats?.osName ? ` · ${stats.osName}` : ""}
            {" "}实时资源占用{refreshMs > 0 ? `（每 ${refreshMs / 1000} 秒刷新）` : "（已暂停自动刷新）"}
          </p>
        </div>
        <div className="head-tools">
          <select
            className="input select-inline"
            value={String(refreshMs)}
            onChange={(e) => setRefreshMs(Number(e.target.value))}
            title="刷新间隔"
          >
            <option value="100">0.1 秒</option>
            <option value="500">0.5 秒</option>
            <option value="2000">2 秒</option>
            <option value="5000">5 秒</option>
            <option value="0">关闭</option>
          </select>
          <button
            className="btn-ghost icon-only"
            onClick={refresh}
            disabled={loading}
            title="现在刷新"
          >
            ⟳
          </button>
        </div>
      </div>

      {env && (
        <div className="banner info top-banner" title="环境信息">
          <span className="banner-icon">◆</span>
          <span className="top-item">系统 · {env.os.name} {env.os.version}（{env.os.arch}）</span>
          {env.pkg
            .filter((p) => p.available)
            .map((p) => (
              <span className="top-item" key={p.name}>
                {p.name} ·{" "}
                <b>{p.version ? p.version.split(/\s+/, 2).join(" ") : "未安装"}</b>
              </span>
            ))}
        </div>
      )}

      {mise && mise.installed && (
        <div className="banner info">
          <span className="banner-icon">◆</span>
          已接入 <strong>mise</strong> {mise.version ? `· ${mise.version}` : ""}，运行时工具页可管理版本
        </div>
      )}
      {mise && !mise.installed && (
        <div className="banner warn">
          <span className="banner-icon">✕</span>
          未检测到 <strong>mise</strong>。访问{" "}
          <a href="https://mise.jdx.dev" target="_blank" rel="noreferrer">
            mise.jdx.dev
          </a>{" "}
          安装后，即可管理运行时版本（安装：<code>curl https://mise.run | sh</code>）
        </div>
      )}

      {error && (
        <div className="banner error" onClick={() => setError(null)}>
          ⚠ {error}
        </div>
      )}

      {loading && !stats ? (
        <div className="empty">正在检测系统资源…</div>
      ) : stats ? (
        <div className="usage-grid">
          <Card
            title="CPU"
            meta={`${stats.cpuCores} 核`}
            model={stats.cpuBrand ?? undefined}
            display={<Ring percent={stats.cpuUsage} color="#4f8cff" size={64} />}
            body={
              cpuHistory.length < 2 ? (
                <div className="empty small">采样中…</div>
              ) : (
                <UsageChart data={cpuHistory} color="#4f8cff" />
              )
            }
          />

          <Card
            title="内存"
            meta={`${stats.memoryUsedGb.toFixed(1)} / ${stats.memoryTotalGb.toFixed(0)} GB 已用`}
            model={
              stats.memories
                .map((m) => `${m.title} · ${m.size}`)
                .join(" / ") || undefined
            }
            display={<Ring percent={stats.memoryPercent} color="#3fb97f" size={64} />}
            body={
              memoryHistory.length < 2 ? (
                <div className="empty small">采样中…</div>
              ) : (
                <UsageChart data={memoryHistory} color="#3fb97f" />
              )
            }
          />

          <Card
            title="网络"
            meta={`下行 ${fmtRate(curRx)}`}
            model={`上行 ${fmtRate(curTx)}`}
            display={<NetRing rx={curRx} tx={curTx} max={netMax} size={64} />}
            body={
              rxHistory.length < 2 ? (
                <div className="empty small">采样中…</div>
              ) : (
                <NetChart rx={rxHistory} tx={txHistory} />
              )
            }
          />

          <Card
            title="磁盘"
            meta={`${diskUsed.toFixed(0)} / ${diskTotal.toFixed(0)} GB 已用`}
            model={
              stats.storage
                .map((s) => `${s.title} · ${s.size}`)
                .join(" / ") || undefined
            }
            display={<Ring percent={diskPercent} color="#e0b45a" size={64} />}
            body={
              diskHistory.length < 2 ? (
                <div className="empty small">采样中…</div>
              ) : (
                <UsageChart data={diskHistory} color="#e0b45a" />
              )
            }
          />

          {stats.battery && (
            <Card
              title="电池"
              meta={stats.battery.charging ? "充电中" : "使用中"}
              display={<Ring percent={stats.battery.percent} color="#3fb97f" size={64} />}
              body={
                <div className="battery-fill">
                  <span className="battery-fill-num" style={{ color: "#3fb97f" }}>
                    {Math.round(stats.battery.percent)}%
                  </span>
                  <span className="battery-fill-status">
                    {stats.battery.charging ? "充电中 ⚡" : "使用中"}
                  </span>
                </div>
              }
            />
          )}

          <Card
            title="GPU"
            meta={
              stats.gpus.length > 0
                ? `${stats.gpus.length} 个图形处理器`
                : "未检测到 GPU"
            }
            model={
              stats.gpus
                .map((g) => (g.vram ? `${g.name} · ${g.vram}` : g.name))
                .join(" / ") || undefined
            }
            display={<Ring percent={0} color="#3a4150" size={64} />}
            body={
              stats.gpus.length > 0 ? (
                <div className="space-fill">
                  <span className="gpu-vram">
                    {stats.gpus
                      .map((g) => g.vram || "显存未知")
                      .join(" / ")}
                  </span>
                </div>
              ) : (
                <div className="empty small">未检测到 GPU 或无法读取</div>
              )
            }
          />
        </div>
      ) : (
        <div className="empty">暂无数据</div>
      )}
    </div>
  );
}