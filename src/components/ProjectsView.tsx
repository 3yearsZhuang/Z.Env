import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  readProjectConfig,
  writeProjectConfig,
  installAllProject,
  detectToolSources,
  stableBinPath,
  discoverProjects,
  errorMessage,
  listTools,
  installVersionStreaming,
  onInstallProgress,
  presetExport,
  presetImport,
  ToolSource,
  type DiscoveredProject,
  type PresetFile,
  type PresetTool,
} from "../api";
import {
  parseStoredPresets,
  presetFileName,
  presetToToml,
  toolsOf,
  type UserPreset,
} from "../lib/preset";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";

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
  const [userPresets, setUserPresets] = useState<UserPreset[]>([]);
  const dark = useDarkMode();

  // 预设导入：预览 + 选择安装去向（整机 / 项目）
  const [imported, setImported] = useState<PresetFile | null>(null);
  const [installedNames, setInstalledNames] = useState<Set<string>>(new Set());
  const [installTarget, setInstallTarget] = useState<"global" | "project">("project");
  const [installLog, setInstallLog] = useState<string[]>([]);
  const [installBusy, setInstallBusy] = useState(false);
  const logRef = useRef<HTMLPreElement | null>(null);

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
  // 启动时读取本地保存的用户预设（旧数据与损坏项由 parseStoredPresets 兜底）
  useEffect(() => {
    setUserPresets(parseStoredPresets(localStorage.getItem(PRESET_STORAGE_KEY)));
  }, []);

  function persistPresets(list: UserPreset[]) {
    setUserPresets(list);
    try {
      localStorage.setItem(PRESET_STORAGE_KEY, JSON.stringify(list));
    } catch {
      /* 存储满等忽略 */
    }
  }

  // 安装日志自动滚到底部
  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [installLog]);

  /** 把当前配置保存为用户预设 */
  function saveUserPreset() {
    const name = window.prompt("为这个自定义预设命名：", "我的预设");
    if (!name || !name.trim()) return;
    const trimmed = name.trim();
    const list = [...userPresets.filter((p) => p.name !== trimmed), { name: trimmed, content }];
    persistPresets(list);
    setNotice(`已保存用户预设「${trimmed}」`);
  }

  /** 导出预设为可分享文件（三个入口共用） */
  async function exportPreset(name: string, description: string, toml: string) {
    const target = await save({
      title: "导出环境预设",
      defaultPath: presetFileName(name),
      filters: [{ name: "Z.Env 环境预设", extensions: ["toml"] }],
    });
    if (!target) return;
    setError(null);
    setNotice(null);
    try {
      setNotice(await presetExport(target, name, description, toml));
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  /** 导出编辑器里的当前配置（名字走与保存用户预设一致的 prompt 交互） */
  function exportCurrentConfig() {
    const name = window.prompt("为导出的预设命名：", "我的环境");
    if (!name || !name.trim()) return;
    void exportPreset(name.trim(), "", content);
  }

  /** 选择预设文件并打开导入预览 */
  async function handleImportPreset() {
    const picked = await open({
      multiple: false,
      title: "选择环境预设文件",
      filters: [{ name: "Z.Env 环境预设", extensions: ["toml"] }],
    });
    if (!picked || typeof picked !== "string") return;
    setError(null);
    setNotice(null);
    try {
      const file = await presetImport(picked);
      // 已装状态仅供预览参考，取不到就不标注
      try {
        const tools = await listTools();
        setInstalledNames(new Set(tools.map((t) => t.name)));
      } catch {
        setInstalledNames(new Set());
      }
      setInstallLog([]);
      setInstallTarget("project");
      setImported(file);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  /** 把导入的预设写入本地预设库；同名时先确认覆盖，用户取消则返回 false */
  function rememberImported(file: PresetFile): boolean {
    if (
      userPresets.some((p) => p.name === file.name) &&
      !window.confirm(`已存在同名预设「${file.name}」，覆盖它吗？`)
    ) {
      return false;
    }
    const list = [
      ...userPresets.filter((p) => p.name !== file.name),
      {
        name: file.name,
        content: presetToToml(toolsOf(file.tools)),
        ...(file.description ? { description: file.description } : {}),
      },
    ];
    persistPresets(list);
    return true;
  }

  /** 载入导入的预设到编辑器 */
  function loadImported() {
    if (!imported || !rememberImported(imported)) return;
    setContent(presetToToml(toolsOf(imported.tools)));
    setLoaded(true);
    setError(null);
    setNotice(`已导入预设「${imported.name}」并载入编辑器，可编辑后保存或一键安装`);
    setImported(null);
  }

  /** 整机安装：逐个流式安装，单个失败不中断（返回成功数） */
  async function installToMachine(tools: PresetTool[]): Promise<number> {
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await onInstallProgress((p) => {
        setInstallLog((prev) => [...prev, `[${p.tool}] ${p.line}`]);
      });
    } catch {
      /* 订阅失败不阻断安装，只是没有实时输出 */
    }
    let ok = 0;
    try {
      for (const t of tools) {
        setInstallLog((prev) => [...prev, `▶ 正在安装 ${t.name}@${t.version} …`]);
        try {
          await installVersionStreaming(t.name, t.version);
          ok += 1;
          setInstallLog((prev) => [...prev, `✓ ${t.name}@${t.version} 完成`]);
        } catch (e) {
          setInstallLog((prev) => [...prev, `✗ ${t.name}@${t.version} 失败：${errorMessage(e)}`]);
        }
      }
    } finally {
      unlisten?.();
    }
    return ok;
  }

  /** 项目安装：写入 mise.toml 后执行 mise install（复用现成命令） */
  async function installToProject(file: PresetFile, dir: string): Promise<string> {
    const toml = presetToToml(toolsOf(file.tools));
    const msg = await installAllProject(dir, toml);
    setContent(toml);
    setLoaded(true);
    return msg;
  }

  /** 一键安装导入的预设 */
  async function installImported() {
    if (!imported) return;
    const dir = path.trim();
    if (installTarget === "project" && !dir) {
      setError("请先选择要安装到的项目目录");
      return;
    }
    if (!rememberImported(imported)) return;

    setInstallBusy(true);
    setError(null);
    setNotice(null);
    setInstallLog([]);
    try {
      if (installTarget === "global") {
        const total = imported.tools.length;
        const ok = await installToMachine(imported.tools);
        setNotice(`整机安装完成：成功 ${ok}/${total}`);
        if (ok < total) {
          setInstallLog((prev) => [...prev, `⚠ ${total - ok} 个工具未装成功，详见上方日志`]);
        }
      } else {
        setNotice(await installToProject(imported, dir));
      }
      rescan();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setInstallBusy(false);
    }
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
        <div className="preset-head">
          <div>
            <h2 className="section-title">环境预设</h2>
            <p className="preset-hint">选择技术栈一键生成全套环境，也可导入他人分享的预设文件</p>
          </div>
          <button className="btn" onClick={handleImportPreset}>
            导入预设
          </button>
        </div>
        <div className="preset-grid">
          {PRESETS.map((p) => (
            <div className="preset-card-wrap" key={p.id}>
              <button className="preset-card" onClick={() => applyPreset(p.tools, p.name)}>
                <span className="preset-name">{p.name}</span>
                <span className="preset-desc">{p.desc}</span>
                <span className="preset-tools">{Object.keys(p.tools).slice(0, 4).join(" · ")}</span>
              </button>
              <div className="preset-acts">
                <button
                  className="preset-act"
                  title="导出为可分享的预设文件"
                  onClick={() => void exportPreset(p.name, p.desc, presetToToml(p.tools))}
                >
                  ⤓
                </button>
              </div>
            </div>
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
                  <span className="preset-tools">
                    {up.description ?? "点击载入 · 可再一键安装"}
                  </span>
                </button>
                <div className="preset-acts">
                  <button
                    className="preset-act"
                    title="导出为可分享的预设文件"
                    onClick={() => void exportPreset(up.name, up.description ?? "", up.content)}
                  >
                    ⤓
                  </button>
                  <button
                    className="preset-act danger"
                    title="删除该预设"
                    onClick={() => deleteUserPreset(up.name)}
                  >
                    ✕
                  </button>
                </div>
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
              <button className="btn" onClick={exportCurrentConfig} title="导出为可分享的预设文件">
                导出为预设
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

      <Dialog
        open={imported !== null}
        onOpenChange={(o) => {
          if (!o && !installBusy) setImported(null);
        }}
      >
        <DialogContent className="w-[520px] p-0">
          <div className="dialog-head">
            <DialogTitle>导入环境预设</DialogTitle>
            <DialogDescription className="setting-desc">
              {imported ? imported.path : ""}
            </DialogDescription>
          </div>
          <div className="dialog-body">
            {imported && (
              <>
                <div className="dialog-tip">
                  <strong>{imported.name}</strong>
                  {imported.description ? ` · ${imported.description}` : ""}
                  <br />共 {imported.tools.length} 个工具；选择安装去向后点「一键安装」。
                  已装状态按本机 mise 现状标注，仅作参考。
                </div>
                <div className="adopt-list">
                  {imported.tools.map((t) => (
                    <div className="adopt-row" key={t.name}>
                      <span className="adopt-info">
                        {t.name} @ {t.version}
                      </span>
                      {installedNames.has(t.name) ? (
                        <span className="pill active">已装</span>
                      ) : (
                        <span className="pill muted">未装</span>
                      )}
                    </div>
                  ))}
                </div>

                <div className="preset-target">
                  <label className="preset-radio">
                    <input
                      type="radio"
                      name="preset-target"
                      checked={installTarget === "global"}
                      onChange={() => setInstallTarget("global")}
                    />
                    安装到整机（不依赖项目，逐个装进 mise）
                  </label>
                  <label className="preset-radio">
                    <input
                      type="radio"
                      name="preset-target"
                      checked={installTarget === "project"}
                      onChange={() => setInstallTarget("project")}
                    />
                    安装到项目
                  </label>
                  {installTarget === "project" && (
                    <div className="form-row" style={{ marginTop: 8 }}>
                      <input
                        className="input grow"
                        placeholder="项目目录路径，将写入该目录的 mise.toml"
                        value={path}
                        onChange={(e) => setPath(e.target.value)}
                      />
                      <button className="btn" onClick={handlePickDir}>
                        📁 选择文件夹
                      </button>
                    </div>
                  )}
                </div>

                {installLog.length > 0 && (
                  <pre className="terminal" ref={logRef}>
                    {installLog.join("\n")}
                  </pre>
                )}
              </>
            )}
          </div>
          <div className="dialog-foot">
            <button className="btn-ghost" onClick={() => setImported(null)} disabled={installBusy}>
              关闭
            </button>
            <span style={{ flex: 1 }} />
            <button className="btn" onClick={loadImported} disabled={installBusy}>
              载入到编辑器
            </button>
            <button className="btn primary" onClick={installImported} disabled={installBusy}>
              {installBusy ? "安装中…" : "一键安装"}
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
