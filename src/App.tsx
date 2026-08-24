import { useEffect, useState } from "react";
import Dashboard from "./components/Dashboard";
import ToolsView from "./components/ToolsView";
import ProjectsView from "./components/ProjectsView";
import SettingsView from "./components/SettingsView";
import EasterEgg from "./components/EasterEgg";
import { initLogger, logInfo } from "./logger";
import "./styles.css";

type Tab = "dashboard" | "tools" | "projects" | "settings";
type Theme = "auto" | "light" | "dark";
const THEME_NEXT: Record<Theme, Theme> = {
  auto: "light",
  light: "dark",
  dark: "auto",
};
const THEME_ICON: Record<Theme, string> = { auto: "◐", light: "☀", dark: "☾" };
const THEME_LABEL: Record<Theme, string> = {
  auto: "跟随系统",
  light: "浅色",
  dark: "深色",
};

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

  // 日夜主题：auto 跟随系统
  useEffect(() => {
    document.body.dataset.theme = theme;
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

  return (
    <div className="app">
      <aside className={`sidebar ${collapsed ? "collapsed" : ""}`}>
        <div className="brand clickable" onClick={() => setCollapsed((c) => !c)} title={collapsed ? "展开侧边栏" : "收起侧边栏"}>
          <div className="brand-mark">
            <img src="/zenv-icon.svg" alt="Z.Env" draggable={false} />
          </div>
          <div className="brand-text">
            <span className="brand-title">Z.Env</span>
            <span className="brand-sub">运行时管理</span>
          </div>
        </div>

        <nav className="nav">
          <button
            className={`nav-item ${tab === "dashboard" ? "active" : ""}`}
            onClick={() => setTab("dashboard")}
          >
            <span className="nav-icon">◉</span>
            <span className="nav-label">系统概览</span>
          </button>
          <button
            className={`nav-item ${tab === "tools" ? "active" : ""}`}
            onClick={() => setTab("tools")}
          >
            <span className="nav-icon">▤</span>
            <span className="nav-label">运行时工具</span>
          </button>
          <button
            className={`nav-item ${tab === "projects" ? "active" : ""}`}
            onClick={() => setTab("projects")}
          >
            <span className="nav-icon">▰</span>
            <span className="nav-label">项目配置</span>
          </button>
          <button
            className={`nav-item ${tab === "settings" ? "active" : ""}`}
            onClick={() => setTab("settings")}
          >
            <span className="nav-icon">⚙</span>
            <span className="nav-label">设置</span>
          </button>
        </nav>

        <div className="sidebar-footer">
          <button
            className="theme-toggle"
            onClick={() => changeTheme(THEME_NEXT[theme])}
            title={"切换主题：" + THEME_LABEL[theme]}
          >
            <span className="theme-icon">{THEME_ICON[theme]}</span>
            <span className="theme-label">{THEME_LABEL[theme]}</span>
          </button>
          <div className="footer-tag">mise · 版本管理</div>
        </div>
      </aside>

      <main className="content">
        {tab === "dashboard" && <Dashboard />}
        {tab === "tools" && <ToolsView />}
        {tab === "projects" && <ProjectsView />}
        {tab === "settings" && <SettingsView onEaster={() => setEaster(true)} />}
      </main>

      {easter && <EasterEgg onClose={() => setEaster(false)} />}
    </div>
  );
}