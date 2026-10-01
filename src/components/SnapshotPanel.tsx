// 「环境」页的整机快照区块：把本机现状打包成 TOML，或从快照重建。
// 快照含 mise 已激活工具 + 全局 [env] + brew 软件清单；重建时的工具安装与
// 「环境预设 → 装到整机」共用同一条流式进度通道（mise:install-progress）。
import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { errorMessage, onInstallProgress, snapshotExport, snapshotRestore } from "../api";

interface Props {
  /** 重建会改写全局 [env]，完成后回调页面刷新环境变量快照 */
  onRestored?: () => void;
}

export default function SnapshotPanel({ onRestored }: Props) {
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const logRef = useRef<HTMLPreElement | null>(null);

  // 重建日志自动滚到底部
  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [log]);

  async function handleExport() {
    const p = await save({
      title: "保存环境快照",
      defaultPath: "zenv-snapshot.toml",
      filters: [{ name: "TOML", extensions: ["toml"] }],
    });
    if (!p) return;
    setBusy(true);
    setMsg("正在导出…");
    try {
      const s = await snapshotExport(p);
      setMsg(
        `已导出 ${s.path}\nmise 工具 ${s.tools} 个 · 全局 env ${s.envVars} 个 · brew 软件 ${s.brewPackages} 个`,
      );
    } catch (e) {
      setMsg(`导出失败：${errorMessage(e)}`);
    } finally {
      setBusy(false);
    }
  }

  async function handleRestore() {
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
    setBusy(true);
    setLog([]);
    setMsg("重建中，安装工具可能需要几分钟…");

    let unlisten: (() => void) | undefined;
    try {
      unlisten = await onInstallProgress((e) => {
        setLog((prev) => [...prev, `[${e.tool}] ${e.line}`]);
      });
    } catch {
      /* 订阅失败不阻断重建，只是没有实时输出 */
    }
    try {
      setMsg(await snapshotRestore(p));
      onRestored?.();
    } catch (e) {
      setMsg(`重建失败：${errorMessage(e)}`);
    } finally {
      unlisten?.();
      setBusy(false);
    }
  }

  return (
    <section className="panel">
      <h2 className="panel-title">整机快照与迁移</h2>
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
        <button className="btn primary" onClick={handleExport} disabled={busy}>
          {busy ? "处理中…" : "导出快照"}
        </button>
        <button className="btn" onClick={handleRestore} disabled={busy}>
          从快照重建
        </button>
      </div>
      {msg && (
        <p className="setting-desc" style={{ whiteSpace: "pre-wrap", marginTop: 8 }}>
          {msg}
        </p>
      )}
      {log.length > 0 && (
        <pre className="terminal" ref={logRef} style={{ marginTop: 8 }}>
          {log.join("\n")}
        </pre>
      )}
      <p className="setting-desc" style={{ marginTop: 8 }}>
        快照是<strong>本机现状</strong>的完整打包：mise 已激活工具 + 全局 [env] + brew 软件清单。
        换机或重装前导出，新机上重建时 env 直接写入、mise 工具自动安装，brew 软件生成 Brewfile
        后执行一条 <code>brew bundle</code> 即可。
        <br />
        只想装一套<strong>挑选好的</strong>运行时（而不是还原本机现状），用上面的环境预设。
      </p>
    </section>
  );
}
