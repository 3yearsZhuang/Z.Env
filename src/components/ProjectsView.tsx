import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  readProjectConfig,
  writeProjectConfig,
  installAllProject,
  errorMessage,
} from "../api";

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
    setLoading(true);
    setError(null);
    setNotice(null);
    try {
      const c = await readProjectConfig(path.trim());
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
            <button
              key={p.id}
              className="preset-card"
              onClick={() => applyPreset(p.tools, p.name)}
            >
              <span className="preset-name">{p.name}</span>
              <span className="preset-desc">{p.desc}</span>
              <span className="preset-tools">
                {Object.keys(p.tools)
                  .slice(0, 4)
                  .join(" · ")}
              </span>
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
                <button
                  className="preset-card"
                  onClick={() => loadUserPreset(up.name)}
                >
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
            <textarea
              className="editor"
              value={content}
              onChange={(e) => setContent(e.target.value)}
              spellCheck={false}
            />
            <div className="form-actions">
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
              <button
                className="btn"
                onClick={handleSave}
                disabled={saving}
              >
                {saving ? "保存中…" : "保存配置"}
              </button>
              <button
                className="btn primary"
                onClick={handleInstallAll}
                disabled={installing}
              >
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
    </div>
  );
}