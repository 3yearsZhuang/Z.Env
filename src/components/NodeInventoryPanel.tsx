import { lazy, Suspense, useEffect, useId, useRef, useState } from "react";
import { AlertTriangle, ChevronDown, FileSearch, RefreshCw, X } from "lucide-react";
import type { Installation, InstallationAlias, ProviderId, ProviderReport } from "../api/inventory";
import { useInventory } from "../lib/useInventory";
import type { CatalogReadState } from "../lib/catalogEvidence";
import "./NodeInventoryPanel.css";
const OperationPlanDialog = lazy(() => import("./OperationPlanDialog"));

interface Props {
  active?: boolean;
  focusRequest?: number;
  refreshRequest?: number;
  onReadState?: (state: CatalogReadState) => void;
}

const PROVIDERS: { id: ProviderId; name: string; description: string }[] = [
  { id: "mise", name: "mise", description: "运行时目录" },
  { id: "brew", name: "Homebrew", description: "安装收据与目录" },
  { id: "nvm", name: "nvm", description: "版本目录" },
  { id: "path", name: "PATH", description: "可执行文件观测" },
];

const OWNERSHIP = {
  owned: "已确认归属",
  observed: "仅观测",
  unknown: "归属未知",
} as const;

const HEALTH = {
  healthy: "已验证可执行",
  broken: "异常",
  unverified: "未验证",
} as const;

function sourceName(providerId: ProviderId): string {
  return PROVIDERS.find((provider) => provider.id === providerId)?.name ?? providerId;
}

function isNode(toolId: string): boolean {
  return ["node", "nodejs", "node.js"].includes(toolId.toLowerCase());
}

function completeNodeCoverage(report: ProviderReport | undefined): boolean {
  return Boolean(
    report &&
    report.status === "ok" &&
    report.error === null &&
    report.coverage.complete &&
    !report.coverage.truncated &&
    !report.stale &&
    report.coverage.toolIds.some(isNode),
  );
}

function reportStatus(report: ProviderReport | undefined): string {
  if (!report) return "未知 · 尚未读取";
  switch (report.status) {
    case "ok":
      return completeNodeCoverage({ ...report, stale: false }) ? "读取完成" : "读取未完整";
    case "unavailable":
      return "未知 · 来源不可用";
    case "unsupported":
      return "当前平台不支持";
    case "permissionDenied":
      return "未知 · 权限不足";
    case "timeout":
      return "未知 · 读取超时";
    case "cancelled":
      return "未知 · 已取消";
    case "error":
      return "未知 · 读取失败";
  }
}

function ReadTime({ timestamp }: { timestamp: number }) {
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime()) || timestamp <= 0) return <span>读取时间未知</span>;
  return (
    <time dateTime={date.toISOString()}>
      {date.toLocaleString("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: false,
      })}
    </time>
  );
}

function focusInstallation(event: React.MouseEvent<HTMLAnchorElement>, id: string) {
  event.preventDefault();
  const summary = document.getElementById(id);
  if (summary?.parentElement instanceof HTMLDetailsElement) summary.parentElement.open = true;
  summary?.focus();
  summary?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
}

