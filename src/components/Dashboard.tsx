import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import {
  Activity,
  ArrowDown,
  ArrowUp,
  ArrowUpRight,
  BatteryCharging,
  Box,
  Check,
  ChevronRight,
  CircleAlert,
  Cpu,
  FolderKanban,
  HardDrive,
  Layers,
  MemoryStick,
  Monitor,
  Network,
  RefreshCw,
  ServerOff,
  X,
} from "lucide-react";
import type { Navigate } from "../lib/nav";
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
function Ring({ percent, color, size = 110 }: { percent: number; color: string; size?: number }) {
  const r = 42;
  const c = 2 * Math.PI * r;
  const clamped = Math.min(100, Math.max(0, percent));
  const dash = (clamped / 100) * c;
  return (
    <svg viewBox="0 0 110 110" width={size} height={size} aria-hidden="true">
      <circle cx="55" cy="55" r={r} className="gauge-track" />
      <circle
        cx="55"
        cy="55"
        r={r}
        className="gauge-value"
        style={{ stroke: color, strokeDasharray: `${dash} ${c}` }}
      />
    </svg>
  );
}

/** 单值历史曲线（0~100，用于 CPU / 内存 / 磁盘使用率） */
function UsageChart({ data, color }: { data: number[]; color: string }) {
  const gid = useId();
  const W = 720;
  const H = 140;
  const PAD = 8;
  const N = Math.max(2, data.length);

  const toX = (i: number) => PAD + (i / (N - 1)) * (W - PAD * 2);
  const toY = (v: number) => H - PAD - (Math.min(100, Math.max(0, v)) / 100) * (H - PAD * 2);

  const line = data
    .map((v, i) => `${i === 0 ? "M" : "L"} ${toX(i).toFixed(1)} ${toY(v).toFixed(1)}`)
    .join(" ");
  const area = `${line} L ${toX(N - 1).toFixed(1)} ${(H - PAD).toFixed(1)} L ${PAD} ${(H - PAD).toFixed(1)} Z`;

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="chart" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.18" />
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
  const toY = (v: number) => H - PAD - (Math.min(maxVal, Math.max(0, v)) / maxVal) * (H - PAD * 2);

  const rxLine = base
    .map((v, i) => `${i === 0 ? "M" : "L"} ${toX(i).toFixed(1)} ${toY(v).toFixed(1)}`)
    .join(" ");
  const txLine = other
    .map((v, i) => `${i === 0 ? "M" : "L"} ${toX(i).toFixed(1)} ${toY(v).toFixed(1)}`)
    .join(" ");

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="chart" preserveAspectRatio="none" aria-hidden="true">
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
      <path
        d={rxLine}
        fill="none"
        className="chart-line"
        style={{ stroke: "var(--chart-network)" }}
      />
      <path
        d={txLine}
        fill="none"
        className="chart-line"
        style={{ stroke: "var(--chart-memory)" }}
      />
    </svg>
  );
}

