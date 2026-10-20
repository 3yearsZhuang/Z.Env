import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowDownToLine,
  ArrowRight,
  Check,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  FileCode2,
  FolderKanban,
  FolderOpen,
  Layers,
  LoaderCircle,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import {
  bindProjectPath,
  detectToolSources,
  stableBinPath,
  discoverProjects,
  errorMessage,
  type ToolSource,
  type DiscoveredProject,
  type PresetFile,
} from "../api";
import { presetToToml, toolsOf } from "../lib/preset";
import type { Navigate } from "../lib/nav";
import { exportPresetFile } from "../lib/presetFileIO";
import { useProjectEditor } from "../lib/useProjectEditor";
import { useUserPresets } from "../lib/useUserPresets";
import PresetLibrary, { type PresetChoice } from "./PresetLibrary";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";
import "./ProjectsView.css";

// CodeMirror 与 TOML 语言包只在首次载入项目后拉取。
const TomlEditor = lazy(() => import("./TomlEditor"));
const ProjectEnvironmentPanel = lazy(() => import("./ProjectEnvironmentPanel"));
const PAGE_SIZE = 6;
type Operation = "loading" | "picking" | "saving" | "installing" | "binding" | "exporting";

function useDarkMode(): boolean {
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));
  useEffect(() => {
    const observer = new MutationObserver(() =>
      setDark(document.documentElement.classList.contains("dark")),
    );
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);
  return dark;
}

interface Props {
  onNavigate?: Navigate;
  active?: boolean;
}