function InstanceRow({
  installation,
  summaryId,
  stale,
  aliases,
  installations,
  instanceIds,
  onPlan,
}: {
  installation: Installation;
  summaryId: string;
  stale: boolean;
  aliases: InstallationAlias[];
  installations: Installation[];
  instanceIds: Map<string, string>;
  onPlan: (installationId: string) => void;
}) {
  const { providerPackage } = installation;
  const provider = sourceName(providerPackage.providerId);
  const version = installation.version ?? "版本未知";
  const architecture = installation.architecture ?? "架构未知";
  return (
    <li className="node-inventory-instance">
      <details>
        <summary
          id={summaryId}
          aria-label={`${provider} ${providerPackage.packageId} ${version} ${architecture}，目录 ${installation.locator}`}
        >
          <span className="node-inventory-instance-main">
            <span className="node-inventory-instance-title">
              <strong>{version}</strong>
              <span className={`node-inventory-tag node-inventory-health-${installation.health}`}>
                {HEALTH[installation.health]}
              </span>
              {stale && <span className="node-inventory-tag node-inventory-stale">上次结果</span>}
            </span>
            <span className="node-inventory-instance-meta">
              <span>{provider}</span>
              <code>{providerPackage.packageId}</code>
              <span>{architecture}</span>
              <span>{OWNERSHIP[installation.ownership]}</span>
            </span>
            <code className="node-inventory-location">{installation.locator}</code>
          </span>
          <ChevronDown size={16} aria-hidden="true" className="node-inventory-disclosure" />
        </summary>
        <div className="node-inventory-instance-body">
          <button className="btn" onClick={() => onPlan(installation.installationId)}>
            预览此实例的操作
          </button>
          {stale && (
            <p className="node-inventory-stale-note">此记录来自上次结果，最新状态待确认。</p>
          )}
          <dl className="node-inventory-fields">
            <div>
              <dt>来源与包</dt>
              <dd>
                {provider} · <code>{providerPackage.packageId}</code>
              </dd>
            </div>
            <div>
              <dt>归属</dt>
              <dd>{OWNERSHIP[installation.ownership]}</dd>
            </div>
            <div>
              <dt>原始文件路径</dt>
              <dd>
                <code>{installation.executablePath ?? "未知"}</code>
              </dd>
            </div>
            <div>
              <dt>解析后文件路径</dt>
              <dd>
                <code>{installation.canonicalExecutablePath ?? "未知"}</code>
              </dd>
            </div>
          </dl>
          {providerPackage.providerId === "path" && installation.health === "unverified" && (
            <p className="node-inventory-evidence-note">
              仅保留文件证据；未自动执行未知来源的 PATH 文件。
            </p>
          )}
          {aliases.map((alias, index) => {
            const target = installations.find(
              (item) => item.installationId === alias.targetInstallationId,
            );
            const targetId = instanceIds.get(alias.targetInstallationId);
            return (
              <div className="node-inventory-alias" key={`${alias.targetInstallationId}-${index}`}>
                <strong>同一文件的来源记录</strong>
                <p>
                  {target && targetId ? (
                    <a
                      href={`#${targetId}`}
                      onClick={(event) => focusInstallation(event, targetId)}
                    >
                      查看 {sourceName(target.providerPackage.providerId)} ·{" "}
                      {target.providerPackage.packageId} · {target.version ?? "版本未知"} ·{" "}
                      {target.architecture ?? "架构未知"} 的关联记录
                    </a>
                  ) : (
                    "关联目标暂不可见，本条记录仍保留。"
                  )}{" "}
                  两条记录分别保留；文件关联不会改变各自归属。
                </p>
                <code>{alias.evidence.canonicalExecutablePath}</code>
                <p className="node-inventory-alias-meta">
                  关联版本：{alias.evidence.version ?? "未知"} · 架构：
                  {alias.evidence.architecture ?? "未知"}
                </p>
              </div>
            );
          })}
          <h3>观测证据</h3>
          {installation.evidence.length > 0 ? (
            <ul className="node-inventory-evidence">
              {installation.evidence.map((evidence, index) => (
                <li key={index}>
                  <code className="node-inventory-evidence-kind">{evidence.kind}</code>
                  <p>
                    <span>路径：</span>
                    <code>{evidence.path ?? "未提供"}</code>
                  </p>
                  <p>
                    <span>说明：</span>
                    {evidence.detail ?? "未提供"}
                  </p>
                </li>
              ))}
            </ul>
          ) : (
            <p className="node-inventory-evidence-note">此来源未提供额外证据。</p>
          )}
        </div>
      </details>
    </li>
  );
}

