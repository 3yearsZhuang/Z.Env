import TaskPanel from "./components/TaskPanel";
import {
  lazy,
  Suspense,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import {
  FolderKanban,
  Gauge,
  Layers,
  LoaderCircle,
  Monitor,
  Moon,
  ChevronRight,
  PanelLeftClose,
  PanelLeftOpen,
  Server,
  Settings,
  Sun,
} from "lucide-react";
import Dashboard from "./components/Dashboard";
import { initLogger, logInfo } from "./logger";
import type { Tab } from "./lib/nav";
import "./index.css";
import "./styles.css";
import "./App.css";

// 概览优先展示；其他页面首次访问时加载，已访问的编辑页面继续保留会话。
const EnvironmentView = lazy(() => import("./components/EnvironmentView"));
const ProjectsView = lazy(() => import("./components/ProjectsView"));
const SettingsView = lazy(() => import("./components/SettingsView"));
const ServicesCachesView = lazy(() => import("./components/ServicesCachesView"));
const EasterEgg = lazy(() => import("./components/EasterEgg"));

function PageContent({ name, children }: { name: string; children: ReactNode }) {
  return (
    <Suspense
      fallback={
        <div className="workspace-loading" role="status">
          <LoaderCircle size={22} className="workspace-spinner" aria-hidden="true" />
          <span>正在加载{name}…</span>
        </div>
      }
    >
      {children}
    </Suspense>
  );
}

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
  { key: "tools", label: "本机工具", icon: Layers },
  { key: "projects", label: "项目配置", icon: FolderKanban },
  { key: "ops", label: "服务与缓存", icon: Server },
  { key: "settings", label: "设置", icon: Settings },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [compatibilityRequest, setCompatibilityRequest] = useState(0);
  const contentRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (contentRef.current) contentRef.current.scrollTop = 0;
  }, [tab]);
  const [visited, setVisited] = useState<Set<string>>(new Set());
  useEffect(() => {
    setVisited((v) => new Set([...v, tab]));
  }, [tab]);
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem("zenv-theme") as Theme | null;
    return saved === "light" || saved === "dark" ? saved : "auto";
  });
  const [desktopCollapsed, setDesktopCollapsed] = useState(false);
  const [compactExpanded, setCompactExpanded] = useState(false);
  const [compact, setCompact] = useState(() => window.matchMedia("(max-width: 900px)").matches);
  const collapsed = compact ? !compactExpanded : desktopCollapsed;
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

  // 只在跨越断点时收起窄窗导航；同一区间内拖动窗口不会覆盖用户的展开选择。
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 900px)");
    const onResize = () => {
      setCompact(mq.matches);
      setCompactExpanded(false);
    };
    mq.addEventListener("change", onResize);
    return () => mq.removeEventListener("change", onResize);
  }, []);

  useEffect(() => {
    if (!compactExpanded) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setCompactExpanded(false);
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [compactExpanded]);

  const toggleSidebar = () => {
    if (compact) setCompactExpanded((value) => !value);
    else setDesktopCollapsed((value) => !value);
  };

  const ThemeIcon = THEME_ICON[theme];
  const currentPage = NAV.find((item) => item.key === tab)!;
  const navigate = (next: Tab) => {
    if (next === "software") {
      setCompatibilityRequest((request) => request + 1);
      setTab("tools");
    } else setTab(next);
    if (compact) setCompactExpanded(false);
  };

  return (
    <div className="app">
      <a className="workspace-skip-link" href="#workspace-content">
        跳到主要内容
      </a>
      {compactExpanded && compact && (
        <button
          className="workspace-nav-backdrop"
          aria-label="收起导航"
          onClick={() => setCompactExpanded(false)}
          tabIndex={-1}
        />
      )}
      <aside id="workspace-navigation" className={`sidebar ${collapsed ? "collapsed" : ""}`}>
        <button
          className="brand clickable"
          onClick={toggleSidebar}
          title={collapsed ? "展开侧边栏" : "收起侧边栏"}
          aria-label={collapsed ? "展开侧边栏" : "收起侧边栏"}
          aria-expanded={!collapsed}
          aria-controls="workspace-navigation"
        >
          <span className="brand-mark">
            <img src="/zenv-icon.svg" alt="Z.Env" draggable={false} />
          </span>
          <span className="brand-text">
            <span className="brand-title">Z.Env</span>
            <span className="brand-sub">环境管理</span>
          </span>
        </button>

        <div className="nav-section-label">工作空间</div>
        <nav className="nav" aria-label="主导航">
          {NAV.filter((item) => item.key !== "settings").map(({ key, label, icon: Icon }) => (
            <button
              key={key}
              className={`nav-item ${tab === key ? "active" : ""}`}
              onClick={() => navigate(key)}
              aria-label={label}
              aria-current={tab === key ? "page" : undefined}
              title={label}
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
            className={`nav-item ${tab === "settings" ? "active" : ""}`}
            onClick={() => navigate("settings")}
            aria-label="设置"
            aria-current={tab === "settings" ? "page" : undefined}
            title="设置"
          >
            <span className="nav-icon">
              <Settings size={17} />
            </span>
            <span className="nav-label">设置</span>
          </button>
          <button
            className="theme-toggle"
            onClick={() => changeTheme(THEME_NEXT[theme])}
            title={"切换主题：" + THEME_LABEL[theme]}
            aria-label={`当前主题：${THEME_LABEL[theme]}，切换为${THEME_LABEL[THEME_NEXT[theme]]}`}
          >
            <span className="theme-icon">
              <ThemeIcon size={14} />
            </span>
            <span className="theme-label">{THEME_LABEL[theme]}</span>
          </button>
          <div className="footer-tag">
            <span className="workspace-dot" />
            本地环境管理
          </div>
        </div>
      </aside>

      <main className="workspace">
        <header className="workspace-header">
          <div className="workspace-breadcrumb">
            <button
              className="sidebar-toggle"
              onClick={toggleSidebar}
              aria-label={collapsed ? "展开导航" : "收起导航"}
              aria-expanded={!collapsed}
              aria-controls="workspace-navigation"
              title={collapsed ? "展开导航" : "收起导航"}
            >
              {collapsed ? <PanelLeftOpen size={18} /> : <PanelLeftClose size={18} />}
            </button>
            <span className="breadcrumb-root">工作空间</span>
            <ChevronRight size={13} aria-hidden="true" />
            <span className="breadcrumb-current">{currentPage.label}</span>
          </div>
          <span className="workspace-local">
            <Monitor size={14} />
            本地工作台
          </span>
        </header>
        <div
          id="workspace-content"
          ref={contentRef}
          className="content"
          tabIndex={-1}
          aria-label={currentPage.label}
        >
          {tab === "dashboard" && <Dashboard onNavigate={navigate} />}
          {(tab === "tools" || visited.has("tools")) && (
            <div hidden={tab !== "tools"}>
              <PageContent name="本机工具">
                <EnvironmentView
                  onNavigate={navigate}
                  active={tab === "tools"}
                  compatibilityRequest={compatibilityRequest}
                />
              </PageContent>
            </div>
          )}
          {(tab === "projects" || visited.has("projects")) && (
            <div hidden={tab !== "projects"}>
              <PageContent name="项目配置">
                <ProjectsView onNavigate={navigate} active={tab === "projects"} />
              </PageContent>
            </div>
          )}
          {tab === "ops" && (
            <PageContent name="服务与缓存">
              <ServicesCachesView />
            </PageContent>
          )}
          {tab === "settings" && (
            <PageContent name="设置">
              <SettingsView onEaster={() => setEaster(true)} />
            </PageContent>
          )}
        </div>
      </main>

      <TaskPanel />
      {easter && (
        <Suspense fallback={null}>
          <EasterEgg onClose={() => setEaster(false)} />
        </Suspense>
      )}
    </div>
  );
}