export default function ProjectsView({ onNavigate, active = true }: Props) {
  const editor = useProjectEditor();
  const { path, setPath, content, setContent, target, original } = editor;
  const presets = useUserPresets();
  const dark = useDarkMode();
  const [loaded, setLoaded] = useState(false);
  const [operation, setOperation] = useState<Operation | null>(null);
  const operationRef = useRef(false);
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [environmentRefresh, setEnvironmentRefresh] = useState(0);
  const [environmentChanged, setEnvironmentChanged] = useState(false);
  const editorSection = useRef<HTMLElement>(null);
  const [editorReveal, setEditorReveal] = useState(0);
  useEffect(() => {
    if (editorReveal === 0) return;
    editorSection.current?.focus({ preventScroll: true });
    editorSection.current?.scrollIntoView({ block: "start" });
  }, [editorReveal]);
  const busy = operation !== null;
  const dirty = loaded && (original === null || content !== original);

  const [discovered, setDiscovered] = useState<DiscoveredProject[] | null>(null);
  const [scanning, setScanning] = useState(true);
  const scanningRef = useRef(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const filtered = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    return (discovered ?? []).filter((project) =>
      `${project.name} ${project.path}`.toLocaleLowerCase().includes(query),
    );
  }, [discovered, search]);
  const pageCount = Math.ceil(filtered.length / PAGE_SIZE);
  const currentPage = Math.min(page, Math.max(0, pageCount - 1));
  const visibleProjects = filtered.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE);

  const [bindOpen, setBindOpen] = useState(false);
  const [bindSources, setBindSources] = useState<ToolSource[]>([]);
  const [bindLoading, setBindLoading] = useState(false);
  const bindLoadingRef = useRef(false);
  const [bindError, setBindError] = useState<string | null>(null);
  const [bindBusy, setBindBusy] = useState<string | null>(null);

  // ref 在下一次渲染前也能阻止重复提交；所有项目读写仍由 useProjectEditor 管理。
  async function runOperation(kind: Operation, action: () => Promise<void>) {
    if (operationRef.current) return;
    operationRef.current = true;
    setOperation(kind);
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      operationRef.current = false;
      setOperation(null);
      setLoadingPath(null);
    }
  }

  async function rescan() {
    if (scanningRef.current) return;
    scanningRef.current = true;
    setScanning(true);
    setScanError(null);
    try {
      setDiscovered(await discoverProjects());
      setPage(0);
    } catch (e) {
      setScanError(errorMessage(e));
    } finally {
      scanningRef.current = false;
      setScanning(false);
    }
  }
  useEffect(() => {
    void rescan();
  }, []);

  async function readProjectAt(projectPath: string) {
    // 先解除旧保存目标：读取新目录失败时，保留草稿但不能误写回旧项目。
    setPath(projectPath);
    setLoadingPath(projectPath);
    setOperation("loading");
    try {
      if (!(await editor.load(projectPath))) return;
      setLoaded(true);
      setEnvironmentChanged(false);
      setEditorReveal((value) => value + 1);
      setNotice("已读取项目配置，可编辑后保存，或从预设库选择技术栈。");
    } catch (e) {
      throw new Error(`读取项目配置失败：${errorMessage(e)}`);
    }
  }
  function loadProjectAt(projectPath: string) {
    return runOperation("loading", () => readProjectAt(projectPath));
  }
  function handleLoad() {
    if (operationRef.current) return;
    if (!path.trim()) {
      setError("请输入项目目录或 mise.toml 文件路径");
      return;
    }
    void loadProjectAt(path.trim());
  }
  function handlePickDir() {
    void runOperation("picking", async () => {
      let selected;
      try {
        selected = await open({ directory: true, multiple: false, title: "选择项目目录" });
      } catch (e) {
        throw new Error(`选择项目目录失败：${errorMessage(e)}`);
      }
      if (typeof selected === "string" && selected) await readProjectAt(selected);
    });
  }
  function handleSave(install = false) {
    if (!target || environmentChanged || operationRef.current) return;
    void runOperation(install ? "installing" : "saving", async () => {
      try {
        setNotice(await editor.save(install));
      } finally {
        setEnvironmentRefresh((value) => value + 1);
      }
    });
  }

  async function loadBindSources() {
    if (bindLoadingRef.current) return;
    bindLoadingRef.current = true;
    setBindLoading(true);
    setBindError(null);
    try {
      const sources = await detectToolSources();
      setBindSources(sources.filter((source) => ["brew", "scoop"].includes(source.manager)));
    } catch (e) {
      setBindError(errorMessage(e));
    } finally {
      bindLoadingRef.current = false;
      setBindLoading(false);
    }
  }
  function openBindDialog() {
    if (operationRef.current) return;
    setBindOpen(true);
    void loadBindSources();
  }
  function bindSource(source: ToolSource) {
    void runOperation("binding", async () => {
      setBindBusy(`${source.manager}::${source.tool}`);
      try {
        await editor.transform(async (current) => {
          const bin = await stableBinPath(source.manager, source.tool, source.path);
          return bindProjectPath(current, bin);
        });
        setNotice(`已将 ${source.tool}（${source.manager}）绑定进当前配置，保存后生效。`);
        setBindOpen(false);
      } catch (e) {
        setBindError(errorMessage(e));
      } finally {
        setBindBusy(null);
      }
    });
  }
  function saveUserPreset() {
    if (operationRef.current) return;
    const name = window.prompt("为这个自定义预设命名：", "我的预设");
    if (!name?.trim()) return;
    presets.save(name, content);
    setNotice(`已保存用户预设「${name.trim()}」`);
  }
  function handleExportPreset(choice: PresetChoice) {
    return runOperation("exporting", async () => {
      const message = await exportPresetFile(choice.name, choice.description, choice.toml);
      if (message) setNotice(message);
    });
  }
  function exportCurrentConfig() {
    if (operationRef.current) return;
    const name = window.prompt("为导出的预设命名：", "我的环境");
    if (name?.trim())
      void handleExportPreset({ name: name.trim(), description: "", toml: content });
  }
  function applyChoice(choice: PresetChoice) {
    if (operationRef.current || !target) return;
    setContent(choice.toml);
    setLoaded(true);
    setError(null);
    setNotice(`已载入「${choice.name}」，检查配置后保存，或一键安装到当前项目。`);
    setEditorReveal((value) => value + 1);
  }
  function applyImported(file: PresetFile) {
    if (operationRef.current || !target) {
      setNotice(`已导入预设「${file.name}」。读取项目后，可从「我的预设」载入。`);
      return;
    }
    applyChoice({
      name: file.name,
      description: file.description ?? "",
      toml: presetToToml(toolsOf(file.tools)),
    });
  }

  return (
    <div className="view project-view">
      <div className="view-head project-view-head">
        <div>
          <h1>项目配置</h1>
          <p className="view-sub">选择项目，管理运行时版本，让每个工作区拥有独立的开发环境。</p>
        </div>
        <span className="project-format">
          <FileCode2 size={15} aria-hidden="true" /> mise.toml
        </span>
      </div>
      {error && (
        <div className="project-feedback project-feedback-error" role="alert">
          <CircleAlert size={17} aria-hidden="true" />
          <span>{error}</span>
          <button
            className="project-icon-button"
            aria-label="关闭错误提示"
            onClick={() => setError(null)}
          >
            <X size={16} />
          </button>
        </div>
      )}
      {notice && (
        <div className="project-feedback project-feedback-success" role="status">
          <Check size={17} aria-hidden="true" />
          <span>{notice}</span>
          <button
            className="project-icon-button"
            aria-label="关闭操作提示"
            onClick={() => setNotice(null)}
          >
            <X size={16} />
          </button>
        </div>
      )}

      <section className="panel project-panel" aria-labelledby="project-discovery-title">
        <div className="project-section-head">
          <div className="project-section-heading">
            <span className="project-section-icon">
              <FolderKanban size={19} aria-hidden="true" />
            </span>
            <div>
              <h2 id="project-discovery-title">发现的项目</h2>
              <p>扫描常用开发目录，选择一个项目开始配置。</p>
            </div>
            {discovered !== null && <span className="project-count">{discovered.length}</span>}
          </div>
          <button className="btn" onClick={() => void rescan()} disabled={scanning}>
            <RefreshCw
              size={14}
              className={scanning ? "project-spinning" : undefined}
              aria-hidden="true"
            />
            {scanning ? "扫描中…" : "重新扫描"}
          </button>
        </div>
        {scanError && (
          <div className="project-scan-error" role="alert">
            <CircleAlert size={16} aria-hidden="true" />
            <span>
              扫描失败：{scanError}
              {discovered !== null && "。下方保留上次扫描结果。"} 可重试，或手动选择项目目录。
            </span>
          </div>
        )}
        {discovered !== null && discovered.length > 0 && (
          <div className="project-search">
            <Search size={16} aria-hidden="true" />
            <input
              type="search"
              aria-label="搜索项目名称或路径"
              placeholder="搜索项目名称或路径…"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
                setPage(0);
              }}
            />
          </div>
        )}
        {scanning && discovered === null ? (
          <div className="project-empty" role="status">
            <LoaderCircle size={25} className="project-spinning" aria-hidden="true" />
            <strong>正在发现项目</strong>
            <p>查找常用目录中的 Git、Node.js 和 mise 项目。</p>
          </div>
        ) : discovered?.length === 0 ? (
          <div className="project-empty">
            <FolderOpen size={28} aria-hidden="true" />
            <strong>还没有发现项目</strong>
            <p>可以在下方手动输入路径，或选择项目所在的文件夹。</p>
            <button className="btn" onClick={handlePickDir} disabled={busy}>
              选择项目目录
              <ArrowRight size={14} aria-hidden="true" />
            </button>
          </div>
        ) : discovered !== null && filtered.length === 0 ? (
          <div className="project-empty">
            <Search size={25} aria-hidden="true" />
            <strong>没有匹配的项目</strong>
            <p>试试其他名称或路径关键词。</p>
            <button
              className="btn"
              onClick={() => {
                setSearch("");
                setPage(0);
              }}
            >
              清除搜索
            </button>
          </div>
        ) : (
          <ul className="project-list" aria-label="发现的项目列表" aria-busy={scanning}>
            {visibleProjects.map((project) => {
              const active =
                target === project.path ||
                target === `${project.path.replace(/[\\/]$/, "")}/mise.toml`;
              return (
                <li
                  key={project.path}
                  className={`project-row${active ? " project-row-active" : ""}`}
                >
                  <span className="project-folder-icon">
                    <FolderOpen size={19} aria-hidden="true" />
                  </span>
                  <div className="project-row-info">
                    <div className="project-row-title">
                      <strong>{project.name}</strong>
                      <span
                        className={`project-badge${project.hasMiseToml ? " project-badge-configured" : ""}`}
                      >
                        {project.hasMiseToml ? "有 mise 配置" : "尚无 mise 配置"}
                      </span>
                      {active && <span className="project-current">当前项目</span>}
                    </div>
                    <p className="project-path" title={project.path}>
                      {project.path}
                    </p>
                    {project.tools.length > 0 && (
                      <p className="project-missing">
                        推断工具：{project.tools.join("、")} · 版本与生效待核对
                      </p>
                    )}
                  </div>
                  <button
                    className="btn project-load-button"
                    aria-label={`载入 ${project.name}`}
                    onClick={() => void loadProjectAt(project.path)}
                    disabled={busy}
                  >
                    {loadingPath === project.path ? "载入中…" : active ? "重新载入" : "载入"}
                    <ChevronRight size={14} aria-hidden="true" />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        {filtered.length > 0 && (
          <div className="project-list-footer">
            <span aria-live="polite">
              显示 {currentPage * PAGE_SIZE + 1}–
              {Math.min((currentPage + 1) * PAGE_SIZE, filtered.length)} / {filtered.length} 个项目
              {search.trim() && ` · 共 ${discovered?.length} 个`}
            </span>
            {pageCount > 1 && (
              <nav className="project-pagination" aria-label="项目列表分页">
                <button
                  className="project-icon-button"
                  aria-label="上一页项目"
                  disabled={currentPage === 0}
                  onClick={() => setPage(currentPage - 1)}
                >
                  <ChevronLeft size={16} />
                </button>
                <span>
                  {currentPage + 1} / {pageCount}
                </span>
                <button
                  className="project-icon-button"
                  aria-label="下一页项目"
                  disabled={currentPage === pageCount - 1}
                  onClick={() => setPage(currentPage + 1)}
                >
                  <ChevronRight size={16} />
                </button>
              </nav>
            )}
          </div>
        )}
      </section>

      <section
        className="panel project-panel project-editor-section"
        aria-labelledby="project-editor-title"
        tabIndex={-1}
        ref={editorSection}
      >
        <div className="project-section-head">
          <div className="project-section-heading">
            <span className="project-section-icon">
              <FileCode2 size={19} aria-hidden="true" />
            </span>
            <div>
              <h2 id="project-editor-title">项目工作区</h2>
              <p>读取配置后编辑，保存到当前项目。</p>
            </div>
          </div>
          <span className={`project-badge${target ? " project-badge-configured" : ""}`}>
            {busy
              ? "操作进行中"
              : !target
                ? "待读取项目"
                : environmentChanged
                  ? "项目待重新读取"
                  : dirty
                    ? "有未保存修改"
                    : "已载入"}
          </span>
        </div>
        <form
          className="project-path-form"
          onSubmit={(event) => {
            event.preventDefault();
            handleLoad();
          }}
        >
          <label htmlFor="project-path-input">项目目录或配置文件</label>
          <div className="project-path-controls">
            <input
              id="project-path-input"
              className="input"
              placeholder="例如 /Users/you/my-project 或 mise.toml 路径"
              value={path}
              disabled={busy}
              onChange={(event) => {
                setPath(event.target.value);
                setNotice(null);
              }}
              aria-describedby="project-target-hint"
              spellCheck={false}
              autoComplete="off"
            />
            <button type="button" className="btn" onClick={handlePickDir} disabled={busy}>
              <FolderOpen size={15} aria-hidden="true" />
              {operation === "picking" ? "选择中…" : "选择文件夹"}
            </button>
            <button type="submit" className="btn primary" disabled={busy || !path.trim()}>
              {operation === "loading" ? (
                <LoaderCircle size={15} className="project-spinning" aria-hidden="true" />
              ) : (
                <ArrowDownToLine size={15} aria-hidden="true" />
              )}
              {operation === "loading" ? "读取中…" : "读取配置"}
            </button>
          </div>
          <p className="project-target-hint" id="project-target-hint">
            {target ? (
              <>
                保存目标：<code>{target}</code>
              </>
            ) : loaded ? (
              "路径尚未载入。下方保留当前草稿，读取成功后才能保存到项目。"
            ) : (
              "已有配置会载入编辑器；没有 mise.toml 的项目可在读取后创建配置。"
            )}
          </p>
        </form>
        {loaded ? (
          <>
            <div className="project-editor-heading">
              <span>
                <FileCode2 size={14} aria-hidden="true" /> mise.toml
              </span>
              <span>
                {!target
                  ? "草稿 · 待读取项目"
                  : original === null
                    ? "新建配置"
                    : dirty
                      ? "未保存"
                      : environmentChanged
                        ? "项目可能已变化，请重新读取"
                        : "与载入内容一致"}
              </span>
            </div>
            <div
              className="project-editor"
              role="group"
              aria-label="mise.toml 配置编辑器"
              inert={busy}
            >
              <Suspense
                fallback={
                  <div className="project-empty" role="status">
                    编辑器加载中…
                  </div>
                }
              >
                <TomlEditor
                  value={content}
                  dark={dark}
                  onChange={(value) => {
                    if (!operationRef.current) setContent(value);
                  }}
                />
              </Suspense>
            </div>
            <div className="project-editor-actions">
              <div className="project-secondary-actions">
                <button className="btn" onClick={openBindDialog} disabled={busy}>
                  整机环境绑定
                </button>
                <button className="btn" onClick={handleLoad} disabled={busy || !path.trim()}>
                  重新读取
                </button>
                <button className="btn" onClick={saveUserPreset} disabled={busy}>
                  保存为用户预设
                </button>
                <button className="btn" onClick={exportCurrentConfig} disabled={busy}>
                  导出为预设
                </button>
              </div>
              <div className="project-primary-actions">
                <button
                  className="btn"
                  onClick={() => handleSave()}
                  disabled={busy || !target || environmentChanged}
                >
                  {operation === "saving" ? "保存中…" : "保存配置"}
                </button>
                <button
                  className="btn primary"
                  onClick={() => handleSave(true)}
                  disabled={busy || !target || environmentChanged}
                >
                  {operation === "installing" ? "正在安装…" : "保存并一键安装"}
                </button>
              </div>
            </div>
            {target && (
              <Suspense fallback={<p role="status">正在加载项目环境核对…</p>}>
                <ProjectEnvironmentPanel
                  path={target}
                  active={active}
                  dirty={dirty || environmentChanged}
                  busy={busy}
                  refreshKey={environmentRefresh}
                  onChanged={() => {
                    setEnvironmentChanged(true);
                    setNotice("项目环境操作已结束；配置可能变化，请重新读取编辑器后继续保存。");
                  }}
                />
              </Suspense>
            )}
          </>
        ) : (
          <div className="project-empty project-editor-empty">
            <FileCode2 size={28} aria-hidden="true" />
            <strong>从一个项目开始</strong>
            <p>
              载入上方发现的项目，或选择文件夹，
              <br />
              在这里查看和编辑它的运行时版本配置。
            </p>
          </div>
        )}
      </section>

      <details className="project-presets">
        <summary>
          <span className="project-section-icon">
            <Layers size={19} aria-hidden="true" />
          </span>
          <span>
            <strong>从预设配置项目</strong>
            <span>读取项目后，应用技术栈预设，或导入分享的配置。</span>
          </span>
          <ChevronRight className="project-details-chevron" size={17} aria-hidden="true" />
        </summary>
        <div className="project-scope-note">
          预设将应用到当前项目。
          {onNavigate ? (
            <button className="link-btn" onClick={() => onNavigate("tools")}>
              前往环境页管理整机环境 <ArrowRight size={13} aria-hidden="true" />
            </button>
          ) : (
            "整机安装和全局环境变量请在「环境」页管理。"
          )}
        </div>
        <fieldset className="project-preset-content" disabled={busy}>
          <legend className="sr-only">项目环境预设</legend>
          <PresetLibrary
            presets={presets}
            useHint="载入到当前项目"
            useDisabled={busy || !target}
            useDisabledHint={busy ? "请等待当前操作完成" : "请先读取项目，再应用预设"}
            onUse={applyChoice}
            onExport={(choice) => void handleExportPreset(choice)}
            onImported={applyImported}
            onError={setError}
          />
        </fieldset>
      </details>

      <Dialog
        open={bindOpen}
        onOpenChange={(open) => {
          if (operation !== "binding") setBindOpen(open);
        }}
      >
        <DialogContent className="w-[460px] p-0" showClose={operation !== "binding"}>
          <div className="dialog-head">
            <DialogTitle>绑定整机环境</DialogTitle>
          </div>
          <div className="dialog-body">
            <DialogDescription className="project-bind-description">
              将 brew / scoop 维护的稳定 bin 路径写入 <code>[env] _.path</code>
              。保存只记录选择，实际解析需在项目环境核对中验证。
            </DialogDescription>
            {bindError && (
              <div className="project-scan-error" role="alert">
                <CircleAlert size={16} aria-hidden="true" />
                <span>{bindError}</span>
                <button
                  className="btn"
                  onClick={() => void loadBindSources()}
                  disabled={busy || bindLoading}
                >
                  重新检测
                </button>
              </div>
            )}
            {bindLoading ? (
              <div className="project-empty" role="status">
                <LoaderCircle size={23} className="project-spinning" aria-hidden="true" />
                正在检测可绑定的运行时…
              </div>
            ) : !bindError && bindSources.length === 0 ? (
              <div className="project-empty">未发现可通过稳定路径绑定的运行时。</div>
            ) : (
              !bindError && (
                <div className="adopt-list">
                  {bindSources.map((source) => (
                    <div
                      className="adopt-row"
                      key={`${source.manager}:${source.tool}:${source.path}`}
                    >
                      <span className="adopt-info">
                        {source.manager} · {source.tool} · {source.version}
                      </span>
                      <button
                        className="btn xs primary"
                        disabled={busy}
                        onClick={() => bindSource(source)}
                      >
                        {bindBusy === `${source.manager}::${source.tool}` ? "绑定中…" : "写入配置"}
                      </button>
                    </div>
                  ))}
                </div>
              )
            )}
          </div>
          <div className="dialog-foot">
            <button
              className="btn-ghost"
              onClick={() => setBindOpen(false)}
              disabled={operation === "binding"}
            >
              关闭
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