/** 格式化字节/秒为可读速率 */
function fmtRate(bytesPerSec: number): string {
  if (bytesPerSec >= 1048576) return `${(bytesPerSec / 1048576).toFixed(2)} MB/s`;
  if (bytesPerSec >= 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${Math.round(bytesPerSec)} B/s`;
}

/** 资源卡片：当前数值、历史趋势与硬件说明。 */
function Card({
  title,
  icon,
  value,
  unit,
  meta,
  model,
  color,
  percent,
  body,
  footer,
}: {
  title: string;
  icon: ReactNode;
  value: string;
  unit?: string;
  meta: string;
  model?: string;
  color: string;
  percent?: number;
  body: ReactNode;
  footer: ReactNode;
}) {
  return (
    <section className="usage-card" aria-label={title}>
      <div className="resource-heading">
        <span className="resource-icon" style={{ color }}>
          {icon}
        </span>
        <h2>{title}</h2>
        <span className="resource-meta">{meta}</span>
      </div>
      <div className="resource-reading">
        <div className="resource-value">
          {value}
          <span>{unit}</span>
        </div>
        {percent !== undefined && <Ring percent={percent} color={color} size={44} />}
      </div>
      <div className="usage-body">{body}</div>
      <div className="resource-footer">{footer}</div>
      {model && (
        <div className="usage-model" title={model}>
          {model}
        </div>
      )}
    </section>
  );
}

const SHORTCUTS = [
  { tab: "tools", icon: Layers, title: "管理开发环境", description: "运行时版本与全局环境变量" },
  {
    tab: "projects",
    icon: FolderKanban,
    title: "配置项目",
    description: "发现项目，管理 mise.toml",
  },
  { tab: "tools", icon: Box, title: "查看工具与渠道", description: "核对工具定义、实例与可用操作" },
] as const;

export default function Dashboard({ onNavigate }: { onNavigate: Navigate }) {
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [mise, setMise] = useState<MiseStatus | null>(null);
  const [miseError, setMiseError] = useState(false);
  const [env, setEnv] = useState<EnvInfo | null>(null);
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [memoryHistory, setMemoryHistory] = useState<number[]>([]);
  const [diskHistory, setDiskHistory] = useState<number[]>([]);
  const [rxHistory, setRxHistory] = useState<number[]>([]);
  const [txHistory, setTxHistory] = useState<number[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshMs, setRefreshMs] = useState(2000);
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setRefreshing(true);
    try {
      const s = await systemStats();
      setStats(s);
      const diskTotal = s.disks.reduce((a, d) => a + d.totalGb, 0);
      const diskUsed = s.disks.reduce((a, d) => a + (d.totalGb - d.availableGb), 0);
      const diskPercent = diskTotal > 0 ? (diskUsed / diskTotal) * 100 : 0;
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
      inFlight.current = false;
      setRefreshing(false);
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    checkMise()
      .then(setMise)
      .catch(() => setMiseError(true));
    getEnvInfo()
      .then(setEnv)
      .catch(() => setEnv(null));
  }, []);

  useEffect(() => {
    void refresh();
    if (refreshMs <= 0) return;
    const timer = window.setInterval(refresh, refreshMs);
    return () => window.clearInterval(timer);
  }, [refresh, refreshMs]);

  const diskTotal = stats?.disks.reduce((a, d) => a + d.totalGb, 0) ?? 0;
  const diskUsed = stats?.disks.reduce((a, d) => a + (d.totalGb - d.availableGb), 0) ?? 0;
  const diskPercent = diskTotal > 0 ? (diskUsed / diskTotal) * 100 : 0;
  const curRx = stats?.interfaces.reduce((a, i) => a + i.rxRate, 0) ?? 0;
  const curTx = stats?.interfaces.reduce((a, i) => a + i.txRate, 0) ?? 0;
  const historyLabel = `最近 ${cpuHistory.length} 次采样`;
  const chart = (data: number[], color: string) =>
    data.length < 2 ? (
      <div className="chart-sampling">等待下一次采样…</div>
    ) : (
      <UsageChart data={data} color={color} />
    );

  return (
    <div className="view dashboard-view">
      <div className="view-head">
        <div>
          <div className="view-eyebrow">SYSTEM OVERVIEW</div>
          <h1>系统概览</h1>
          <p className="view-sub">掌握资源状态，让开发环境井然有序。</p>
        </div>
        <div className="head-tools dashboard-controls">
          <span
            className={`live-status ${error ? "unavailable" : refreshMs === 0 ? "paused" : ""}`}
          >
            <span className="workspace-dot" />
            {error ? "连接异常" : refreshMs > 0 ? "自动更新" : "已暂停"}
          </span>
          <select
            className="input select-inline"
            value={String(refreshMs)}
            onChange={(e) => setRefreshMs(Number(e.target.value))}
            aria-label="刷新间隔"
          >
            <option value="100">每 0.1 秒</option>
            <option value="500">每 0.5 秒</option>
            <option value="2000">每 2 秒</option>
            <option value="5000">每 5 秒</option>
            <option value="0">暂停更新</option>
          </select>
          <button
            className="btn-ghost icon-only"
            onClick={refresh}
            disabled={refreshing}
            title="现在刷新"
            aria-label="现在刷新"
          >
            <RefreshCw size={16} className={refreshing ? "refresh-spinning" : undefined} />
          </button>
        </div>
      </div>

      <div className="overview-section-heading">
        <h2>
          <Activity size={16} />
          资源监测
        </h2>
        <span>
          {stats
            ? `${stats.hostName || "本机"} · ${stats.osName || env?.os.name || "本地系统"}`
            : "本机资源"}
        </span>
      </div>
      {error && stats && (
        <div className="banner error resource-error" role="alert">
          <CircleAlert size={16} />
          <span>更新失败，当前显示上次采样数据。{error}</span>
          <button
            className="dismiss-banner"
            aria-label="关闭错误提示"
            onClick={() => setError(null)}
          >
            <X size={16} />
          </button>
        </div>
      )}
      {loading && !stats ? (
        <div className="usage-grid" role="status" aria-label="正在检测系统资源">
          {[0, 1, 2, 3].map((key) => (
            <div className="usage-card resource-skeleton" key={key}>
              <span />
              <span />
              <span />
            </div>
          ))}
        </div>
      ) : stats ? (
        <div className="usage-grid">
          <Card
            title="CPU"
            icon={<Cpu size={16} />}
            value={stats.cpuUsage.toFixed(1)}
            unit="%"
            meta={`${stats.cpuCores} 核心`}
            percent={stats.cpuUsage}
            color="var(--chart-cpu)"
            model={stats.cpuBrand ?? undefined}
            body={chart(cpuHistory, "var(--chart-cpu)")}
            footer={
              <>
                <span>处理器使用率</span>
                <span>{historyLabel}</span>
              </>
            }
          />
          <Card
            title="内存"
            icon={<MemoryStick size={16} />}
            value={stats.memoryUsedGb.toFixed(1)}
            unit="GB"
            meta={`共 ${stats.memoryTotalGb.toFixed(0)} GB`}
            percent={stats.memoryPercent}
            color="var(--chart-memory)"
            model={stats.memories.map((m) => `${m.title} · ${m.size}`).join(" / ") || undefined}
            body={chart(memoryHistory, "var(--chart-memory)")}
            footer={
              <>
                <span>已用 {stats.memoryPercent.toFixed(1)}%</span>
                <span>{historyLabel}</span>
              </>
            }
          />
          <Card
            title="磁盘"
            icon={<HardDrive size={16} />}
            value={diskUsed.toFixed(0)}
            unit="GB"
            meta={`共 ${diskTotal.toFixed(0)} GB`}
            percent={diskPercent}
            color="var(--chart-disk)"
            model={stats.storage.map((s) => `${s.title} · ${s.size}`).join(" / ") || undefined}
            body={chart(diskHistory, "var(--chart-disk)")}
            footer={
              <>
                <span>已用 {diskPercent.toFixed(1)}%</span>
                <span>{historyLabel}</span>
              </>
            }
          />
          <Card
            title="网络"
            icon={<Network size={16} />}
            value={fmtRate(curRx).split(" ")[0]}
            unit={fmtRate(curRx).split(" ")[1]}
            meta="实时速率"
            color="var(--chart-network)"
            model={stats.interfaces.map((i) => i.name).join(" / ") || "未检测到网卡"}
            body={
              rxHistory.length < 2 ? (
                <div className="chart-sampling">等待下一次采样…</div>
              ) : (
                <NetChart rx={rxHistory} tx={txHistory} />
              )
            }
            footer={
              <>
                <span className="network-legend down">
                  <ArrowDown size={12} />
                  下行
                </span>
                <span className="network-legend up">
                  <ArrowUp size={12} />
                  上行 {fmtRate(curTx)}
                </span>
              </>
            }
          />
        </div>
      ) : (
        <div className="overview-empty" role="status">
          <span className="overview-empty-icon">
            <ServerOff size={26} />
          </span>
          <h3>{error ? "暂时无法读取系统资源" : "等待系统数据"}</h3>
          <p>
            {error
              ? "请在 Z.Env 桌面应用中查看本机资源，或重试连接。"
              : "连接后会在这里展示处理器、内存、磁盘与网络状态。"}
          </p>
          {error && (
            <details className="resource-error-detail">
              <summary>查看错误详情</summary>
              <p>{error}</p>
            </details>
          )}
          <button className="btn" onClick={refresh} disabled={refreshing}>
            <RefreshCw size={14} />
            重新检测
          </button>
        </div>
      )}

      <div className="overview-details">
        <section className="panel device-panel">
          <h2 className="panel-title">
            <Monitor size={16} />
            设备信息
          </h2>
          <dl className="overview-info-list">
            <div>
              <dt>设备名称</dt>
              <dd>{stats?.hostName || "—"}</dd>
            </div>
            <div>
              <dt>操作系统</dt>
              <dd>{env ? `${env.os.name} ${env.os.version}` : stats?.osName || "—"}</dd>
            </div>
            <div>
              <dt>系统架构</dt>
              <dd>{env?.os.arch || "—"}</dd>
            </div>
            <div>
              <dt>图形处理器</dt>
              <dd>
                {stats
                  ? stats.gpus
                      .map((g) => (g.vram ? `${g.name} · ${g.vram}` : g.name))
                      .join(" / ") || "未检测到"
                  : "—"}
              </dd>
            </div>
          </dl>
          {stats?.battery && (
            <div className="device-battery">
              <BatteryCharging size={16} />
              <span>电池</span>
              <div className="bar-track">
                <div
                  className="bar-fill"
                  style={{ width: `${Math.min(100, Math.max(0, stats.battery.percent))}%` }}
                />
              </div>
              <span>
                {Math.round(stats.battery.percent)}% ·{" "}
                {stats.battery.charging ? "充电中" : "使用中"}
              </span>
            </div>
          )}
        </section>
        <section className="panel environment-panel">
          <div className="overview-panel-heading">
            <h2 className="panel-title">
              <Layers size={16} />
              环境接入
            </h2>
            <button className="overview-text-link" onClick={() => onNavigate("tools")}>
              管理环境
              <ChevronRight size={14} />
            </button>
          </div>
          <div className="mise-connection">
            <span className="mise-logo">
              m<span>_</span>
            </span>
            <div>
              <strong>mise</strong>
              <p>开发工具版本管理</p>
            </div>
            <span
              className={`connection-badge ${mise?.installed ? "connected" : miseError || mise ? "missing" : ""}`}
            >
              {mise?.installed ? (
                <>
                  <Check size={12} />
                  已接入
                </>
              ) : miseError ? (
                "检测失败"
              ) : mise ? (
                "未安装"
              ) : (
                "检测中…"
              )}
            </span>
          </div>
          {mise?.installed ? (
            <dl className="overview-info-list environment-info">
              <div>
                <dt>mise 版本</dt>
                <dd>{mise.version || "已安装"}</dd>
              </div>
              <div>
                <dt>Git</dt>
                <dd>{env?.git_version || "未检测到"}</dd>
              </div>
              <div>
                <dt>包管理器</dt>
                <dd>
                  {env
                    ? env.pkg
                        .filter((p) => p.available)
                        .map((p) => p.name)
                        .join(" · ") || "未检测到"
                    : "—"}
                </dd>
              </div>
            </dl>
          ) : (
            <div className="connection-help">
              <p>
                {miseError
                  ? "无法检测版本管理器，请在桌面应用中重试。"
                  : mise
                    ? "安装 mise 后，即可统一管理本机运行时版本。"
                    : "正在检测本机的版本管理器…"}
              </p>
              {mise && !mise.installed && (
                <a
                  href="https://mise.jdx.dev/getting-started.html"
                  target="_blank"
                  rel="noreferrer"
                >
                  查看安装指南
                  <ArrowUpRight size={14} />
                </a>
              )}
            </div>
          )}
        </section>
      </div>

      <div className="overview-section-heading shortcuts-heading">
        <h2>开始工作</h2>
        <span>常用入口</span>
      </div>
      <div className="overview-shortcuts">
        {SHORTCUTS.map(({ tab, icon: Icon, title, description }) => (
          <button className="overview-shortcut" key={tab} onClick={() => onNavigate(tab)}>
            <span className="shortcut-icon">
              <Icon size={19} />
            </span>
            <span className="shortcut-copy">
              <strong>{title}</strong>
              <span>{description}</span>
            </span>
            <ArrowUpRight size={16} className="shortcut-arrow" />
          </button>
        ))}
      </div>
    </div>
  );
}
