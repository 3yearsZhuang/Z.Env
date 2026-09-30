import { useEffect, useMemo, useState, type ReactNode } from "react";
import { listTools, listRegistry, ToolInfo } from "../api";
import { KNOWN_RUNTIMES } from "../data/catalog";
import { getLogs, LogEntry } from "../logger";

/** 真实收款码图片路径（放入 public/donate/ 下即可自动嵌入；缺失则回退占位假码） */
const DONATE_IMGS = {
  wechat: "/donate/wechat.png",
  alipay: "/donate/alipay.jpg",
};

function fmtTime(t: number): string {
  const d = new Date(t);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

// 确定性 PRNG，保证二维码码格固定（不会每次刷新都变）
function mulberry32(a: number) {
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function makeGrid(seed: number, size = 25): boolean[][] {
  const rnd = mulberry32(seed);
  const g = Array.from({ length: size }, () => Array(size).fill(false));
  const inFinder = (fx: number, fy: number, x: number, y: number) =>
    x >= fx && x < fx + 7 && y >= fy && y < fy + 7;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      if (inFinder(0, 0, x, y) || inFinder(size - 7, 0, x, y) || inFinder(0, size - 7, x, y)) {
        continue;
      }
      g[y][x] = rnd() < 0.48;
    }
  }
  return g;
}

/** 3 个 QR 定位角中的一个（7×7） */
function Finder({ x, y, cw }: { x: number; y: number; cw: number }) {
  const inner = cw * 3;
  return (
    <g>
      <rect x={x} y={y} width={cw * 7} height={cw * 7} fill="#111" />
      <rect x={x + cw} y={y + cw} width={cw * 5} height={cw * 5} fill="#fff" />
      <rect x={x + cw * 2} y={y + cw * 2} width={inner} height={inner} fill="#111" />
    </g>
  );
}

/** 模拟“真实收款码”的二维码占位图 */
function PayQr({ seed, mark, tint }: { seed: number; mark: string; tint: string }) {
  const size = 25;
  const cw = 100 / size;
  const grid = useMemo(() => makeGrid(seed, size), [seed]);
  const cx = size / 2;
  const l = (cx - 2) * cw;
  const w = 5 * cw;
  return (
    <svg viewBox="0 0 100 100" width="150" height="150">
      <rect width="100" height="100" fill="#fff" rx="6" />
      {grid.map((row, y) =>
        row.map((on, x) =>
          on ? (
            <rect key={`${x}-${y}`} x={x * cw} y={y * cw} width={cw} height={cw} fill="#111" />
          ) : null,
        ),
      )}
      <Finder x={0} y={0} cw={cw} />
      <Finder x={(size - 7) * cw} y={0} cw={cw} />
      <Finder x={0} y={(size - 7) * cw} cw={cw} />
      {/* 中央 logo 占位 */}
      <rect x={l} y={l} width={w} height={w} fill="#fff" stroke={tint} strokeWidth="1.5" rx="4" />
      <text x="50" y="56" textAnchor="middle" fontSize="18" fontWeight="800" fill={tint}>
        {mark}
      </text>
    </svg>
  );
}

/** 打赏图：优先显示真实收款码图片，加载失败则回退占位假码 */
function DonateImg({ src, fallback }: { src: string; fallback: ReactNode }) {
  const [fail, setFail] = useState(false);
  if (!fail) {
    return (
      <img
        src={src}
        alt="收款码"
        className="donate-real"
        draggable={false}
        onError={() => setFail(true)}
      />
    );
  }
  return <>{fallback}</>;
}

export default function EasterEgg({ onClose }: { onClose: () => void }) {
  const [registry, setRegistry] = useState<string[]>([]);
  const [installed, setInstalled] = useState<string[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);

  // 自检
  useEffect(() => {
    listRegistry()
      .then(setRegistry)
      .catch(() => setRegistry([]));
    listTools()
      .then((t: ToolInfo[]) => setInstalled(t.map((x) => x.name)))
      .catch(() => setInstalled([]));
  }, []);

  // 日志实时刷新
  useEffect(() => {
    const t = setInterval(() => setLogs(getLogs()), 700);
    return () => clearInterval(t);
  }, []);

  const coveredSet = new Set([...installed, ...KNOWN_RUNTIMES]);
  const coveredCount = registry.filter((n) => coveredSet.has(n)).length;
  const missing = registry.filter((n) => !coveredSet.has(n));

  // 防止 ESC/overlay 点击误关由外部控制，这里不自动关闭

  return (
    <div className="overlay easter-overlay" onClick={onClose}>
      <div className="easter" onClick={(e) => e.stopPropagation()}>
        <div className="easter-head">
          <div>
            <h2>🥚 彩蛋</h2>
            <p className="view-sub">解锁隐藏面板</p>
          </div>
          <button className="btn-close" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="easter-body">
          {/* 1. 运行时覆盖自检 */}
          <section className="panel">
            <h2 className="panel-title">运行时覆盖自检</h2>
            <div
              className={`selfcheck ${missing.length ? "warn" : "ok"}`}
              style={{ marginBottom: 0 }}
            >
              <div className="selfcheck-head">
                <span className="pill muted small">自检</span>
                <span className="selfcheck-text">
                  已收录 <strong>{coveredCount}</strong>/{registry.length} 个 mise 支持的运行时
                  {missing.length ? `，未覆盖 ${missing.length} 个` : "，完整覆盖"}
                </span>
              </div>
              {missing.length > 0 && (
                <div className="selfcheck-missing" title={missing.join("、")}>
                  {missing.slice(0, 18).join("、")}
                  {missing.length > 18 ? " …" : ""}
                </div>
              )}
            </div>
          </section>

          {/* 2. 日志系统 */}
          <section className="panel">
            <h2 className="panel-title">日志系统</h2>
            <div className="easter-logs">
              {logs.length === 0 ? (
                <div className="empty small">暂无日志</div>
              ) : (
                logs
                  .slice()
                  .reverse()
                  .map((l, i) => (
                    <div className="log-row" key={`${l.time}-${i}`}>
                      <span className={`log-level ${l.level}`}>{l.level}</span>
                      <span className="log-time">{fmtTime(l.time)}</span>
                      <span className="log-msg">{l.msg}</span>
                    </div>
                  ))
              )}
            </div>
          </section>

          {/* 3. 打赏（占位图片） */}
          <section className="panel">
            <h2 className="panel-title">打赏支持</h2>
            <p className="setting-note">如果 Z.Env 帮到了你，欢迎打赏支持。</p>
            <div className="donate-grid">
              <div className="donate-card">
                <span className="donate-label">微信 · 赞赏</span>
                <div className="donate-qr">
                  <DonateImg
                    src={DONATE_IMGS.wechat}
                    fallback={<PayQr seed={20240824} mark="赞" tint="#2ba24c" />}
                  />
                </div>
                <span className="donate-hint">长按识别二维码</span>
              </div>
              <div className="donate-card">
                <span className="donate-label">支付宝 · 赞赏</span>
                <div className="donate-qr">
                  <DonateImg
                    src={DONATE_IMGS.alipay}
                    fallback={<PayQr seed={317370} mark="FUND" tint="#1677ff" />}
                  />
                </div>
                <span className="donate-hint">长按识别二维码</span>
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
