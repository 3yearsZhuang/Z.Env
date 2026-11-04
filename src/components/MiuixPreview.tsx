import { useState } from "react";
import { Gauge, Layers, Moon, Settings, Sun, Wifi, BatteryFull, ChevronRight } from "lucide-react";
import "./MiuixPreview.css";

type Mode = "light" | "dark";

const NAV = [
  { icon: Gauge, label: "系统概览" },
  { icon: Layers, label: "本机工具" },
  { icon: Settings, label: "设置" },
];

/** MiuiX / HyperOS 风格预览：仅用于视觉验收，不接任何后端命令。 */
export default function MiuixPreview() {
  const [mode, setMode] = useState<Mode>("light");
  const [nav, setNav] = useState(0);
  const [toggle, setToggle] = useState(true);

  return (
    <div className={`miuix ${mode === "dark" ? "dark" : ""}`}>
      <div className="miuix-frame">
        <div className="miuix-statusbar">
          <span>09:41</span>
          <span className="miuix-statusbar-right">
            <Wifi size={13} />
            <BatteryFull size={15} />
          </span>
        </div>
        <div className="miuix-layout">
          <aside className="miuix-sidebar">
            <div className="miuix-brand">
              <span className="miuix-brand-mark">Z</span>
              <span className="miuix-brand-text">
                <strong>Z.Env</strong>
                <small>环境管理</small>
              </span>
            </div>
            <nav className="miuix-nav">
              {NAV.map(({ icon: Icon, label }, index) => (
                <button
                  key={label}
                  className={`miuix-nav-item ${nav === index ? "active" : ""}`}
                  onClick={() => setNav(index)}
                >
                  <Icon size={18} />
                  <span>{label}</span>
                </button>
              ))}
            </nav>
            <div className="miuix-sidebar-foot">
              <button
                className="miuix-theme"
                onClick={() => setMode(mode === "light" ? "dark" : "light")}
              >
                {mode === "light" ? <Moon size={16} /> : <Sun size={16} />}
                <span>{mode === "light" ? "深色模式" : "浅色模式"}</span>
              </button>
              <small>v0.7.1</small>
            </div>
          </aside>

          <main className="miuix-content">
            <header className="miuix-head">
              <div>
                <h1>系统概览</h1>
                <p>本机运行时、包管理器与项目环境</p>
              </div>
              <button className="miuix-btn primary">重新扫描</button>
            </header>

            <section className="miuix-hero">
              <div className="miuix-hero-main">
                <span className="miuix-hero-label">当前环境</span>
                <strong className="miuix-hero-value">Node 22.23.3</strong>
                <span className="miuix-hero-meta">mise · 已验证 · arm64</span>
              </div>
              <div className="miuix-hero-ring">
                <span>92</span>
                <small>健康度</small>
              </div>
            </section>

            <section className="miuix-card">
              <div className="miuix-card-title">
                <h2>本机工具</h2>
                <button className="miuix-link">
                  全部 <ChevronRight size={14} />
                </button>
              </div>
              {[
                { name: "Node.js", meta: "mise · 22.23.3", pill: "已验证", tone: "ok" },
                { name: "Python", meta: "Homebrew · 3.13.18", pill: "已验证", tone: "ok" },
                { name: "pnpm", meta: "未安装", pill: "缺失", tone: "muted" },
              ].map((row) => (
                <div className="miuix-row" key={row.name}>
                  <span className="miuix-row-icon">{row.name.slice(0, 1)}</span>
                  <span className="miuix-row-text">
                    <strong>{row.name}</strong>
                    <small>{row.meta}</small>
                  </span>
                  <span className={`miuix-pill ${row.tone}`}>{row.pill}</span>
                </div>
              ))}
            </section>

            <section className="miuix-grid">
              <div className="miuix-card">
                <div className="miuix-card-title">
                  <h2>设置项</h2>
                </div>
                <label className="miuix-field">
                  <span>安装通道</span>
                  <select defaultValue="mise">
                    <option value="mise">mise</option>
                    <option value="brew">Homebrew</option>
                  </select>
                </label>
                <label className="miuix-switch-row">
                  <span>
                    <strong>跟随系统主题</strong>
                    <small>自动切换浅色与深色</small>
                  </span>
                  <button
                    className={`miuix-switch ${toggle ? "on" : ""}`}
                    role="switch"
                    aria-checked={toggle}
                    onClick={() => setToggle(!toggle)}
                  >
                    <i />
                  </button>
                </label>
              </div>
              <div className="miuix-card">
                <div className="miuix-card-title">
                  <h2>状态</h2>
                </div>
                <div className="miuix-banner success">✓ 清单读取完成，覆盖 5 个来源</div>
                <div className="miuix-banner warn">⚠ pyenv 未安装，仅展示 PATH 证据</div>
                <div className="miuix-banner error">✕ 1 个绑定冲突，需人工处理</div>
              </div>
            </section>

            <section className="miuix-card">
              <div className="miuix-card-title">
                <h2>操作</h2>
              </div>
              <div className="miuix-actions">
                <button className="miuix-btn">预览计划</button>
                <button className="miuix-btn primary">执行安装</button>
                <button className="miuix-btn danger">卸载</button>
                <button className="miuix-btn" disabled>
                  不可用
                </button>
              </div>
              <div className="miuix-chips">
                <span className="miuix-chip">mise</span>
                <span className="miuix-chip">Homebrew</span>
                <span className="miuix-chip muted">pyenv</span>
              </div>
            </section>
          </main>
        </div>
      </div>
    </div>
  );
}
