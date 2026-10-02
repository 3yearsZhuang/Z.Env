// 「环境」页的环境预设区块：选一套配方，一键装到整机（不依赖任何项目）。
// 项目级用法（把预设装进某个项目）在「项目配置」页；两页共用同一份预设库。
import { useState } from "react";
import {
  errorMessage,
  listTools,
  presetParseTools,
  type PresetFile,
  type PresetTool,
} from "../api";
import type { Navigate } from "../lib/nav";
import { presetToToml, toolsOf } from "../lib/preset";
import { installChannelGate } from "../lib/installChannel";
import { exportPresetFile } from "../lib/presetFileIO";
import type { MachineInstallApi } from "../lib/useMachineInstall";
import { useUserPresets } from "../lib/useUserPresets";
import PresetLibrary, { type PresetChoice } from "./PresetLibrary";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";

interface Props {
  /** 切到别的 tab（弹窗里给出「装到某个项目」的出口） */
  onNavigate?: Navigate;
  /** 安装 API 由页面持有：与快照重建共用同一份 busy 状态（互斥）与事件订阅 */
  installApi: MachineInstallApi;
  /** 快照重建进行中（同一安装通道被占用，本区块入口须让位） */
  channelBusy?: boolean;
}

export default function MachinePresetSection({ onNavigate, installApi, channelBusy }: Props) {
  const presets = useUserPresets();
  const { installLog, setInstallLog, installBusy, logRef, install } = installApi;

  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // 预设装到整机：先预览工具清单与已装状态，再确认安装
  const [pending, setPending] = useState<PresetChoice | null>(null);
  const [pendingTools, setPendingTools] = useState<PresetTool[]>([]);
  const [installedNames, setInstalledNames] = useState<Set<string>>(new Set());

  /** 打开预设预览弹窗（解析出工具清单）；快照重建占用通道时拒绝进入 */
  async function openInstall(choice: PresetChoice) {
    const gate = installChannelGate(installBusy, channelBusy ?? false, "snapshot");
    if (gate.blocked) {
      setError(gate.blockHint);
      return;
    }
    setError(null);
    setNotice(null);
    try {
      const tools = await presetParseTools(choice.toml);
      if (tools.length === 0) {
        setError(`预设「${choice.name}」里没有可安装的工具`);
        return;
      }
      // 已装状态仅供预览参考，取不到就不标注
      try {
        const all = await listTools();
        setInstalledNames(new Set(all.map((t) => t.name)));
      } catch {
        setInstalledNames(new Set());
      }
      setInstallLog([]);
      setPendingTools(tools);
      setPending(choice);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function confirmInstall() {
    if (!pending) return;
    const total = pendingTools.length;
    const ok = await install(pendingTools);
    setNotice(`「${pending.name}」整机安装完成：成功 ${ok}/${total}`);
    if (ok < total) {
      setInstallLog((prev) => [...prev, `⚠ ${total - ok} 个工具未装成功，详见上方日志`]);
    }
  }

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

  return (
    <>
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

      <PresetLibrary
        presets={presets}
        useHint="点击预览并装到整机"
        useDisabled={channelBusy}
        useDisabledHint="快照重建进行中，暂不能安装预设"
        onUse={(choice) => void openInstall(choice)}
        onExport={(choice) => void handleExportPreset(choice)}
        onImported={(file: PresetFile) =>
          void openInstall({
            name: file.name,
            description: file.description ?? "",
            toml: presetToToml(toolsOf(file.tools)),
          })
        }
        onError={setError}
      />

      <Dialog
        open={pending !== null}
        onOpenChange={(o) => {
          if (!o && !installBusy) setPending(null);
        }}
      >
        <DialogContent className="w-[520px] p-0" showClose={!installBusy}>
          <div className="dialog-head">
            <DialogTitle>装到整机</DialogTitle>
            <DialogDescription className="setting-desc">
              {pending ? `${pending.name} · 共 ${pendingTools.length} 个工具` : ""}
            </DialogDescription>
          </div>
          <div className="dialog-body">
            {pending && (
              <>
                <div className="dialog-tip">
                  这些运行时会装进 mise 全局存储，任何项目都能直接用；不会改动任何项目的
                  mise.toml。已装状态按本机现状标注，已装的会重新校验版本。
                </div>
                <div className="adopt-list">
                  {pendingTools.map((t) => (
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
                {installLog.length > 0 && (
                  <pre className="terminal" ref={logRef}>
                    {installLog.join("\n")}
                  </pre>
                )}
              </>
            )}
          </div>
          <div className="dialog-foot">
            <button className="btn-ghost" onClick={() => setPending(null)} disabled={installBusy}>
              关闭
            </button>
            <span style={{ flex: 1 }} />
            {onNavigate && (
              <button
                className="btn"
                onClick={() => {
                  setPending(null);
                  onNavigate("projects");
                }}
                disabled={installBusy}
                title="只想装进某个项目？去项目配置页"
              >
                装到某个项目
              </button>
            )}
            <button className="btn primary" onClick={confirmInstall} disabled={installBusy}>
              {installBusy ? "安装中…" : "一键装到整机"}
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
