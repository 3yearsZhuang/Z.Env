// 整机安装：把一组预设工具逐个装进 mise 全局，带流式进度与单工具容错。
// 「环境」页的预设区块直接用；「项目配置」页只负责把配置写进项目，不走这里。
import { useCallback, useEffect, useRef, useState } from "react";
import {
  errorMessage,
  installPreset,
  onInstallProgress,
  operationCancel,
  type PresetTool,
} from "../api";

export interface MachineInstallApi {
  /** 安装日志（逐行），由 install 追加 */
  installLog: string[];
  setInstallLog: React.Dispatch<React.SetStateAction<string[]>>;
  /** 安装进行中 */
  installBusy: boolean;
  cancelRequested: boolean;
  cancel: () => Promise<void>;
  /** 挂到日志 <pre> 上即可自动滚到底部 */
  logRef: React.RefObject<HTMLPreElement | null>;
  /** 逐个流式安装；单个失败不中断，返回成功数 */
  install: (tools: PresetTool[]) => Promise<number>;
}

export function useMachineInstall(): MachineInstallApi {
  const [installLog, setInstallLog] = useState<string[]>([]);
  const [installBusy, setInstallBusy] = useState(false);
  const [cancelRequested, setCancelRequested] = useState(false);
  const currentJob = useRef<string | null>(null);
  const logRef = useRef<HTMLPreElement | null>(null);

  // 日志自动滚到底部
  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [installLog]);

  const install = useCallback(async (tools: PresetTool[]) => {
    const jobId = crypto.randomUUID();
    currentJob.current = jobId;
    setCancelRequested(false);
    setInstallBusy(true);
    setInstallLog([]);
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await onInstallProgress((p) => {
        setInstallLog((prev) => [...prev.slice(-499), `[${p.tool}] ${p.line}`]);
      }, jobId);
    } catch {
      /* 订阅失败不阻断安装，只是没有实时输出 */
    }

    let ok = 0;
    try {
      ok = await installPreset(tools, jobId);
    } catch (e) {
      setInstallLog((prev) => [...prev, errorMessage(e)]);
    } finally {
      unlisten?.();
      setInstallBusy(false);
      currentJob.current = null;
    }
    return ok;
  }, []);

  const cancel = useCallback(async () => {
    if (!currentJob.current) return;
    try {
      await operationCancel(currentJob.current);
      setCancelRequested(true);
    } catch (e) {
      setInstallLog((prev) => [...prev.slice(-499), errorMessage(e)]);
    }
  }, []);
  return { installLog, setInstallLog, installBusy, logRef, install, cancel, cancelRequested };
}
