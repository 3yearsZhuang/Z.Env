import { lazy, Suspense, useEffect, useId, useRef, useState } from "react";
import { pythonBindings, type PythonBinding } from "../api/pythonProject";
import { useInventory } from "../lib/useInventory";
import type { CatalogReadState } from "../lib/catalogEvidence";
import type { ProviderReport, ProviderId } from "../api/inventory";
import "./PythonInventoryPanel.css";
const OperationPlanDialog = lazy(() => import("./OperationPlanDialog"));
const SOURCES: { id: ProviderId; name: string }[] = [
  { id: "mise", name: "mise" },
  { id: "brew", name: "Homebrew" },
  { id: "pyenv", name: "pyenv" },
  { id: "uv", name: "uv" },
  { id: "path", name: "PATH" },
];
const HEALTH = { healthy: "已验证 CPython", unverified: "未验证", broken: "异常" };
const OWNERSHIP = { owned: "已确认归属", observed: "仅观测", unknown: "归属未知" };
function complete(p?: ProviderReport) {
  return Boolean(
    p &&
    p.status === "ok" &&
    !p.error &&
    !p.stale &&
    p.coverage.complete &&
    !p.coverage.truncated &&
    p.coverage.toolIds.includes("python"),
  );
}
interface Props {
  active: boolean;
  focusRequest: number;
  refreshRequest: number;
  onReadState: (state: CatalogReadState) => void;
}
export default function PythonInventoryPanel({
  active,
  focusRequest,
  refreshRequest,
  onReadState,
}: Props) {
  const state = useInventory(active, "python");
  const [planning, setPlanning] = useState<{
    installationId: string | null;
    action?: "verify" | "bind" | "unbind";
    bindingId?: string;
  } | null>(null);
  const [bindings, setBindings] = useState<PythonBinding[]>([]);
  useEffect(() => {
    if (!active) setPlanning(null);
  }, [active]);
  const heading = useRef<HTMLHeadingElement>(null);
  const focused = useRef(0),
    refreshed = useRef(refreshRequest);
  const panelId = useId();
  useEffect(() => {
    onReadState({
      snapshot: state.snapshot,
      loading: state.loading || state.cancelling,
      error: state.error,
    });
  }, [state.snapshot, state.loading, state.cancelling, state.error, onReadState]);
  useEffect(() => {
    if (active && focusRequest > focused.current) {
      focused.current = focusRequest;
      heading.current?.focus({ preventScroll: true });
      heading.current?.scrollIntoView?.({ block: "start" });
    }
  }, [active, focusRequest]);
  useEffect(() => {
    if (state.waiting || state.cancelling) {
      refreshed.current = refreshRequest;
      return;
    }
    if (active && !state.loading && refreshed.current !== refreshRequest) {
      refreshed.current = refreshRequest;
      void state.refresh();
    }
  }, [active, refreshRequest, state.loading, state.waiting, state.cancelling, state.refresh]);
  useEffect(() => {
    if (!active || !state.snapshot) return;
    let stopped = false;
    void pythonBindings()
      .then((items) => {
        if (!stopped) setBindings(items);
      })
      .catch(() => {
        if (!stopped) setBindings([]);
      });
    return () => {
      stopped = true;
    };
  }, [active, state.snapshot]);
  const records = state.snapshot?.installations.filter((i) => i.toolId === "python") ?? [];
  const providers = state.snapshot?.providers ?? [];
  const stale = !active || state.loading || Boolean(state.error);
  const allComplete =
    !stale && SOURCES.every((s) => complete(providers.find((p) => p.providerId === s.id)));
  return (
    <section
      className="python-inventory"
      aria-labelledby={`${panelId}-heading`}
      aria-busy={state.loading}
    >
      <div className="python-inventory-heading">
        <div>
          <h2 ref={heading} tabIndex={-1} id={`${panelId}-heading`}>
            Python 本机清单
          </h2>
          <p>macOS CPython 3 来源样板；项目虚拟环境、其他实现及特殊构建不作为已验证安装。</p>
        </div>
        <div className="python-inventory-actions">
          <button
            className="btn"
            disabled={!active || state.loading || Boolean(planning)}
            onClick={() => void state.refresh()}
          >
            刷新 Python 来源
          </button>
          <button
            className="btn"
            disabled={!active || state.loading || Boolean(state.error) || !state.snapshot}
            onClick={() => setPlanning({ installationId: null })}
          >
            预览 Python 安装计划
          </button>
          {state.loading && (
            <button className="btn" disabled={state.cancelling} onClick={() => void state.cancel()}>
              {state.cancelling ? "Python 取消中…" : "取消 Python 刷新"}
            </button>
          )}
        </div>
      </div>
      {state.loading && (
        <p role="status">
          {state.waiting ? "等待旧读取结束后扫描 Python…" : "正在读取 Python 来源…"}
        </p>
      )}
      {state.error && (
        <p role="alert">Python 来源读取失败：{state.error}。保留的旧记录不能证明当前状态。</p>
      )}
      {state.snapshot && (
        <p className="python-inventory-note">
          读取时间：{new Date(state.snapshot.observedAt).toLocaleString("zh-CN")}
          ，结果是读取时点证据，外部环境变化后需刷新。
        </p>
      )}
      <p className="python-inventory-note">
        页面读取不调用安装器或修复；mise 与 uv 托管安装/卸载以计划中的能力、归属和影响为准；
        其他来源安装仅预览，绑定和项目解析尚未开放。
      </p>
      <div className="python-source-grid">
        {SOURCES.map((source) => {
          const report = providers.find((p) => p.providerId === source.id);
          return (
            <section key={source.id} aria-label={`${source.name} Python 来源`}>
              <h3>{source.name}</h3>
              <p>
                {stale
                  ? "待重新核对"
                  : complete(report)
                    ? "读取完成"
                    : report?.status === "unsupported"
                      ? "当前范围不支持"
                      : "来源未知或读取未完整"}
              </p>
              {report?.error && <p>{report.error.message}</p>}
              {report?.stale && <p>含上次记录，已标过期</p>}
              {report?.coverage.truncated && <p>达到预算，结果已截断</p>}
              <button
                className="btn"
                disabled={!active || state.loading || Boolean(planning)}
                onClick={() => void state.refresh(source.id)}
              >
                重试 {source.name} Python 来源
              </button>
            </section>
          );
        })}
      </div>
      <p role="status">
        {records.length} 条来源记录 ·{" "}
        {stale
          ? "当前状态待核对"
          : `${records.filter((i) => !i.stale && i.health === "healthy").length} 条通过隔离探针`}
      </p>
      {records.length === 0 && !state.loading && (
        <p>
          {allComplete
            ? "本次受支持来源中未观察到 Python 安装，不代表系统中绝对不存在。"
            : "尚无 Python 安装记录；来源未完整确认，不能判定未安装。"}
        </p>
      )}
      <ul className="python-instance-list">
        {records.map((item) => (
          <li key={item.installationId}>
            <details>
              <summary>
                <strong>
                  {item.providerPackage.providerId} · {item.version ?? "版本未知"}
                </strong>
                <span>
                  {stale || item.stale ? "上次记录 · 待核对" : HEALTH[item.health]} ·{" "}
                  {OWNERSHIP[item.ownership]}
                </span>
                <code>{item.locator}</code>
              </summary>
              <dl>
                <dt>原生包</dt>
                <dd>
                  {item.providerPackage.packageId} · {item.providerPackage.kind}
                </dd>
                <dt>架构</dt>
                <dd>{item.architecture ?? "未知"}</dd>
                <dt>文件</dt>
                <dd>
                  <code>{item.executablePath ?? "未提供"}</code>
                </dd>
                <dt>真实路径</dt>
                <dd>
                  <code>{item.canonicalExecutablePath ?? "未知"}</code>
                </dd>
              </dl>
              <p>声明目录与实际版本分别核对；来源健康不代表项目或终端正在使用。</p>
              <button
                className="btn"
                disabled={
                  !active ||
                  state.loading ||
                  Boolean(state.error) ||
                  item.stale ||
                  item.health !== "healthy" ||
                  item.providerPackage.providerId !== "mise"
                }
                title={
                  item.providerPackage.providerId !== "mise"
                    ? "Python 绑定当前只支持 mise 管理的实例"
                    : undefined
                }
                onClick={() => setPlanning({ installationId: item.installationId, action: "bind" })}
              >
                绑定此实例到项目
              </button>
              <button
                className="btn"
                disabled={
                  !active ||
                  state.loading ||
                  Boolean(state.error) ||
                  item.stale ||
                  item.health !== "healthy" ||
                  item.providerPackage.providerId === "path"
                }
                onClick={() => setPlanning({ installationId: item.installationId })}
              >
                预览重新验证此 Python 实例
              </button>
              <h4>观测证据</h4>
              <ul>
                {item.evidence.map((e, index) => (
                  <li key={index}>
                    <code>{e.kind}</code>
                    <p>{e.path}</p>
                    <p>{e.detail ?? "未提供说明"}</p>
                  </li>
                ))}
              </ul>
              {state.snapshot?.aliases
                .filter((a) => a.installationId === item.installationId)
                .map((a, index) => (
                  <p key={index}>
                    同文件关联：<code>{a.evidence.canonicalExecutablePath}</code>
                    。来源身份和归属各自保留。
                  </p>
                ))}
            </details>
          </li>
        ))}
      </ul>
      {bindings.length > 0 && (
        <section className="python-bindings" aria-label="Python 项目绑定">
          <h3>已有 Python 绑定</h3>
          <ul>
            {bindings.map((binding) => (
              <li key={binding.bindingId}>
                <p>
                  <code>{binding.version}</code> · {binding.scope === "global" ? "全局" : "项目"} ·{" "}
                  <code>{binding.configPath}</code>
                </p>
                <button
                  className="btn"
                  onClick={() =>
                    setPlanning({
                      installationId: binding.installationId,
                      action: "unbind",
                      bindingId: binding.bindingId,
                    })
                  }
                >
                  解除绑定
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {planning && state.snapshot && (
        <Suspense fallback={<p role="status">正在加载 Python 操作计划…</p>}>
          <OperationPlanDialog
            toolId="python"
            snapshotRequestId={state.snapshot.requestId}
            installationId={planning.installationId}
            initialAction={
              planning.action === "bind"
                ? "bind"
                : planning.action === "unbind"
                  ? "unbind"
                  : undefined
            }
            initialBinding={
              planning.action === "bind"
                ? { strategy: "direct", scope: "project", projectPath: "" }
                : undefined
            }
            initialBindingId={planning.bindingId}
            onClose={() => setPlanning(null)}
            onFinished={() => {
              if (active) void state.refresh();
            }}
          />
        </Suspense>
      )}
    </section>
  );
}
