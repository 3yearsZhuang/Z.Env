import { useEffect, useRef, useState } from "react";
import { ChevronUp, CircleCheck, CircleX, ListTodo, LoaderCircle, X } from "lucide-react";
import { errorMessage, operationCancel, operationList, type Operation } from "../api";
import "./TaskPanel.css";

const LABELS = {
  running: "进行中",
  completed: "已完成",
  partial: "部分失败",
  failed: "失败",
  cancelled: "已取消",
  interrupted: "已中断，未重放",
};
const STEP_LABELS = {
  preflight: "前置条件",
  command: "执行变更",
  refresh: "重读来源",
  verify: "确认结果",
  compensate: "补偿本次变更",
};
const STEP_STATUSES = {
  planned: "待执行",
  running: "进行中",
  completed: "已完成",
  failed: "未完成",
  skipped: "已跳过",
};

/** 任务属于应用；切页或重新加载后仍可从后端恢复查看与取消。 */
export default function TaskPanel() {
  const [jobs, setJobs] = useState<Operation[]>([]);
  const [error, setError] = useState("");
  const [disconnected, setDisconnected] = useState(false);
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());
  const pendingRef = useRef(new Set<string>());
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await operationList();
        if (!stopped) {
          setJobs(next);
          setDisconnected(false);
          // 取消请求已发出后，等待后端确认终态；慢速轮询不能重新开放取消按钮。
          const pending = pendingRef.current;
          for (const id of pending) {
            if (!next.some((job) => job.id === id && job.status === "running")) pending.delete(id);
          }
          setPendingIds(new Set(pending));
        }
      } catch {
        if (!stopped) setDisconnected(true);
      }
      if (!stopped) timer = setTimeout(poll, 1000);
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, []);

  async function cancel(job: Operation) {
    if (job.cancelRequested || pendingRef.current.has(job.id)) return;
    pendingRef.current.add(job.id);
    setPendingIds(new Set(pendingRef.current));
    setError("");
    try {
      await operationCancel(job.id);
    } catch (e) {
      pendingRef.current.delete(job.id);
      setPendingIds(new Set(pendingRef.current));
      setError(`无法取消「${job.label}」：${errorMessage(e)}`);
    }
  }

  if (!jobs.length) return null;
  const active = jobs.find((j) => j.status === "running");
  return (
    <details className="task-panel task-center">
      <summary className="task-center-toggle">
        {active ? (
          <LoaderCircle size={16} className="task-center-spinner" aria-hidden="true" />
        ) : (
          <ListTodo size={16} aria-hidden="true" />
        )}
        <span className="task-center-title">{active ? `${active.label}进行中…` : "最近任务"}</span>
        <span className="task-center-count">{jobs.length}</span>
        <ChevronUp size={15} className="task-center-chevron" aria-hidden="true" />
      </summary>
      <div className="task-center-body">
        {disconnected && (
          <p className="task-center-warning" role="status">
            任务状态暂时无法更新，正在重试。下方显示上次获取的结果。
          </p>
        )}
        {error && (
          <div className="task-center-error" role="alert">
            <span>{error}</span>
            <button
              className="btn-ghost icon-only"
              aria-label="关闭任务错误提示"
              onClick={() => setError("")}
            >
              <X size={14} />
            </button>
          </div>
        )}
        {jobs.map((job) => {
          const cancelling = job.cancelRequested || pendingIds.has(job.id);
          return (
            <section key={job.id} className="task-center-entry" aria-label={job.label}>
              <div className="task-center-entry-head">
                <strong>{job.label}</strong>
                <span className={`task-center-status ${job.status}`}>
                  {job.status === "completed" && <CircleCheck size={12} aria-hidden="true" />}
                  {(job.status === "failed" ||
                    job.status === "partial" ||
                    job.status === "interrupted") && <CircleX size={12} aria-hidden="true" />}
                  {job.status === "running" && cancelling ? "正在取消" : LABELS[job.status]}
                </span>
              </div>
              {job.steps && job.steps.length > 0 && (
                <ol
                  aria-label={`${job.label}的执行步骤`}
                  style={{ paddingLeft: 20, fontSize: 12, lineHeight: 1.8 }}
                >
                  {job.steps.map((step, index) => (
                    <li key={index}>
                      {STEP_LABELS[step.kind]} · {STEP_STATUSES[step.status]}
                    </li>
                  ))}
                </ol>
              )}
              {job.compensation && (
                <p role="status">
                  {job.compensation === "completed"
                    ? "本次补偿已完成，请刷新确认原来源状态。"
                    : job.compensation === "conflict"
                      ? "补偿遇到外部修改，未覆盖冲突内容；请检查绑定记录与配置备份。"
                      : "补偿未完成，请检查绑定记录与配置备份。"}
                </p>
              )}
              <div className="task-center-actions">
                <details className="task-center-log">
                  <summary>查看输出 · {job.lines.length} 行</summary>
                  <pre className="terminal" tabIndex={0} aria-label={`${job.label}的任务输出`}>
                    {job.lines.join("\n") ||
                      (job.status === "running" ? "等待输出…" : "没有任务输出")}
                  </pre>
                </details>
                {job.status === "running" && (
                  <button
                    className="btn xs task-center-cancel"
                    disabled={cancelling}
                    aria-label={`取消任务：${job.label}`}
                    onClick={() => void cancel(job)}
                  >
                    {cancelling ? "正在取消…" : "取消任务"}
                  </button>
                )}
              </div>
            </section>
          );
        })}
      </div>
    </details>
  );
}