export default function NodeInventoryPanel({
  active = true,
  focusRequest = 0,
  refreshRequest = 0,
  onReadState,
}: Props) {
  const { snapshot, loading, waiting, cancelling, error, refresh, cancel } = useInventory(active);
  useEffect(() => {
    onReadState?.({ snapshot, loading: loading || cancelling, error });
  }, [snapshot, loading, cancelling, error, onReadState]);
  const [planning, setPlanning] = useState<{
    installationId: string | null;
    initialAction?: "install" | "unbind";
  } | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const lastFocusRequest = useRef(0);
  const lastRefreshRequest = useRef(refreshRequest);
  const panelId = `node-inventory-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;

  useEffect(() => {
    if (!active || focusRequest <= 0 || lastFocusRequest.current === focusRequest) return;
    lastFocusRequest.current = focusRequest;
    heading.current?.focus({ preventScroll: true });
    heading.current?.scrollIntoView?.({ block: "start", behavior: "smooth" });
  }, [active, focusRequest]);

  useEffect(() => {
    // 接续扫描会包含最新变化；用户取消期间的外部刷新也不应重启扫描。
    if (waiting || cancelling) {
      lastRefreshRequest.current = refreshRequest;
      return;
    }
    if (!active || loading || lastRefreshRequest.current === refreshRequest) return;
    lastRefreshRequest.current = refreshRequest;
    void refresh();
  }, [active, loading, waiting, cancelling, refreshRequest, refresh]);

  const installations = snapshot?.installations.filter((item) => isNode(item.toolId)) ?? [];
  const reports = snapshot?.providers ?? [];
  const instanceIds = new Map(
    installations.map((item, index) => [item.installationId, `${panelId}-instance-${index}`]),
  );
  const failures = reports.filter((report) => report.error !== null);
  const pending = loading || cancelling;
  const complete =
    !error &&
    PROVIDERS.every(({ id }) =>
      completeNodeCoverage(reports.find((report) => report.providerId === id)),
    );

  return (
    <section className="node-inventory" aria-labelledby={`${panelId}-heading`}>
      <div className="node-inventory-head">
        <div className="node-inventory-heading">
          <span className="node-inventory-icon">
            <FileSearch size={21} aria-hidden="true" />
          </span>
          <div>
            <h2 id={`${panelId}-heading`} ref={heading} tabIndex={-1}>
              Node.js 本机清单
            </h2>
            <p>按来源保留文件证据，同版本的不同目录分别展示。</p>
          </div>
        </div>
        <div className="node-inventory-actions">
          <button
            className="btn node-inventory-button"
            disabled={!snapshot || loading || cancelling}
            onClick={() => setPlanning({ installationId: null })}
          >
            预览 Node 计划
          </button>
          <button
            type="button"
            className="btn node-inventory-button"
            disabled={!snapshot || pending}
            onClick={() => setPlanning({ installationId: null, initialAction: "unbind" })}
          >
            管理 Node 绑定
          </button>
          {pending && (
            <button
              type="button"
              className="btn node-inventory-button"
              disabled={cancelling}
              onClick={() => {
                // 明确取消时同时丢弃当前排队意图，避免结束后立即再次扫描。
                lastRefreshRequest.current = refreshRequest;
                void cancel();
              }}
            >
              <X size={14} aria-hidden="true" />
              {cancelling ? "取消中…" : "取消刷新"}
            </button>
          )}
          <button
            type="button"
            className="btn node-inventory-button"
            aria-label="刷新全部 Node.js 来源"
            disabled={pending}
            onClick={() => void refresh()}
          >
            <RefreshCw
              size={14}
              aria-hidden="true"
              className={pending ? "node-inventory-spin" : undefined}
            />
            刷新全部
          </button>
        </div>
      </div>

      <div className="node-inventory-content">
        <p className="node-inventory-scope">只读发现 · 当前为 macOS 上的 Node.js 试点</p>
        {pending && (
          <p className="node-inventory-progress" role="status">
            {cancelling
              ? "正在取消刷新，等待本次读取结束…"
              : waiting
                ? "正在等待上次读取结束，随后刷新清单…"
                : "正在读取各来源，已有结果会保留。"}
          </p>
        )}
        {(error || failures.length > 0) && (
          <div className="node-inventory-feedback" role="alert">
            <AlertTriangle size={16} aria-hidden="true" />
            <div>
              <strong>{error ? "本次清单查询未完成" : "部分来源未完成读取"}</strong>
              {error && (
                <p>
                  {error}
                  {snapshot ? "。当前展示上次读取的结果，尚未确认最新状态。" : "。可重试读取。"}
                </p>
              )}
              {failures.length > 0 && (
                <ul>
                  {failures.map((report) => (
                    <li key={report.providerId} id={`${panelId}-error-${report.providerId}`}>
                      {sourceName(report.providerId)}：{report.error?.message}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        )}

        <ul className="node-inventory-sources" aria-label="Node.js 来源状态">
          {PROVIDERS.map((provider) => {
            const report = reports.find((item) => item.providerId === provider.id);
            const count = installations.filter(
              (item) => item.providerPackage.providerId === provider.id,
            ).length;
            const stale = report?.stale || Boolean(error && snapshot);
            const absent = completeNodeCoverage(report) && !stale && count === 0;
            const sourceReadSucceeded = report?.status === "ok" && report.error === null;
            return (
              <li
                key={provider.id}
                className="node-inventory-source"
                aria-label={`${provider.name} 来源`}
              >
                <div className="node-inventory-source-top">
                  <h3>{provider.name}</h3>
                  <button
                    type="button"
                    className="node-inventory-source-refresh"
                    aria-label={`${sourceReadSucceeded ? "刷新" : "重试"} ${provider.name} 来源`}
                    disabled={pending}
                    onClick={() => void refresh(provider.id)}
                  >
                    <RefreshCw size={12} aria-hidden="true" />
                    {sourceReadSucceeded ? "刷新" : "重试"}
                  </button>
                </div>
                <p
                  className={`node-inventory-source-status${completeNodeCoverage(report) && !stale ? " node-inventory-source-ok" : ""}`}
                  aria-describedby={report?.error ? `${panelId}-error-${provider.id}` : undefined}
                >
                  {reportStatus(report)}
                </p>
                <p className="node-inventory-source-count">
                  {count > 0
                    ? `${count} 条来源记录`
                    : absent
                      ? "未发现 Node.js 记录"
                      : "安装情况未知"}
                </p>
                <p className="node-inventory-source-description">{provider.description}</p>
                <div className="node-inventory-source-flags">
                  {report?.coverage.truncated && <span>结果已截断</span>}
                  {report && !report.coverage.complete && <span>扫描范围未完整</span>}
                  {stale && <span>上次结果 · 待刷新</span>}
                  {report?.status === "unsupported" && <span>此发现能力目前仅支持 macOS</span>}
                </div>
                <p className="node-inventory-time">
                  {report ? (
                    <>
                      <span>读取于 </span>
                      <ReadTime timestamp={report.observedAt} />
                    </>
                  ) : (
                    "尚无读取时间"
                  )}
                </p>
              </li>
            );
          })}
        </ul>

        <div className="node-inventory-list-head">
          <h3>
            安装记录 <span>{installations.length}</span>
          </h3>
          {snapshot && (
            <p>
              最近快照：
              <ReadTime timestamp={snapshot.observedAt} />
            </p>
          )}
        </div>
        {installations.length > 0 ? (
          <ul className="node-inventory-instances" aria-label="Node.js 安装实例">
            {installations.map((installation) => (
              <InstanceRow
                key={installation.installationId}
                installation={installation}
                summaryId={instanceIds.get(installation.installationId)!}
                stale={
                  installation.stale ||
                  Boolean(error) ||
                  Boolean(
                    reports.find(
                      (report) => report.providerId === installation.providerPackage.providerId,
                    )?.stale,
                  )
                }
                aliases={
                  snapshot?.aliases.filter(
                    (alias) => alias.installationId === installation.installationId,
                  ) ?? []
                }
                installations={installations}
                instanceIds={instanceIds}
                onPlan={(installationId) => setPlanning({ installationId })}
              />
            ))}
          </ul>
        ) : (
          <p className="node-inventory-empty">
            {pending
              ? "正在读取 Node.js 安装记录…"
              : complete
                ? "当前来源范围内未发现 Node.js 安装记录。"
                : "尚无可展示的 Node.js 安装记录；部分来源情况未知，可按来源重试。"}
          </p>
        )}
        <p className="node-inventory-footnote">
          版本探针只说明该文件可执行，不代表终端或项目正在使用它。PATH 文件观测与来源归属分别记录。
        </p>
      </div>
      {planning && snapshot && (
        <Suspense fallback={<p role="status">正在加载操作计划…</p>}>
          <OperationPlanDialog
            snapshotRequestId={snapshot.requestId}
            installationId={planning.installationId}
            initialAction={planning.initialAction}
            onClose={() => setPlanning(null)}
            onFinished={() => void refresh()}
          />
        </Suspense>
      )}
    </section>
  );
}
