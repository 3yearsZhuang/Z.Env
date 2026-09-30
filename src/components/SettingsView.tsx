import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  enable as enableAutostart,
  disable as disableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import {
  errorMessage,
  doctorRun,
  doctorFix,
  historyList,
  snapshotExport,
  snapshotRestore,
  DoctorCheck,
  HistoryEntry,
} from "../api";

const APP_NAME = "Z.Env";
const APP_TAGLINE = "整机环境管理中心";
const FALLBACK_VERSION = "0.7.0";

/** 操作历史 kind → 中文标签 */
const OP_KINDS: Record<string, string> = {
  install: "安装",
  "install-all": "项目安装",
  uninstall: "卸载",
  use: "切换",
  adopt: "接入",
  unadopt: "解除接入",
  reconcile: "对账自愈",
  "doctor-fix": "体检修复",
  "env-set": "环境写入",
  "env-remove": "环境移除",
  "sys-install": "软件安装",
  "sys-uninstall": "软件卸载",
  service: "服务操作",
  "cache-clean": "缓存清理",
  "snapshot-export": "快照导出",
  "snapshot-restore": "快照重建",
};

const STACK = [
  "Tauri 2",
  "React + TypeScript",
  "Rust",
  "CSS (亚克力 / 液态玻璃)",
  "mise · asdf · GitHub Releases · 官方生态源",
];

const LINKS: { label: string; url: string }[] = [
  { label: "GitHub: 3yearsZhuang", url: "https://github.com/3yearsZhuang" },
  { label: "mise 官方文档", url: "https://mise.jdx.dev" },
  { label: "mise install", url: "https://mise.jdx.dev/getting-started.html" },
  { label: "GitHub: mise", url: "https://github.com/jdx/mise" },
];

/** 触发彩蛋的连续点击次数 */
const EASTER_CLICKS = 6;

interface Props {
  onEaster: () => void;
}

type UpState = "idle" | "checking" | "downloading" | "none" | "error";

