import { useEffect, useState, type ComponentType } from "react";
import {
  Blocks,
  FolderKanban,
  Gauge,
  Layers,
  Monitor,
  Moon,
  Server,
  Settings,
  Sun,
} from "lucide-react";
import Dashboard from "./components/Dashboard";
import EnvironmentView from "./components/EnvironmentView";
import ProjectsView from "./components/ProjectsView";
import SettingsView from "./components/SettingsView";
import SoftwareView from "./components/SoftwareView";
import ServicesCachesView from "./components/ServicesCachesView";
import EasterEgg from "./components/EasterEgg";
import { initLogger, logInfo } from "./logger";
import type { Tab } from "./lib/nav";
import "./index.css";
import "./styles.css";

type Theme = "auto" | "light" | "dark";
const THEME_NEXT: Record<Theme, Theme> = {
  auto: "light",
  light: "dark",
  dark: "auto",
};
const THEME_LABEL: Record<Theme, string> = {
  auto: "跟随系统",
  light: "浅色",
  dark: "深色",
};
const THEME_ICON: Record<Theme, ComponentType<{ size?: number }>> = {
  auto: Monitor,
  light: Sun,
  dark: Moon,
};

/** 解析生效主题：auto 跟随系统，其余用所选值 */
function resolveTheme(t: Theme): "light" | "dark" {
  if (t !== "auto") return t;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

const NAV: { key: Tab; label: string; icon: ComponentType<{ size?: number }> }[] = [
  { key: "dashboard", label: "系统概览", icon: Gauge },
  { key: "software", label: "支持列表", icon: Blocks },
  { key: "tools", label: "环境", icon: Layers },
  { key: "projects", label: "项目配置", icon: FolderKanban },
  { key: "ops", label: "服务与缓存", icon: Server },
  { key: "settings", label: "设置", icon: Settings },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem("zenv-theme") as Theme | null;
    return saved === "light" || saved === "dark" ? saved : "auto";
  });
  const [collapsed, setCollapsed] = useState(false);
  const [easter, setEaster] = useState(false);

  // 初始化日志采集（仅一次）
  useEffect(() => {
    initLogger();
  }, []);
  useEffect(() => {
    if (easter) logInfo("打开了彩蛋面板");
  }, [easter]);

  const changeTheme = (t: Theme) => {
    localStorage.setItem("zenv-theme", t);
    setTheme(t);
  };

  // 侧边栏玻璃设计（全平台），主操作区仍实色
  useEffect(() => {
    document.body.classList.add("glass");
  }, []);

  // 明暗主题：auto 跟随系统（监听系统切换实时生效）。
  // 解析结果写入 <html>.dark 类驱动设计令牌切换，data-theme 同步给个别旧样式。
  useEffect(() => {
    const apply = () => {
      const effective = resolveTheme(theme);
      document.documentElement.classList.toggle("dark", effective === "dark");
      document.body.dataset.theme = effective;
    };
    apply();
    if (theme !== "auto") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  // 响应式：窄窗口自动复用"点击收起"逻辑（仅图标）
  useEffect(() => {
    const onResize = () => {
      if (window.innerWidth <= 900) setCollapsed(true);
    };
    onResize();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const ThemeIcon = THEME_ICON[theme];

  return (
    <div className="app">
      <aside className={`sidebar ${collapsed ? "collapsed" : ""}`}>
        <div
          className="brand clickable"
          onClick={() => setCollapsed((c) => !c)}
          title={collapsed ? "展开侧边栏" : "收起侧边栏"}
        >
          <div className="brand-mark">
            <img src="/zenv-icon.svg" alt="Z.Env" draggable={false} />
          </div>
          <div className="brand-text">
            <span className="brand-title">Z.Env</span>
            <span className="brand-sub">环境管理</span>
          </div>
        </div>

        <nav className="nav">
          {NAV.map(({ key, label, icon: Icon }) => (
            <button
              key={key}
              className={`nav-item ${tab === key ? "active" : ""}`}
              onClick={() => setTab(key)}
            >
              <span className="nav-icon">
                <Icon size={17} />
              </span>
              <span className="nav-label">{label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <button
            className="theme-toggle"
            onClick={() => changeTheme(THEME_NEXT[theme])}
            title={"切换主题：" + THEME_LABEL[theme]}
          >
            <span className="theme-icon">
              <ThemeIcon size={14} />
            </span>
            <span className="theme-label">{THEME_LABEL[theme]}</span>
          </button>
          <div className="footer-tag">mise · 版本管理</div>
        </div>
      </aside>

      <main className="content">
        {tab === "dashboard" && <Dashboard />}
        {tab === "tools" && <EnvironmentView onNavigate={setTab} />}
        {tab === "projects" && <ProjectsView onNavigate={setTab} />}
        {tab === "ops" && <ServicesCachesView />}
        {tab === "settings" && <SettingsView onEaster={() => setEaster(true)} />}
        {tab === "software" && <SoftwareView />}
      </main>

      {easter && <EasterEgg onClose={() => setEaster(false)} />}
    </div>
  );
}
