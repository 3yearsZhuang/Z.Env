import { useEffect, useMemo, useRef, useState } from "react";
import {
  listRemoteVersions,
  listRemoteVersionsAsdf,
  listRemoteVersionsGithub,
  listRemoteVersionsOfficial,
  installVersionStreaming,
  linkVersion,
  unlinkVersion,
  onInstallProgress,
  errorMessage,
  ToolInfo,
  ToolSource,
} from "../api";

interface Props {
  tool: ToolInfo;
  /** 该工具被其他工具（nvm/pyenv 等）托管的版本 */
  external?: ToolSource[];
  onClose: () => void;
  onDone: () => void;
}

type RemoteSource = "mise" | "asdf" | "github" | "official";

export default function InstallDialog({ tool, external, onClose, onDone }: Props) {
  // 已接管标记持久化到 localStorage，跨会话保留
  const ADOPT_KEY = "zenv:adopted";
  const loadAdopted = (): string[] => {
    try {
      const v = JSON.parse(localStorage.getItem(ADOPT_KEY) || "[]");
      return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
    } catch {
      return [];
    }
  };

  const [remote, setRemote] = useState<string[]>([]);
  const [source, setSource] = useState<RemoteSource>("mise");
  const [loadingRemote, setLoadingRemote] = useState(true);
  const [selected, setSelected] = useState("");
  const [manual, setManual] = useState("");
  const [busy, setBusy] = useState(false);
  const [adopting, setAdopting] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<string>("");
  const [finished, setFinished] = useState(false);
  const [adopted, setAdopted] = useState<string[]>(loadAdopted);
  const [toast, setToast] = useState<string | null>(null);
  const [showCli, setShowCli] = useState(false);
  const [confirm, setConfirm] = useState<{
    msg: string;
    onOk: () => void;
  } | null>(null);
  const progressRef = useRef<HTMLPreElement>(null);

  const adoptedKey = (m: string, v: string) => `${tool.name}::${m}::${v}`;

  const saveAdopted = (next: string[]) => {
    localStorage.setItem(ADOPT_KEY, JSON.stringify(next));
    setAdopted(next);
  };

  // 接管失败提示：短暂显示后自动消失
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 3500);
    return () => clearTimeout(t);
  }, [toast]);

  useEffect(() => {
    let alive = true;
    (async () => {
      setLoadingRemote(true);
      setError(null);
      try {
        const versions = await (source === "asdf"
          ? listRemoteVersionsAsdf(tool.name)
          : source === "github"
          ? listRemoteVersionsGithub(tool.name)
          : source === "official"
          ? listRemoteVersionsOfficial(tool.name)
          : listRemoteVersions(tool.name));
        if (!alive) return;
        setRemote(versions);
        const installed = new Set(tool.versions.map((v) => v.version));
        const candidate = versions.find((v) => !installed.has(v));
        if (candidate) setSelected(candidate);
      } catch {
        // 远程版本获取失败（如 C/C++ 等无对应源）：静默处理，不展示红色报错，
        // 让用户走“手动输入版本号”路径。
        if (alive) setRemote([]);
      } finally {
        if (alive) setLoadingRemote(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, [tool, source]);

  // 订阅安装进度事件；只在安装期捕获当前工具相关行
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    (async () => {
      unlisten = await onInstallProgress((p) => {
        if (!alive) return;
        if (p.tool === tool.name) {
          setProgress(p.line);
        }
      });
    })();
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [tool.name]);

  // 进度自动滚动到底部
  useEffect(() => {
    if (progressRef.current) {
      progressRef.current.scrollTop = progressRef.current.scrollHeight;
    }
  }, [progress]);

  const versionToInstall = manual.trim() || selected;

  const filteredRemote = useMemo(() => {
    const limit = 12;
    const installed = new Set(tool.versions.map((v) => v.version));
    const notInstalled = remote.filter((v) => !installed.has(v));
    return notInstalled.slice(0, limit);
  }, [remote, tool.versions]);

  async function handleInstall() {
    if (!versionToInstall) return;
    setBusy(true);
    setAdopting(null);
    setFinished(false);
    setProgress("");
    setError(null);
    try {
      // 流式安装：await 期间事件会持续更新 progress
      await installVersionStreaming(tool.name, versionToInstall);
      setFinished(true);
      setProgress("安装完成 ✓");
      setTimeout(() => {
        onDone();
        onClose();
      }, 500);
    } catch (e) {
      setError(errorMessage(e));
      setFinished(true);
    } finally {
      setBusy(false);
    }
  }

  /** 接管：把已托管的外部环境链接为 mise 版本，不重新下载 */
  async function handleAdopt(version: string, path: string, key: string) {
    setBusy(true);
    setAdopting(`${tool.name}@${version}`);
    setError(null);
    setToast(null);
    try {
      await linkVersion(tool.name, version, path);
      setAdopting(null);
      saveAdopted(adopted.includes(key) ? adopted : [...adopted, key]);
      onDone(); // 刷新父级工具列表
    } catch (e) {
      setAdopting(null);
      setToast(`无法接管 ${tool.name}@${version}：${errorMessage(e)}`);
    } finally {
      setBusy(false);
    }
  }

  /** 解除接管：移除某版本与外部目录的链接 */
  async function handleUnadopt(version: string, key: string) {
    setBusy(true);
    setAdopting(`${tool.name}@${version}`);
    setError(null);
    setToast(null);
    try {
      await unlinkVersion(tool.name, version);
      setAdopting(null);
      saveAdopted(adopted.filter((k) => k !== key));
      onDone();
    } catch (e) {
      setAdopting(null);
      setToast(`解除接管失败 ${tool.name}@${version}：${errorMessage(e)}`);
    } finally {
      setBusy(false);
    }
  }

  /** 接管（含解除）均需二次确认 */
  function requestAdopt(version: string, path: string, key: string, isDone: boolean) {
    setConfirm(
      isDone
        ? {
            msg: `确认解除接管 ${tool.name}@${version} 吗？`,
            onOk: () => handleUnadopt(version, key),
          }
        : {
            msg: `确认将 ${tool.name}@${version} 接管到 mise 管理吗？`,
            onOk: () => handleAdopt(version, path, key),
          }
    );
  }

  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-head">
          <h3>安装 {tool.name}</h3>
          <button className="btn-close" onClick={onClose}>
            ✕
          </button>
        </div>

        {error && (
          <div className="banner error" onClick={() => setError(null)}>
            ⚠ {error}
          </div>
        )}
        {toast && (
          <div className="toast error" onClick={() => setToast(null)}>
            ⚠ {toast}
          </div>
        )}

        <div className="dialog-body">
          {!busy && (
            <>
              <div className="source-row">
                <span className="field-label">远程源</span>
                <select
                  className="input"
                  value={source}
                  onChange={(e) => {
                    setSource(e.target.value as RemoteSource);
                    setSelected("");
                  }}
                >
                  <option value="mise">mise 官方</option>
                  <option value="asdf">ASDF 插件</option>
                  <option value="github">GitHub Releases</option>
                  <option value="official">官方生态源</option>
                </select>
              </div>
              {loadingRemote ? (
                <div className="empty small">正在获取远程版本列表…</div>
              ) : filteredRemote.length > 0 ? (
                <>
                  <label className="field-label">选择远程版本</label>
                  <select
                    className="input"
                    value={selected}
                    onChange={(e) => {
                      setSelected(e.target.value);
                      setManual("");
                    }}
                  >
                    {filteredRemote.map((v) => (
                      <option key={v} value={v}>
                        {v}
                      </option>
                    ))}
                  </select>
                </>
              ) : (
                <div className="empty small">无法自动获取远程版本，可手动输入版本号</div>
              )}

              {external && external.length > 0 && (
                <>
                  <label className="field-label">
                    接管已存在环境（无需重新下载）
                  </label>
                  <div className="dialog-tip">
                    接管仅适用于 nvm / pyenv / asdf / sdkman / rvm 等
                    <strong>用户级目录</strong>托管的版本。由<strong>系统</strong>
                    （Homebrew、/usr/bin、Xcode Command Line Tools 等）全局安装的
                    运行时无法通过接管纳管，请改用下方安装或由系统直接管理。
                  </div>
                  <div className="adopt-list">
                    {external.map((s, i) => {
                      // brew / system 等系统级来源无法被 mise link 接管
                      const adoptable = !["system", "brew"].includes(s.manager);
                      const key = adoptable ? adoptedKey(s.manager, s.version) : "";
                      const done = adoptable && adopted.includes(key);
                      return (
                        <div className="adopt-row" key={i}>
                          <span className="adopt-info">
                            {s.manager} · {s.version}
                          </span>
                          {adoptable ? (
                            <button
                              className={`btn xs ${done ? "" : "primary"}`}
                              disabled={busy}
                              onClick={() =>
                                requestAdopt(s.version, s.path, key, done)
                              }
                              title={
                                done
                                  ? "已接入 mise · 点击解除接管"
                                  : "点击接管到 mise"
                              }
                            >
                              {done ? "已接管 · 点击解除" : "接管"}
                            </button>
                          ) : (
                            <span className="pill muted small">
                              系统/Homebrew · 不可接管
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </>
              )}

              <label className="field-label">或手动输入版本号</label>
              <input
                className="input"
                placeholder={`例如 20.20.2 / latest / temurin-21.0.12`}
                value={manual}
                onChange={(e) => setManual(e.target.value)}
              />

              <div className="dialog-tip">
                支持输入任意 mise 识别的版本表达式，如 <code>latest</code>、{" "}
                <code>22.23.2</code>。
              </div>
            </>
          )}

          {busy && (
            <div className="install-area">
              <div className="install-title">
                {adopting
                  ? `正在接管 ${adopting}…`
                  : `正在安装 ${tool.name}@${versionToInstall}`}
                {finished && <span className="pill active small">完成</span>}
              </div>
              <div className="progress-track">
                <div className="progress-fill" />
              </div>
              <button
                type="button"
                className="cli-toggle"
                onClick={() => setShowCli((c) => !c)}
              >
                {showCli ? "收起 CLI 输出 ▴" : "展开 CLI 输出 ▾"}
              </button>
              {showCli && (
                <pre className="terminal" ref={progressRef}>
                  {adopting
                    ? "正在把外部环境链接为 mise 版本…"
                    : progress || "开始下载…"}
                </pre>
              )}
            </div>
          )}
        </div>

        <div className="dialog-foot">
          <button className="btn" onClick={onClose} disabled={busy}>
            {busy ? "安装中…" : "取消"}
          </button>
          {!busy && (
            <button
              className="btn primary"
              onClick={handleInstall}
              disabled={!versionToInstall}
            >
              安装 {versionToInstall}
            </button>
          )}
        </div>

        {confirm && (
          <div className="confirm-mask" onClick={() => setConfirm(null)}>
            <div className="confirm-box" onClick={(e) => e.stopPropagation()}>
              <p>{confirm.msg}</p>
              <div className="confirm-actions">
                <button
                  className="btn"
                  disabled={busy}
                  onClick={() => setConfirm(null)}
                >
                  取消
                </button>
                <button
                  className="btn primary"
                  disabled={busy}
                  onClick={() => {
                    const ok = confirm.onOk;
                    setConfirm(null);
                    ok();
                  }}
                >
                  确认
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}