import { useEffect, useMemo, useRef, useState } from "react";
import {
  listRemoteVersions,
  listRemoteVersionsAsdf,
  listRemoteVersionsGithub,
  listRemoteVersionsOfficial,
  installVersionStreaming,
  managedAdopt,
  managedUnadopt,
  onInstallProgress,
  errorMessage,
  ToolInfo,
  ToolSource,
} from "../api";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";

interface Props {
  tool: ToolInfo;
  /** 该工具被其他工具（nvm/pyenv 等）托管的版本 */
  external?: ToolSource[];
  onClose: () => void;
  onDone: () => void;
}

type RemoteSource = "mise" | "asdf" | "github" | "official";

const SOURCE_LABEL: Record<RemoteSource, string> = {
  mise: "mise 官方",
  asdf: "ASDF 插件",
  github: "GitHub Releases",
  official: "官方生态源",
};

// 各源读取函数
const SOURCE_FETCH: Record<RemoteSource, (tool: string) => Promise<string[]>> = {
  mise: listRemoteVersions,
  asdf: listRemoteVersionsAsdf,
  github: listRemoteVersionsGithub,
  official: listRemoteVersionsOfficial,
};
// 自动回退顺序：用户所选源优先，其后按此补序
const SOURCE_ORDER: RemoteSource[] = ["mise", "official", "github", "asdf"];

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
  const [usedSource, setUsedSource] = useState<RemoteSource>("mise");
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
      // 用户所选源优先，其后按固定顺序自动回退，取首个非空结果
      const ordered = [source, ...SOURCE_ORDER.filter((s) => s !== source)];
      let found: string[] = [];
      let used: RemoteSource | null = null;
      for (const s of ordered) {
        try {
          const v = await SOURCE_FETCH[s](tool.name);
          if (v && v.length > 0) {
            found = v;
            used = s;
            break;
          }
        } catch {
          // 该源不可用，继续尝试下一源
        }
      }
      if (!alive) return;
      setUsedSource(used ?? source);
      setRemote(found);
      const installed = new Set(tool.versions.map((v) => v.version));
      const candidate = found.find((v) => !installed.has(v));
      if (candidate) setSelected(candidate);
      setLoadingRemote(false);
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

  /** 接管：把已托管的外部环境链接为 mise 版本（brew 走托管模式），不重新下载 */
  async function handleAdopt(
    version: string,
    path: string,
    key: string,
    manager: string
  ) {
    setBusy(true);
    setAdopting(`${tool.name}@${version}`);
    setError(null);
    setToast(null);
    try {
      await managedAdopt(tool.name, version, manager, path);
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
      await managedUnadopt(tool.name, version);
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
  function requestAdopt(
    version: string,
    path: string,
    key: string,
    isDone: boolean,
    manager: string
  ) {
    setConfirm(
      isDone
        ? {
            msg: `确认解除接管 ${tool.name}@${version} 吗？`,
            onOk: () => handleUnadopt(version, key),
          }
        : {
            msg: `确认将 ${tool.name}@${version} 接管到 mise 管理吗？`,
            onOk: () => handleAdopt(version, path, key, manager),
          }
    );
  }

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="w-[420px] p-0" showClose={!busy}>
        <div className="dialog-head">
          <DialogTitle>安装 {tool.name}</DialogTitle>
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
              {!loadingRemote && remote.length > 0 && (
                <div className="dialog-tip">
                  当前版本列表来自：{SOURCE_LABEL[usedSource]}
                  {usedSource !== source ? "（自动回退）" : ""}
                </div>
              )}
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
                    nvm / pyenv / asdf 等用户级目录直连接管；brew 安装的版本走
                    <strong>托管模式</strong>（应用维护软链并在 brew 升级后自动重连）。
                    仅<strong>系统</strong>组件（/usr/bin、Xcode CLT 等）无法接管。
                  </div>
                  <div className="adopt-list">
                    {external.map((s, i) => {
                      // 系统组件（/usr/bin、Xcode CLT 等）没有独立版本目录，无法接管；
                      // brew 走托管模式（应用维护软链 + 对账自愈），其余直连
                      const adoptable = s.manager !== "system";
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
                                requestAdopt(s.version, s.path, key, done, s.manager)
                              }
                              title={
                                done
                                  ? "已接入 mise · 点击解除接管"
                                  : s.manager === "brew"
                                    ? "托管模式接管到 mise（brew 升级后自动重连）"
                                    : "点击接管到 mise"
                              }
                            >
                              {done ? "已接管 · 点击解除" : "接管"}
                            </button>
                          ) : (
                            <span className="pill muted small">
                              系统组件 · 不可接管
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
      </DialogContent>
    </Dialog>
  );
}