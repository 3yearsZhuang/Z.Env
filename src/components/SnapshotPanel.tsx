// 「环境」页的整机快照区块：把本机现状打包成 TOML，或从快照重建。
// 快照含 mise 已激活工具 + 全局 [env] + brew 软件清单；重建时的工具安装与
// 「环境预设 → 装到整机」共用同一条流式进度通道（mise:install-progress）。
import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  errorMessage,
  onInstallProgress,
  snapshotExport,
  snapshotRestore,
  snapshotPreview,
} from "../api";
import { installChannelGate } from "../lib/installChannel";

interface Props {
  /** 重建会改写全局 [env]，完成后回调页面刷新环境变量快照 */
  onRestored?: () => void;
  /** 「装到整机」预设安装进行中——同一安装通道被占用，本区块按钮须禁用 */
  machineInstallBusy?: boolean;
  /** 重建开始/结束时上报通道占用（导出不动安装通道，不上报） */
  onRestoreBusyChange?: (busy: boolean) => void;
}

export default function SnapshotPanel({
  onRestored,
  machineInstallBusy,
  onRestoreBusyChange,
}: Props) {
  const [includeEnv, setIncludeEnv] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const logRef = useRef<HTMLPreElement | null>(null);
  // 通道占用判定：另一路（预设安装）在跑时禁用入口并提示
  const gate = installChannelGate(busy, machineInstallBusy ?? false, "preset");

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
      const s = await snapshotExport(p, includeEnv);
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
    let plan;
    try {
      plan = await snapshotPreview(p);
    } catch (e) {
      setMsg(`预览失败：${errorMessage(e)}`);
      return;
    }
    const summary = Object.entries(plan.tools)
      .map(([name, versions]) => `${name}: ${versions.join(", ")}`)
      .join("\n");
    if (
      !window.confirm(
        `将恢复这些全局版本：\n${summary || "（无）"}\n环境变量：${plan.envKeys.join(", ") || "（无）"}\nbrew 包：${plan.brewPackages}\n${plan.warnings.join("\n")}\n已有配置会备份；继续？`,
      )
    )
      return;
    const jobId = crypto.randomUUID();
    setBusy(true);
    setLog([]);
    setMsg("重建中，安装工具可能需要几分钟…");
    onRestoreBusyChange?.(true);

    let unlisten: (() => void) | undefined;
    try {
      unlisten = await onInstallProgress((e) => {
        setLog((prev) => [...prev.slice(-499), `[${e.tool}] ${e.line}`]);
      }, jobId);
    } catch {
      /* 订阅失败不阻断重建，只是没有实时输出 */
    }
    try {
      setMsg(await snapshotRestore(p, plan, jobId));
    } catch (e) {
      setMsg(`重建失败：${errorMessage(e)}`);
    } finally {
      unlisten?.();
      setBusy(false);
      onRestoreBusyChange?.(false);
      onRestored?.();
    }
  }

  return (
    <section className="panel">
      <h2 className="panel-title">整机快照与迁移</h2>
      <label className="setting-desc">
        <input
          type="checkbox"
          checked={includeEnv}
          onChange={(e) => setIncludeEnv(e.target.checked)}
        />{" "}
        导出包含全局环境变量（可能含密钥，请妥善保管）
      </label>
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
        <button className="btn primary" onClick={handleExport} disabled={busy}>
          {busy ? "处理中…" : "导出快照"}
        </button>
        <button
          className="btn"
          onClick={handleRestore}
          disabled={gate.selfBusy || gate.blocked}
          title={gate.blockHint ?? undefined}
        >
          从快照重建
        </button>
        {gate.blockHint && <span className="pill muted">{gate.blockHint}</span>}
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
        快照包含全局激活的全部 mise 版本、可选全局 [env]、brew formula/cask 清单。
        换机或重装前导出，重建前预览，安装成功后原子提交配置并验证激活；brew 软件生成 Brewfile
        后执行一条 <code>brew bundle</code> 即可。
        <br />
        只想装一套<strong>挑选好的</strong>运行时（而不是还原本机现状），用上面的环境预设。
      </p>
    </section>
  );
}