export default function SettingsView({ onEaster }: Props) {
  const clickCount = useRef(0);
  const [version, setVersion] = useState(FALLBACK_VERSION);
  const [notice, setNotice] = useState<string | null>(null);

  // 应用更新
  const [update, setUpdate] = useState<Update | null>(null);
  const [upState, setUpState] = useState<UpState>("idle");
  const [upMsg, setUpMsg] = useState("");
  const [upProgress, setUpProgress] = useState(0);

  // 开机自启
  const [autoStart, setAutoStart] = useState(false);

  // 环境体检
  const [doctor, setDoctor] = useState<DoctorCheck[] | null>(null);
  const [doctorRunning, setDoctorRunning] = useState(false);
  const [doctorMsg, setDoctorMsg] = useState("");
  const [fixingId, setFixingId] = useState<string | null>(null);

  // 操作历史
  const [history, setHistory] = useState<HistoryEntry[] | null>(null);

  // 环境快照
  const [snapBusy, setSnapBusy] = useState(false);
  const [snapMsg, setSnapMsg] = useState("");

  function loadHistory() {
    historyList()
      .then(setHistory)
      .catch(() => {});
  }

  async function handleSnapshotExport() {
    const p = await save({
      title: "保存环境快照",
      defaultPath: "zenv-snapshot.toml",
      filters: [{ name: "TOML", extensions: ["toml"] }],
    });
    if (!p) return;
    setSnapBusy(true);
    setSnapMsg("正在导出…");
    try {
      const s = await snapshotExport(p);
      setSnapMsg(
        `已导出 ${s.path}\n mise 工具 ${s.tools} 个 · 全局 env ${s.envVars} 个 · brew 软件 ${s.brewPackages} 个`,
      );
      loadHistory();
    } catch (e) {
      setSnapMsg(`导出失败：${errorMessage(e)}`);
    } finally {
      setSnapBusy(false);
    }
  }

  async function handleSnapshotRestore() {
    const p = await open({
      multiple: false,
      title: "选择环境快照",
      filters: [{ name: "TOML", extensions: ["toml"] }],
    });
    if (!p || typeof p !== "string") return;
    if (
      !window.confirm(
        "重建将：写入快照中的全局 env、逐个安装 mise 工具（耗时可能较长）、生成 Brewfile（brew 软件需你执行一条 brew bundle 命令安装，不会自动装）。继续？",
      )
    ) {
      return;
    }
    setSnapBusy(true);
    setSnapMsg("重建中，安装工具可能需要几分钟…");
    try {
      setSnapMsg(await snapshotRestore(p));
      loadHistory();
    } catch (e) {
      setSnapMsg(`重建失败：${errorMessage(e)}`);
    } finally {
      setSnapBusy(false);
    }
  }

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
    isAutostartEnabled()
      .then(setAutoStart)
      .catch(() => {});
    loadHistory();
  }, []);

  const handleVersionClick = () => {
    clickCount.current += 1;
    if (clickCount.current >= EASTER_CLICKS) {
      clickCount.current = 0;
      onEaster();
    }
  };

  async function handleCheckUpdate() {
    setUpState("checking");
    setUpMsg("正在检查更新…");
    setUpdate(null);
    try {
      const u = await check();
      if (u) {
        setUpdate(u);
        setUpMsg(`发现新版本 ${u.version}（当前 ${version}）`);
      } else {
        setUpState("none");
        setUpMsg("已是最新版本");
      }
    } catch (e) {
      setUpState("error");
      setUpMsg(errorMessage(e));
    }
  }

  async function handleInstallUpdate() {
    if (!update) return;
    setUpState("downloading");
    setUpProgress(0);
    let total = 0;
    let received = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          if (total > 0) {
            setUpProgress(Math.min(100, Math.round((received / total) * 100)));
          }
        } else if (event.event === "Finished") {
          setUpMsg("下载完成，即将重启应用…");
        }
      });
      await relaunch();
    } catch (e) {
      setUpState("error");
      setUpMsg(errorMessage(e));
    }
  }

  async function toggleAutoStart() {
    try {
      if (autoStart) {
        await disableAutostart();
        setAutoStart(false);
        setNotice("已关闭开机自启");
      } else {
        await enableAutostart();
        setAutoStart(true);
        setNotice("已开启开机自启");
      }
    } catch (e) {
      setNotice(`设置开机自启失败：${errorMessage(e)}`);
    }
  }

  async function handleDoctor() {
    setDoctorRunning(true);
    setDoctorMsg("");
    try {
      setDoctor(await doctorRun());
    } catch (e) {
      setDoctorMsg(`体检失败：${errorMessage(e)}`);
    } finally {
      setDoctorRunning(false);
    }
  }

  async function handleDoctorFix(id: string) {
    setFixingId(id);
    setDoctorMsg("");
    try {
      setDoctorMsg(await doctorFix(id));
      // 修复后自动重跑体检，让结果即时反映修复效果
      setDoctor(await doctorRun());
    } catch (e) {
      setDoctorMsg(`修复失败：${errorMessage(e)}`);
    } finally {
      setFixingId(null);
    }
  }

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>设置</h1>
          <p className="view-sub">关于与偏好</p>
        </div>
      </div>

      {notice && (
        <div className="banner info" onClick={() => setNotice(null)}>
          {notice}
        </div>
      )}

      <section className="about-hero">
        <div className="about-mark">
          <img src="/zenv-icon.svg" alt={APP_NAME} draggable={false} />
        </div>
        <div className="about-title">{APP_NAME}</div>
        <div className="about-version">
          版本{" "}
          <code onClick={handleVersionClick} title="?">
            {version}
          </code>
        </div>
        <p className="about-desc">
          {APP_TAGLINE}。将 <strong>mise</strong> 包装为可视化界面，统合系统资源监控、
          运行时工具管理与项目环境预设。
        </p>
      </section>

      <section className="panel">
        <h2 className="panel-title">通用</h2>
        <div className="setting-list">
          <button
            className="setting-row"
            onClick={toggleAutoStart}
            title="登录系统时自动启动 Z.Env"
          >
            <span className="setting-info" style={{ flex: 1 }}>
              <span className="setting-name">开机自启</span>
              <span className="setting-desc">登录系统时自动启动 Z.Env 并常驻托盘</span>
            </span>
            <span className={`pill ${autoStart ? "active" : "muted"}`}>
              {autoStart ? "已开启" : "已关闭"}
            </span>
          </button>
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">环境体检</h2>
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
          <button className="btn primary" onClick={handleDoctor} disabled={doctorRunning}>
            {doctorRunning ? "巡检中…" : "开始体检"}
          </button>
          {doctorMsg && <span className="setting-desc">{doctorMsg}</span>}
        </div>
        {doctor && (
          <div className="setting-list" style={{ marginTop: 12 }}>
            {doctor.map((c) => (
              <div key={c.id} className="setting-row" style={{ cursor: "default" }}>
                <span className="setting-info" style={{ flex: 1 }}>
                  <span
                    className="setting-name"
                    style={{ display: "flex", alignItems: "center", gap: 8 }}
                  >
                    <span
                      style={{
                        width: 8,
                        height: 8,
                        borderRadius: "50%",
                        flexShrink: 0,
                        background:
                          c.level === "ok"
                            ? "var(--success)"
                            : c.level === "warn"
                              ? "var(--warning)"
                              : "var(--destructive)",
                      }}
                    />
                    {c.title}
                  </span>
                  <span className="setting-desc">
                    {c.detail}
                    {c.hint ? ` · 建议：${c.hint}` : ""}
                  </span>
                </span>
                {c.fixable && c.level !== "ok" && (
                  <button
                    className="btn"
                    disabled={fixingId !== null}
                    onClick={() => handleDoctorFix(c.id)}
                  >
                    {fixingId === c.id ? "修复中…" : "一键修复"}
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="panel">
        <h2 className="panel-title">应用更新</h2>
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
          <button
            className="btn primary"
            onClick={handleCheckUpdate}
            disabled={upState === "checking" || upState === "downloading"}
          >
            {upState === "checking" ? "检查中…" : "检查更新"}
          </button>
          {upMsg && (
            <span className="setting-desc">
              {upMsg}
              {upState === "downloading" ? ` ${upProgress}%` : ""}
            </span>
          )}
          {update && upState !== "downloading" && (
            <button className="btn primary" onClick={handleInstallUpdate}>
              下载并安装
            </button>
          )}
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">环境快照与迁移</h2>
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
          <button className="btn primary" onClick={handleSnapshotExport} disabled={snapBusy}>
            {snapBusy ? "处理中…" : "导出快照"}
          </button>
          <button className="btn" onClick={handleSnapshotRestore} disabled={snapBusy}>
            从快照重建
          </button>
        </div>
        {snapMsg && (
          <p className="setting-desc" style={{ whiteSpace: "pre-wrap", marginTop: 8 }}>
            {snapMsg}
          </p>
        )}
        <p className="setting-desc" style={{ marginTop: 8 }}>
          导出 mise 已激活工具、全局 [env] 与 brew 软件清单为单个 TOML；新机上重建时 env
          直接写入、mise 工具自动安装，brew 软件生成 Brewfile 后执行一条 brew bundle 即可。
        </p>
      </section>

      <section className="panel">
        <h2 className="panel-title">操作历史</h2>
        <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8 }}>
          <span className="setting-desc">本机管理动作留痕（~/.zenv/history.jsonl，不上传）</span>
          <span style={{ flex: 1 }} />
          <button className="btn" onClick={loadHistory}>
            刷新
          </button>
        </div>
        {history && history.length === 0 && (
          <div className="empty small">
            还没有管理动作记录。安装、接入、修复等操作都会留痕于此。
          </div>
        )}
        <div className="setting-list">
          {(history ?? []).slice(0, 50).map((h, i) => (
            <div key={`${h.time}-${i}`} className="setting-row" style={{ cursor: "default" }}>
              <span className="setting-info" style={{ flex: 1, minWidth: 0 }}>
                <span
                  className="setting-name"
                  style={{ display: "flex", alignItems: "center", gap: 8 }}
                >
                  <span className="pill muted">{OP_KINDS[h.kind] ?? h.kind}</span>
                  <span className="setting-desc">{new Date(h.time * 1000).toLocaleString()}</span>
                </span>
                <span className="setting-desc" style={{ wordBreak: "break-all" }}>
                  {h.detail}
                </span>
              </span>
            </div>
          ))}
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">技术栈</h2>
        <div className="about-tags">
          {STACK.map((s) => (
            <span className="pill muted" key={s}>
              {s}
            </span>
          ))}
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">相关链接</h2>
        <div className="about-links">
          {LINKS.map((l) => (
            <a key={l.url} href={l.url} target="_blank" rel="noreferrer">
              {l.label} ↔
            </a>
          ))}
        </div>
      </section>

      <p className="about-foot">
        CC BY-NC-SA 4.0 · 用 <strong>mise</strong> 管理你的运行时，让环境回归简单。
      </p>
    </div>
  );
}
