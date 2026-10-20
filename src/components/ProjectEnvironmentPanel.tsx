import { lazy, Suspense, useState } from "react";
import type {
  EnvironmentContext,
  ProjectCandidate,
  RequirementMatch,
} from "../api/projectEnvironment";
import { useProjectEnvironment } from "../lib/useProjectEnvironment";
import "./ProjectEnvironmentPanel.css";
const OperationPlanDialog = lazy(() => import("./OperationPlanDialog"));
const MATCH: Record<RequirementMatch, string> = {
  satisfied: "满足声明",
  mismatch: "版本不匹配",
  unknown: "是否满足未知",
};
function ContextCard({
  title,
  result,
  stale,
}: {
  title: string;
  result: EnvironmentContext;
  stale: boolean;
}) {
  const mismatch = result.requirementMatch === "mismatch" || result.selectionMatch === "mismatch";
  return (
    <section
      className={`project-env-context${mismatch && !stale ? " project-env-mismatch" : ""}`}
      aria-label={title}
    >
      <h4>{title}</h4>
      <p>
        {stale
          ? "上次结果 · 待重新核对"
          : result.status === "verified"
            ? `已核对 ${result.version ?? "未知版本"} · ${MATCH[result.requirementMatch]}`
            : result.status === "notRequested"
              ? "尚未探测"
              : result.status === "changed"
                ? `来源已变化 · ${result.version ?? "版本未知"}`
                : "实际解析未知"}
      </p>
      {mismatch && !stale && <strong>版本不匹配，不能确认环境满足</strong>}
      {result.executablePath && <code>{result.executablePath}</code>}
      <p className="project-env-note">{result.reason}</p>
    </section>
  );
}
interface Props {
  path: string;
  active?: boolean;
  dirty: boolean;
  busy: boolean;
  refreshKey: number;
  onChanged: () => void;
}
export default function ProjectEnvironmentPanel({
  path,
  active = true,
  dirty,
  busy,
  refreshKey,
  onChanged,
}: Props) {
  const state = useProjectEnvironment(path, active, refreshKey);
  const [planning, setPlanning] = useState<ProjectCandidate | "install" | null>(null);
  const report = state.report;
  const canPlan = Boolean(report && !state.stale && !dirty && !busy && active);
  const matches =
    report?.candidates.filter(
      (candidate) => candidate.usable && candidate.requirementMatch === "satisfied",
    ) ?? [];
  return (
    <section
      className="project-environment"
      aria-label="Node 项目环境核对"
      aria-busy={state.loading}
    >
      <div className="project-env-heading">
        <div>
          <h3>Node.js 项目环境</h3>
          <p>读取磁盘声明，分别核对候选、配置选择与受控解析。</p>
        </div>
        <div className="project-env-actions">
          <button
            className="btn"
            onClick={() => void state.refresh()}
            disabled={!active || state.loading || busy}
          >
            刷新来源与声明
          </button>
          <button
            className="btn"
            onClick={() => void state.probe()}
            disabled={!active || state.loading || busy || !state.snapshotId}
          >
            验证受控上下文
          </button>
          {state.loading && (
            <button className="btn" onClick={() => void state.cancel()} disabled={state.cancelling}>
              {state.cancelling ? "等待取消完成…" : "取消项目核对"}
            </button>
          )}
        </div>
      </div>
      {dirty && (
        <p className="project-env-warning">
          草稿尚未保存；以下只核对磁盘内容，绑定前请保存或重新读取。
        </p>
      )}
      {state.error && (
        <p role="alert" className="project-env-warning">
          项目核对失败：{state.error}。保留的旧结果不能用于确认环境。
        </p>
      )}
      {state.loading && <p role="status">正在读取来源或核对项目…</p>}
      {report ? (
        <div className={state.stale ? "project-env-stale" : ""}>
          {state.stale && <p role="status">上次结果 · 当前项目或清单已变化，等待重新核对。</p>}
          <section aria-label="Node 声明要求">
            <h4>声明要求</h4>
            <p>
              {report.requirementState === "conflict"
                ? "声明存在冲突"
                : report.requirementState === "unknown"
                  ? "部分声明无法确定"
                  : report.requirementState === "unspecified"
                    ? "未声明 Node 版本要求"
                    : "已解析版本范围"}
            </p>
            {report.declarations.length ? (
              <ul>
                {report.declarations.map((declaration, index) => (
                  <li key={index}>
                    <strong>{declaration.source}</strong> ·{" "}
                    <code>{declaration.requirement ?? "要求未知"}</code>
                    <p>
                      <code>{declaration.path}</code>
                    </p>
                    {declaration.reason && <p>{declaration.reason}</p>}
                  </li>
                ))}
              </ul>
            ) : (
              <p>工具指纹或 package.json 存在不能证明版本满足。</p>
            )}
          </section>
          {report.issues.length > 0 && (
            <ul className="project-env-warning" aria-label="项目环境问题">
              {report.issues.map((item) => (
                <li key={item.code}>{item.message}</li>
              ))}
            </ul>
          )}
          <section aria-label="Node 配置选择">
            <h4>配置选择</h4>
            <p>
              {report.selection.scope === "global" ? "全局配置中的选择" : "项目选择"} ·{" "}
              {report.selection.kind === "mise"
                ? `mise ${report.selection.request ?? "未知"}`
                : report.selection.kind === "path"
                  ? "项目 PATH"
                  : "尚未确认"}
            </p>
            {report.selection.path && <code>{report.selection.path}</code>}
            {report.selection.pathEntries.map((entry) => (
              <p key={entry}>
                <code>{entry}</code>
              </p>
            ))}
            {report.selection.reason && <p>{report.selection.reason}</p>}
          </section>
          <section aria-label="Node 可用候选">
            <h4>安装候选 · {matches.length} 个已验证匹配实例</h4>
            {!report.inventoryComplete && <p>部分来源未完整读取，不能据此断言没有安装。</p>}
            {!matches.length && (
              <p>
                {report.requirementState === "conflict"
                  ? "请先解决声明冲突，再选择安装或绑定。"
                  : report.requirementState !== "supported"
                    ? "请先明确可解析的版本声明，再判断候选是否满足。"
                    : report.inventoryComplete
                      ? "完整来源范围内未发现可确认满足声明的实例。"
                      : "当前没有可确认的匹配实例，可刷新来源后重试。"}
              </p>
            )}
            <ul className="project-env-candidates">
              {report.candidates.slice(0, 64).map((candidate) => (
                <li key={candidate.installationId}>
                  <div>
                    <strong>
                      {candidate.providerPackage.providerId} · {candidate.version ?? "未知版本"}
                    </strong>
                    <span>
                      {MATCH[candidate.requirementMatch]} ·{" "}
                      {candidate.usable ? "来源已验证" : "来源待确认"}
                    </span>
                  </div>
                  <code>{candidate.locator}</code>
                  {candidate.reason && <p>{candidate.reason}</p>}
                  <button
                    className="btn"
                    disabled={
                      !canPlan ||
                      !candidate.usable ||
                      candidate.requirementMatch !== "satisfied" ||
                      report.requirementState !== "supported"
                    }
                    onClick={() => setPlanning(candidate)}
                  >
                    预览绑定此实例到项目
                  </button>
                </li>
              ))}
            </ul>
            {report.candidates.length > 64 && (
              <p>候选过多，仅展示前 64 条；全部来源记录仍可在本机工具页核对。</p>
            )}
            <button
              className="btn"
              onClick={() => setPlanning("install")}
              disabled={!canPlan || report.requirementState === "conflict"}
            >
              预览 Node 安装计划
            </button>
            <p className="project-env-note">
              mise 支持精确版本；Homebrew 按 formula
              渠道提供版本。安装完成后仍须显式选择并核对上下文。
            </p>
          </section>
          <div className="project-env-contexts">
            <ContextCard title="项目受控上下文" result={report.controlled} stale={state.stale} />
            <ContextCard
              title="应用继承 PATH"
              result={report.applicationPath}
              stale={state.stale}
            />
            <ContextCard title="用户终端与 IDE" result={report.terminal} stale={state.stale} />
          </div>
          <p className="project-env-note">
            受控验证不加载项目环境表达式、钩子或任务，不自动安装。它只验证当前选择的 Node
            投影；用户终端、IDE 和完整项目环境仍需分别确认。
          </p>
          <details>
            <summary>核对证据与范围</summary>
            <p>读取时间：{new Date(report.observedAt).toLocaleString("zh-CN")}</p>
            <p>
              清单身份：<code>{report.snapshotRequestId}</code>
            </p>
            <ul>
              {report.files
                .filter((file) => file.exists)
                .map((file) => (
                  <li key={file.path}>
                    <code>{file.path}</code>
                    <p>
                      SHA-256：<code>{file.sha256}</code>
                    </p>
                  </li>
                ))}
            </ul>
          </details>
        </div>
      ) : (
        !state.loading && !state.error && <p>选择并读取项目后开始核对；尚未确认实际运行版本。</p>
      )}
      {planning && active && state.snapshotId && (
        <Suspense fallback={<p role="status">正在加载操作计划…</p>}>
          <OperationPlanDialog
            key={planning === "install" ? "install" : planning.installationId}
            snapshotRequestId={state.snapshotId}
            installationId={planning === "install" ? null : planning.installationId}
            initialAction={planning === "install" ? "install" : "bind"}
            initialBinding={
              planning === "install"
                ? undefined
                : {
                    strategy: planning.providerPackage.providerId === "brew" ? "managed" : "direct",
                    scope: "project",
                    projectPath: path,
                  }
            }
            onClose={() => setPlanning(null)}
            onFinished={() => {
              onChanged();
              void state.refresh();
            }}
          />
        </Suspense>
      )}
    </section>
  );
}
