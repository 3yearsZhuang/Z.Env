import { lazy, Suspense, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  readProjectConfig,
  writeProjectConfig,
  installAllProject,
  detectToolSources,
  stableBinPath,
  discoverProjects,
  errorMessage,
  ToolSource,
  type DiscoveredProject,
} from "../api";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";

// 编辑器整体懒加载：CodeMirror 及其 TOML 语言包只在首次进入项目配置页时拉取
const TomlEditor = lazy(() => import("./TomlEditor"));

/** 常见技术栈环境预设：一键生成全套运行环境 */
const PRESETS: {
  id: string;
  name: string;
  desc: string;
  tools: Record<string, string>;
}[] = [
  {
    id: "node",
    name: "Node 前端",
    desc: "React / Vite / 前端工程",
    tools: { node: "20", typescript: "latest", bun: "latest", deno: "latest" },
  },
  {
    id: "python",
    name: "Python 数据",
    desc: "数据分析 / 脚本 / AI",
    tools: { python: "3.13", uv: "latest" },
  },
  {
    id: "java",
    name: "Java 后端",
    desc: "Spring Boot / JVM",
    tools: { java: "temurin-21", maven: "latest", gradle: "latest", kotlin: "latest" },
  },
  {
    id: "go",
    name: "Go 服务",
    desc: "微服务 / CLI",
    tools: { go: "1.27", gofumpt: "latest", staticcheck: "latest" },
  },
  {
    id: "ruby",
    name: "Ruby",
    desc: "Rails / 脚本",
    tools: { ruby: "4.0", gem: "latest", bundler: "latest" },
  },
  {
    id: "fullstack",
    name: "全栈",
    desc: "Node + Python + Go",
    tools: { node: "20", python: "3.13", go: "1.27", java: "temurin-21" },
  },
];

function presetToToml(tools: Record<string, string>): string {
  const rows = Object.entries(tools)
    .map(([k, v]) => `${k} = "${v}"`)
    .join("\n");
  return `# 由 Mise GUI 环境预设生成\n[tools]\n${rows}\n`;
}

/** 监听 <html>.dark 类变化，让编辑器主题跟随应用明暗切换 */
function useDarkMode(): boolean {
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));
  useEffect(() => {
    const ob = new MutationObserver(() =>
      setDark(document.documentElement.classList.contains("dark")),
    );
    ob.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => ob.disconnect();
  }, []);
  return dark;
}

