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
  type PresetFile,
} from "../api";
import { presetToToml, toolsOf } from "../lib/preset";
import type { Navigate } from "../lib/nav";
import { exportPresetFile } from "../lib/presetFileIO";
import { useUserPresets } from "../lib/useUserPresets";
import PresetLibrary, { type PresetChoice } from "./PresetLibrary";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";

// 编辑器整体懒加载：CodeMirror 及其 TOML 语言包只在首次进入项目配置页时拉取
const TomlEditor = lazy(() => import("./TomlEditor"));

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

interface Props {
  /** 切到别的 tab（用于把整机相关操作指路到「环境」页） */
  onNavigate?: Navigate;
}

export default function ProjectsView({ onNavigate }: Props) {
  const [path, setPath] = useState("");
  const [content, setContent] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
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

  /** 预设库：与「环境」页共用同一份 localStorage（两页互斥挂载，无同步问题） */
  const presets = useUserPresets();

  /** 把当前配置保存为用户预设 */
  function saveUserPreset() {
    const name = window.prompt("为这个自定义预设命名：", "我的预设");
    if (!name || !name.trim()) return;
    presets.save(name, content);
    setNotice(`已保存用户预设「${name.trim()}」`);
  }

  /** 导出预设为可分享文件 */
  async function handleExportPreset(choice: PresetChoice) {
    setError(null);
    setNotice(null);
    try {
      const msg = await exportPresetFile(choice.name, choice.description, choice.toml);
      if (msg) setNotice(msg);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  /** 导出编辑器里的当前配置（名字走与保存用户预设一致的 prompt 交互） */
  function exportCurrentConfig() {
    const name = window.prompt("为导出的预设命名：", "我的环境");
    if (!name || !name.trim()) return;
    void handleExportPreset({ name: name.trim(), description: "", toml: content });
  }

  /** 预设卡片 → 载入编辑器；装到哪个项目由下面的「保存并一键安装」决定 */
  function applyChoice(choice: PresetChoice) {
    setContent(choice.toml);
    setLoaded(true);
    setError(null);
    setNotice(`已载入「${choice.name}」，可编辑后保存，或直接一键安装到项目`);
  }

  /** 导入的预设文件 → 载入编辑器（收录进预设库已由 PresetLibrary 完成） */
  function applyImported(file: PresetFile) {
    setContent(presetToToml(toolsOf(file.tools)));
    setLoaded(true);
    setError(null);
    setNotice(`已导入预设「${file.name}」并载入编辑器，可编辑后保存或一键安装到项目`);
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

      {/* 预设库：项目级用法——载入编辑器，再由「保存并一键安装」落到具体项目。
          文案按行拆开写：换行处 JSX 会吃掉空白，才不会在中文标点前后留出空格 */}
      <div className="banner info scope-note">
        这里把预设装进
        <strong>某个项目</strong>
        ；要装到
        <strong>整机</strong>
        、改全局环境变量或做整机备份，用
        {onNavigate ? (
          <button className="link-btn" onClick={() => onNavigate("tools")}>
            环境
          </button>
        ) : (
          "「环境」"
        )}
        页。
      </div>

      <PresetLibrary
        presets={presets}
        useHint="点击载入 · 可再一键安装到项目"
        onUse={applyChoice}
        onExport={(choice) => void handleExportPreset(choice)}
        onImported={applyImported}
        onError={setError}
      />

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
    </div>
  );
}