export default function ProjectsView() {
  const [path, setPath] = useState("");
  const [content, setContent] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [userPresets, setUserPresets] = useState<{ name: string; content: string }[]>([]);
  const dark = useDarkMode();

  // 整机环境绑定（策略 C）：把 brew/scoop 安装的运行时以 PATH 方式绑定进项目 mise.toml
  const [bindOpen, setBindOpen] = useState(false);
  const [bindSources, setBindSources] = useState<ToolSource[]>([]);
  const [bindBusy, setBindBusy] = useState<string | null>(null);

  // 项目发现：扫描常用目录，点击即载入编辑流程
  const [discovered, setDiscovered] = useState<DiscoveredProject[] | null>(null);
  const [scanning, setScanning] = useState(false);

  function openBindDialog() {
    detectToolSources()
      .then((all) => setBindSources(all.filter((s) => ["brew", "scoop"].includes(s.manager))))
      .catch(() => setBindSources([]));
    setBindOpen(true);
  }

  /** 把稳定 bin 路径写进当前 mise.toml：已有 _.path 则并入数组，已有 [env] 则插入，否则追加 */
  async function bindSource(s: ToolSource) {
    setBindBusy(`${s.manager}::${s.tool}`);
    try {
      const bin = await stableBinPath(s.manager, s.tool, s.path);
      const pathLine = `_.path = ["${bin}"]`;
      setContent((prev) => {
        if (/^_.path = \[.*\]$/m.test(prev)) {
          return prev.replace(/^_.path = \[(.*)\]$/m, `_.path = [$1, "${bin}"]`);
        }
        if (/^\[env\]$/m.test(prev)) {
          return prev.replace(/^\[env\]$/m, `[env]\n${pathLine}`);
        }
        return `${prev.trimEnd()}\n\n[env]\n${pathLine}\n`;
      });
      setNotice(`已把 ${s.tool}（${s.manager}）以 PATH 方式绑定进当前配置，保存后生效`);
      setBindOpen(false);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBindBusy(null);
    }
  }

  const PRESET_STORAGE_KEY = "mise-gui:user-presets";
  // 启动时读取本地保存的用户预设
  useEffect(() => {
    try {
      const raw = localStorage.getItem(PRESET_STORAGE_KEY);
      if (raw) setUserPresets(JSON.parse(raw));
    } catch {
      /* 忽略损坏数据 */
    }
  }, []);

  function persistPresets(list: { name: string; content: string }[]) {
    setUserPresets(list);
    try {
      localStorage.setItem(PRESET_STORAGE_KEY, JSON.stringify(list));
    } catch {
      /* 存储满等忽略 */
    }
  }

  /** 把当前配置保存为用户预设 */
  function saveUserPreset() {
    const name = window.prompt("为这个自定义预设命名：", "我的预设");
    if (!name || !name.trim()) return;
    const trimmed = name.trim();
    const list = [...userPresets.filter((p) => p.name !== trimmed), { name: trimmed, content }];
    persistPresets(list);
    setNotice(`已保存用户预设「${trimmed}」`);
  }

  function loadUserPreset(name: string) {
    const item = userPresets.find((p) => p.name === name);
    if (!item) return;
    setContent(item.content);
    setLoaded(true);
    setError(null);
    setNotice(`已载入用户预设「${name}」`);
  }

  function deleteUserPreset(name: string) {
    persistPresets(userPresets.filter((p) => p.name !== name));
    setNotice(`已删除用户预设「${name}」`);
  }

  function applyPreset(tools: Record<string, string>, name: string) {
    setContent(presetToToml(tools));
    setLoaded(true);
    setError(null);
    setNotice(`已载入「${name}」预设配置，可编辑后保存，或直接一键安装`);
  }

  async function handleInstallAll() {
    if (!path.trim()) {
      setError("请先选择项目目录");
      return;
    }
    setInstalling(true);
    setError(null);
    setNotice(null);
    try {
      const msg = await installAllProject(path.trim(), content);
      setNotice(msg);
      setLoaded(true);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setInstalling(false);
    }
  }

  async function handleLoad() {
    if (!path.trim()) {
      setError("请输入项目目录或 mise.toml 文件路径");
      setLoaded(false);
      return;
    }
    await loadProjectAt(path.trim());
  }

  /** 按给定路径读取配置（发现列表与手动输入共用） */
  async function loadProjectAt(p: string) {
    setLoading(true);
    setError(null);
    setNotice(null);
    try {
      const c = await readProjectConfig(p);
      setContent(c);
      setLoaded(true);
      setNotice("已加载配置（若为只读展示，可编辑后保存）");
    } catch (e) {
      setError(errorMessage(e));
      setLoaded(false);
    } finally {
      setLoading(false);
    }
  }

  /** 扫描常用目录发现项目（只读） */
  function rescan() {
    setScanning(true);
    discoverProjects()
      .then(setDiscovered)
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setScanning(false));
  }

  // 进入页面即自动扫描一次
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(rescan, []);

  async function handlePickDir() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择项目目录",
    });
    if (typeof selected === "string" && selected) {
      setPath(selected);
    }
  }

  async function handleSave() {
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const msg = await writeProjectConfig(path.trim(), content);
      setNotice(msg);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>项目配置</h1>
          <p className="view-sub">
            以图形方式查看和编辑项目的 <code>mise.toml</code>
          </p>
        </div>
      </div>

      {error && (
        <div className="banner error" onClick={() => setError(null)}>
          ⚠ {error}
        </div>
      )}
      {notice && (
        <div className="banner success" onClick={() => setNotice(null)}>
          ✓ {notice}
        </div>
      )}

      <div className="preset-section">
        <h2 className="section-title">环境预设</h2>
        <p className="preset-hint">选择技术栈，一键生成并安装全套运行环境</p>
        <div className="preset-grid">
          {PRESETS.map((p) => (
            <button key={p.id} className="preset-card" onClick={() => applyPreset(p.tools, p.name)}>
              <span className="preset-name">{p.name}</span>
              <span className="preset-desc">{p.desc}</span>
              <span className="preset-tools">{Object.keys(p.tools).slice(0, 4).join(" · ")}</span>
            </button>
          ))}
        </div>
      </div>

      {userPresets.length > 0 && (
        <div className="preset-section">
          <h2 className="section-title">我的预设</h2>
          <div className="preset-grid">
            {userPresets.map((up) => (
              <div className="preset-card-wrap" key={up.name}>
                <button className="preset-card" onClick={() => loadUserPreset(up.name)}>
                  <span className="preset-name">{up.name}</span>
                  <span className="preset-tools">点击载入 · 可再一键安装</span>
                </button>
                <button
                  className="preset-del"
                  title="删除该预设"
                  onClick={() => deleteUserPreset(up.name)}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      <section className="panel" style={{ marginBottom: 16 }}>
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
          <h2 className="panel-title" style={{ margin: 0 }}>
            发现的项目
          </h2>
          <span className="pill muted">{discovered ? discovered.length : "…"} 个</span>
          <span style={{ flex: 1 }} />
          <button className="btn" onClick={rescan} disabled={scanning}>
            {scanning ? "扫描中…" : "重新扫描"}
          </button>
        </div>
        {discovered && discovered.length === 0 && (
          <div className="empty small" style={{ marginTop: 8 }}>
            常用目录下没有发现项目（识别 .git / package.json / mise.toml 等指纹）。
          </div>
        )}
        <div className="setting-list" style={{ marginTop: 10 }}>
          {(discovered ?? []).slice(0, 50).map((p) => (
            <div key={p.path} className="setting-row" style={{ cursor: "default" }}>
              <span className="setting-info" style={{ flex: 1, minWidth: 0 }}>
                <span
                  className="setting-name"
                  style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8 }}
                >
                  {p.name}
                  {p.hasMiseToml ? (
                    <span className="pill active">已配置 mise</span>
                  ) : (
                    <span className="pill muted">未配置</span>
                  )}
                  {p.missingTools.length > 0 && (
                    <span
                      className="pill muted"
                      style={{ color: "var(--warning)", borderColor: "var(--warning)" }}
                    >
                      缺 {p.missingTools.join("、")}
                    </span>
                  )}
                </span>
                <span className="setting-desc" style={{ wordBreak: "break-all" }}>
                  {p.path}
                </span>
              </span>
              <button className="btn" onClick={() => loadProjectAt(p.path)}>
                载入
              </button>
            </div>
          ))}
        </div>
      </section>

      <div className="project-form">
        <div className="form-row">
          <input
            className="input grow"
            placeholder="输入项目目录路径，或指向某个 mise.toml 文件"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleLoad()}
          />
          <button className="btn" onClick={handlePickDir}>
            📁 选择文件夹
          </button>
          <button className="btn primary" onClick={handleLoad} disabled={loading}>
            {loading ? "读取中…" : "读取配置"}
          </button>
        </div>

        {loaded && (
          <>
            <div
              className="editor-cm"
              style={{
                border: "1px solid var(--border)",
                borderRadius: "var(--radius)",
                overflow: "hidden",
              }}
            >
              <Suspense fallback={<div className="empty small">编辑器加载中…</div>}>
                <TomlEditor value={content} dark={dark} onChange={setContent} />
              </Suspense>
            </div>
            <div className="form-actions">
              <button className="btn" onClick={openBindDialog} disabled={!loaded}>
                整机环境绑定
              </button>
              <button className="btn" onClick={handleLoad} disabled={loading}>
                重新读取
              </button>
              <button
                className="btn"
                onClick={saveUserPreset}
                title="把当前配置保存为可复用的用户预设"
              >
                保存为用户预设
              </button>
              <button className="btn" onClick={handleSave} disabled={saving}>
                {saving ? "保存中…" : "保存配置"}
              </button>
              <button className="btn primary" onClick={handleInstallAll} disabled={installing}>
                {installing ? "正在安装…" : "保存并一键安装"}
              </button>
            </div>
          </>
        )}

        {!loaded && !error && (
          <div className="empty small">
            输入一个项目的目录路径，即可查看并编辑它的运行时版本配置。
            <br />
            例如：/Users/you/my-project，或直接选择已有的 mise.toml 文件。
          </div>
        )}
      </div>

      <Dialog open={bindOpen} onOpenChange={(o) => setBindOpen(o)}>
        <DialogContent className="w-[460px] p-0">
          <div className="dialog-head">
            <DialogTitle>绑定整机环境（PATH 方式）</DialogTitle>
          </div>
          <div className="dialog-body">
            <div className="dialog-tip">
              不建立任何软链：把包管理器维护的稳定 bin 路径写入当前 mise.toml 的
              <code>[env] _.path</code>，项目激活时自动可用。适用于 brew / scoop 安装的运行时。
            </div>
            {bindSources.length === 0 && (
              <div className="empty small">未发现可通过稳定路径绑定的运行时</div>
            )}
            <div className="adopt-list">
              {bindSources.map((s, i) => (
                <div className="adopt-row" key={i}>
                  <span className="adopt-info">
                    {s.manager} · {s.tool} · {s.version}
                  </span>
                  <button
                    className="btn xs primary"
                    disabled={bindBusy !== null}
                    onClick={() => bindSource(s)}
                  >
                    {bindBusy === `${s.manager}::${s.tool}` ? "绑定中…" : "写入配置"}
                  </button>
                </div>
              ))}
            </div>
          </div>
          <div className="dialog-foot">
            <button className="btn-ghost" onClick={() => setBindOpen(false)}>
              关闭
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
