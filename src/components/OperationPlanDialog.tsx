import { useEffect, useRef, useState } from "react";
import type { ProviderId, ProviderPackage } from "../api/inventory";
import { errorMessage, operationCancel } from "../api";
import {
  operationExecute,
  operationBindings,
  type BindingSummary,
  type BindingStrategy,
  type BindingScope,
  type BindingOptions,
  type OperationPlan,
  type PlanIntent,
  type PlanAction,
} from "../api/operationPlan";
import { operationPlanV2 } from "../api/operationPlanV2";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";
import "./OperationPlanDialog.css";
import { Notice } from "./ui/notice";

interface Props {
  toolId?: "node" | "python";
  snapshotRequestId: string;
  installationId: string | null;
  onClose: () => void;
  onFinished: () => void;
  initialAction?: "install" | "unbind" | "bind";
  initialBinding?: BindingOptions;
  initialBindingId?: string;
  initialProviderId?: ProviderId;
  initialPackageId?: string;
  openerElement?: HTMLElement | null;
}

function brewFormula(packageId: string, python: boolean): string | null {
  const formula = packageId.replace(/^homebrew\/core\//, "");
  return (python ? /^(?:python3?|python@3\.[1-9]\d?)$/ : /^(?:node|node@[1-9]\d*)$/).test(formula)
    ? formula
    : null;
}

function toolPackage(
  packageIdentity: ProviderPackage | undefined,
  python: boolean,
): packageIdentity is ProviderPackage {
  if (
    !packageIdentity ||
    !["macos", "windows", "linux"].includes(packageIdentity.platform) ||
    typeof packageIdentity.packageId !== "string"
  )
    return false;
  const { providerId, packageId, kind } = packageIdentity;
  if (providerId === "brew") return kind === "formula" && brewFormula(packageId, python) !== null;
  if (providerId === "path")
    return (
      kind === "executable" &&
      (python ? /^python(?:3(?:\.\d+)?)?$/.test(packageId) : ["node", "nodejs"].includes(packageId))
    );
  return (
    kind === "runtime" &&
    (python
      ? ["mise", "pyenv", "uv"].includes(providerId) && packageId === "python"
      : ["mise", "nvm"].includes(providerId) && ["node", "nodejs"].includes(packageId))
  );
}

function executablePackage(target: ProviderPackage, action: PlanAction, python: boolean): boolean {
  if (target.platform !== "macos") return false;
  if (python)
    return action === "bind" || action === "unbind"
      ? target.providerId === "mise"
      : ["mise", "brew", "pyenv", "uv"].includes(target.providerId);
  if (target.providerId === "brew") return true;
  if (target.packageId !== "node") return false;
  return (
    target.providerId === "mise" ||
    (target.providerId === "nvm" && ["verify", "bind", "unbind"].includes(action))
  );
}

function samePackage(left: ProviderPackage, right: ProviderPackage, python: boolean): boolean {
  return (
    left.providerId === right.providerId &&
    left.kind === right.kind &&
    left.platform === right.platform &&
    (left.providerId === "brew"
      ? brewFormula(left.packageId, python) !== null &&
        brewFormula(left.packageId, python) === brewFormula(right.packageId, python)
      : left.packageId === right.packageId)
  );
}

function visible(element: HTMLElement | null): element is HTMLElement {
  if (!element?.isConnected || element.matches(":disabled")) return false;
  for (let current: HTMLElement | null = element; current; current = current.parentElement) {
    const style = getComputedStyle(current);
    if (
      current.hidden ||
      current.hasAttribute("inert") ||
      style.display === "none" ||
      style.visibility === "hidden" ||
      style.visibility === "collapse"
    )
      return false;
    if (
      current instanceof HTMLDetailsElement &&
      !current.open &&
      !current.querySelector(":scope > summary")?.contains(element)
    )
      return false;
  }
  return true;
}
const ACTIONS: { value: PlanAction; label: string }[] = [
  { value: "verify", label: "重新验证此实例" },
  { value: "uninstall", label: "卸载此实例" },
  { value: "bind", label: "绑定此实例" },
  { value: "unbind", label: "解除已有绑定" },
];
const EXECUTE_LABELS = {
  verify: "执行重新验证",
  install: "执行安装",
  uninstall: "执行卸载",
  bind: "执行绑定",
  unbind: "执行解除绑定",
};
export default function OperationPlanDialog({
  toolId = "node",
  snapshotRequestId,
  installationId,
  onClose,
  onFinished,
  initialAction,
  initialBinding,
  initialBindingId,
  initialProviderId,
  initialPackageId,
  openerElement,
}: Props) {
  const python = toolId === "python";
  const displayName = python ? "Python" : "Node.js";
  const packageName = python ? "python" : "node";
  const [action, setAction] = useState<PlanAction>(
    initialAction ?? (installationId ? "verify" : "install"),
  );
  const [strategy, setStrategy] = useState<BindingStrategy>(initialBinding?.strategy ?? "direct");
  const [scope, setScope] = useState<BindingScope>(initialBinding?.scope ?? "registration");
  const [projectPath, setProjectPath] = useState(initialBinding?.projectPath ?? "");
  const [bindings, setBindings] = useState<BindingSummary[]>([]);
  const [bindingId, setBindingId] = useState(initialBindingId ?? "");
  const [bindingsLoading, setBindingsLoading] = useState(false);
  const [provider, setProvider] = useState<ProviderId>(initialProviderId ?? "mise");
  const [packageId, setPackageId] = useState(initialPackageId ?? packageName);
  const [version, setVersion] = useState("");
  const [plan, setPlan] = useState<OperationPlan | null>(null);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [loading, setLoading] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [clock, setClock] = useState(Date.now());
  useEffect(() => {
    if (python && action === "bind") {
      if (scope === "registration") setScope("project");
      if (strategy !== "direct") setStrategy("direct");
    }
  }, [python, action, scope, strategy]);
  const sequence = useRef(0);
  const alive = useRef(true);
  const inFlight = useRef(false);
  const previewing = useRef(false);
  const cancelPending = useRef(false);
  const executingPlan = useRef<string | null>(null);
  const opener = useRef(
    openerElement ??
      (document.activeElement instanceof HTMLElement ? document.activeElement : null),
  );
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      sequence.current += 1;
    };
  }, []);
  useEffect(() => {
    const timer = setInterval(() => setClock(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    sequence.current += 1;
    previewing.current = false;
    setLoading(false);
    if (!inFlight.current) setPlan(null);
    setError("");
    setMessage("");
  }, [action, provider, packageId, version, installationId, strategy, scope, projectPath, toolId]);
  useEffect(() => {
    if (action !== "unbind") return;
    if (python && initialBindingId) {
      setBindings([]);
      setBindingId(initialBindingId);
      setBindingsLoading(false);
      return;
    }
    let stopped = false;
    setBindingsLoading(true);
    setBindings([]);
    setBindingId("");
    void operationBindings()
      .then((items) => {
        if (!stopped) {
          setBindings(items);
          setBindingId(items[0]?.bindingId ?? "");
        }
      })
      .catch((error: unknown) => {
        if (!stopped) setError(errorMessage(error));
      })
      .finally(() => {
        if (!stopped) setBindingsLoading(false);
      });
    return () => {
      stopped = true;
    };
  }, [action, snapshotRequestId, python, initialBindingId]);
  useEffect(() => {
    sequence.current += 1;
    previewing.current = false;
    setLoading(false);
    if (!inFlight.current) setPlan(null);
  }, [snapshotRequestId]);

  async function preview() {
    if (inFlight.current || previewing.current) return;
    previewing.current = true;
    const intent: PlanIntent =
      action === "unbind"
        ? { action, bindingId }
        : installationId && action !== "install"
          ? action === "bind"
            ? {
                action,
                installationId,
                binding: {
                  strategy,
                  scope,
                  projectPath: scope === "project" ? projectPath.trim() : null,
                },
              }
            : { action, installationId }
          : {
              action: "install",
              providerId: provider,
              packageId: provider === "brew" ? packageId.trim() : packageName,
              version: provider === "brew" ? null : version.trim(),
            };
    const attempt = ++sequence.current;
    setLoading(true);
    setPlan(null);
    setError("");
    setMessage("");
    try {
      const request = { snapshotRequestId, intent };
      const response = await operationPlanV2({
        schemaVersion: 2,
        subject: { kind: "tool", toolId },
        request,
      });
      const next = response.plan;
      if (alive.current && attempt === sequence.current) {
        const target = next.target?.providerPackage;
        const binding = bindings.find((item) => item.bindingId === bindingId);
        const installPackage =
          intent.action === "install"
            ? {
                providerId: intent.providerId,
                packageId: intent.packageId,
                kind: intent.providerId === "brew" ? ("formula" as const) : ("runtime" as const),
                // 请求未指定平台；保留后端只读目标的平台，执行能力另按 macOS 边界核对。
                platform: target?.platform ?? ("macos" as const),
              }
            : null;
        if (
          response.schemaVersion !== 2 ||
          response.subject.kind !== "tool" ||
          response.subject.toolId !== toolId ||
          (next.toolId ?? "node") !== toolId ||
          !toolPackage(target, python) ||
          (next.capability.canExecute && !executablePackage(target, intent.action, python)) ||
          (intent.action === "install" &&
            (!installPackage ||
              !samePackage(target, installPackage, python) ||
              next.target.installationId !== null ||
              next.target.locator !== null)) ||
          next.snapshotRequestId !== snapshotRequestId ||
          next.action !== intent.action ||
          (intent.action !== "install" &&
            intent.action !== "unbind" &&
            next.target.installationId !== intent.installationId) ||
          (intent.action === "unbind" &&
            (python
              ? next.target.installationId !== installationId || target.providerId !== "mise"
              : !binding ||
                !samePackage(target, binding.providerPackage, python) ||
                next.target.locator !== binding.locator))
        )
          throw new Error("计划与当前请求不匹配，请重新预览");
        setPlan(next);
        setClock(Date.now());
      }
    } catch (error) {
      if (alive.current && attempt === sequence.current) setError(errorMessage(error));
    } finally {
      if (alive.current && attempt === sequence.current) {
        previewing.current = false;
        setLoading(false);
      }
    }
  }
  async function execute() {
    if (
      inFlight.current ||
      !plan?.capability.canExecute ||
      plan.snapshotRequestId !== snapshotRequestId ||
      Date.now() >= plan.expiresAt
    )
      return;
    inFlight.current = true;
    executingPlan.current = plan.planId;
    setExecuting(true);
    setError("");
    setMessage("");
    try {
      const result = await operationExecute(plan.planId);
      if (
        (result.toolId ?? "node") !== toolId ||
        result.id !== plan.planId ||
        result.action !== plan.action
      )
        throw new Error("执行结果与当前工具计划不匹配，请查看任务记录并重新核对来源");
      if (alive.current)
        setMessage(
          result.status === "completed" && result.outcome === "bound"
            ? "绑定资源与受控版本验证通过；注册、配置选择已按计划处理，用户终端和 IDE 的实际生效尚未确认。"
            : result.status === "completed" && result.outcome === "unbound"
              ? "本绑定已解除；原安装仍由原来源管理，请重新确认项目、终端和 IDE 的实际版本。"
              : result.status === "completed" && result.outcome === "installed"
                ? python
                  ? "安装已通过来源重扫与版本验证；未改写版本请求，mise 别名或 shims 的更新可能影响现有解析，项目与终端请另行核对。"
                  : "安装已通过来源重扫与版本验证；项目与终端不会自动切换，请分别确认实际使用版本。"
                : result.status === "completed" && result.outcome === "uninstalled"
                  ? "已确认所选实例移除；引用此来源的项目和终端配置未改写，可能需要重新选择版本。"
                  : result.status === "completed" && result.outcome === "verified"
                    ? "原实例已重新验证；项目与终端生效状态仍需分别确认。"
                    : "本次操作未确认完成，请查看任务步骤与刷新后的来源记录。",
        );
    } catch (error) {
      if (alive.current) setError(errorMessage(error));
    } finally {
      inFlight.current = false;
      executingPlan.current = null;
      cancelPending.current = false;
      if (alive.current) {
        setExecuting(false);
        setCancelling(false);
        setPlan(null);
      }
      onFinished();
    }
  }
  async function cancel() {
    const planId = executingPlan.current;
    if (!planId || cancelPending.current) return;
    cancelPending.current = true;
    setCancelling(true);
    try {
      await operationCancel(planId);
    } catch (error) {
      if (alive.current && executingPlan.current === planId) {
        cancelPending.current = false;
        setError(errorMessage(error));
        setCancelling(false);
      }
    }
  }
  const expired = plan !== null && clock >= plan.expiresAt;
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent
        className="operation-plan-dialog"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (visible(opener.current)) {
            opener.current.focus();
            if (document.activeElement === opener.current) return;
          }
          for (const fallback of document.querySelectorAll<HTMLElement>(
            `[data-runtime-switch="${toolId}"]`,
          )) {
            if (!visible(fallback)) continue;
            fallback.focus();
            if (document.activeElement === fallback) return;
          }
        }}
      >
        <header>
          <DialogTitle>{displayName} 操作计划</DialogTitle>
          <DialogDescription>
            {python
              ? "重新验证只确认所选实例。mise、uv、pyenv 精确安装与 Homebrew core formula 安装/卸载按计划核验；mise 绑定可写入项目或全局选择，实际生效需另行核对。"
              : "先核对目标、配置差异与来源影响，再执行计划。失败或取消后的补偿单独记录。"}
          </DialogDescription>
        </header>
        <div className="operation-plan-scroll">
          {installationId ? (
            <label>
              操作
              <select
                aria-label="计划操作"
                value={action}
                disabled={executing}
                onChange={(e) => setAction(e.target.value as PlanAction)}
              >
                {ACTIONS.filter(
                  (a) =>
                    !python ||
                    a.value === "verify" ||
                    a.value === "uninstall" ||
                    a.value === "bind" ||
                    (a.value === "unbind" && Boolean(initialBindingId)),
                ).map((a) => (
                  <option key={a.value} value={a.value}>
                    {a.label}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <label>
              操作
              <select
                aria-label="计划操作"
                value={action}
                disabled={executing}
                onChange={(e) => setAction(e.target.value as PlanAction)}
              >
                <option value="install">安装 {displayName}</option>
                {!python && <option value="unbind">解除已有绑定</option>}
              </select>
            </label>
          )}
          {action === "unbind" && (
            <>
              {!python && (
                <label>
                  已有绑定
                  <select
                    aria-label="已有 Node 绑定"
                    value={bindingId}
                    disabled={executing || bindingsLoading}
                    onChange={(e) => {
                      setBindingId(e.target.value);
                      sequence.current += 1;
                      previewing.current = false;
                      setLoading(false);
                      setPlan(null);
                      setError("");
                      setMessage("");
                    }}
                  >
                    {bindings.map((b) => (
                      <option key={b.bindingId} value={b.bindingId}>
                        {b.providerPackage.providerId} · {b.version} · {b.strategy} / {b.scope} ·{" "}
                        {b.state === "bound" ? "已确认" : "中断或待确认"}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              {python && initialBindingId && (
                <p>将解除本应用写入的绑定并恢复原配置；外部修改会停止。</p>
              )}
              {!python &&
                (bindingsLoading ? (
                  <p role="status">正在读取绑定记录…</p>
                ) : bindings.length === 0 ? (
                  <p>没有可解除的 Node 绑定记录。</p>
                ) : (
                  <p>
                    原安装：<code>{bindings.find((b) => b.bindingId === bindingId)?.locator}</code>
                  </p>
                ))}
            </>
          )}
          {action === "bind" && (
            <>
              <label>
                绑定方式
                <select
                  aria-label="绑定方式"
                  value={strategy}
                  disabled={executing}
                  onChange={(e) => {
                    const value = e.target.value as BindingStrategy;
                    setStrategy(value);
                    if (value === "projectPath") setScope("project");
                  }}
                >
                  <option value="direct">直连原生 mise / nvm 精确实例</option>
                  {!python && <option value="managed">Homebrew 托管链接</option>}
                  {!python && <option value="projectPath">仅项目 PATH</option>}
                </select>
              </label>
              <label>
                作用范围
                <select
                  aria-label="绑定作用范围"
                  value={scope}
                  disabled={executing || strategy === "projectPath"}
                  onChange={(e) => setScope(e.target.value as BindingScope)}
                >
                  {!python && <option value="registration">仅注册到 mise</option>}
                  <option value="global">注册并写入全局选择</option>
                  <option value="project">注册并写入项目选择</option>
                </select>
              </label>
              {scope === "project" && (
                <label>
                  项目目录或配置文件
                  <input
                    aria-label="绑定项目路径"
                    value={projectPath}
                    disabled={executing}
                    placeholder="项目绝对路径或 mise.toml"
                    onChange={(e) => setProjectPath(e.target.value)}
                  />
                </label>
              )}
              <small>注册、配置选择和受控验证分别确认；不会修改 shell 配置或原安装所有权。</small>
            </>
          )}
          {action === "install" && (
            <>
              <label>
                安装来源
                <select
                  aria-label="安装来源"
                  value={provider}
                  disabled={executing}
                  onChange={(e) => {
                    setProvider(e.target.value as ProviderId);
                    setPackageId(packageName);
                  }}
                >
                  <option value="mise">mise</option>
                  <option value="brew">Homebrew</option>
                  {python && <option value="pyenv">pyenv</option>}
                  {python && <option value="uv">uv</option>}
                </select>
              </label>
              {provider !== "brew" ? (
                <label>
                  精确版本
                  <input
                    aria-label={`${python ? "Python" : "Node"} 精确版本`}
                    placeholder={python ? "例如 3.13.15" : "例如 22.1.0"}
                    value={version}
                    disabled={executing}
                    onChange={(e) => setVersion(e.target.value)}
                  />
                </label>
              ) : (
                <label>
                  原生 formula
                  <input
                    aria-label={`${displayName === "Node.js" ? "Node" : displayName} formula`}
                    value={packageId}
                    disabled={executing}
                    onChange={(e) => setPackageId(e.target.value)}
                  />
                  <small>
                    {python
                      ? "例如 python@3.13；仅预览 core formula，不能保证精确补丁版。"
                      : "例如 node 或 node@22；安装执行时的公式版本，无法保证精确补丁版。"}
                  </small>
                </label>
              )}
            </>
          )}
          <button
            className="btn"
            disabled={
              loading ||
              executing ||
              (action === "install" &&
                provider !== "brew" &&
                !(python ? /^3\.(?:[89]|[1-9]\d)\.(?:0|[1-9]\d*)$/ : /^\d+\.\d+\.\d+$/).test(
                  version.trim(),
                )) ||
              (action === "bind" && scope === "project" && !projectPath.trim()) ||
              (action === "unbind" && (bindingsLoading || !bindingId))
            }
            onClick={() => void preview()}
          >
            {loading ? "生成计划中…" : "生成预览"}
          </button>
          {error && <Notice tone="error">{error}</Notice>}
          {message && <p role="status">{message}</p>}
          {plan && (
            <section aria-label="后端操作计划">
              {plan.capability.reason && (
                <p className="operation-plan-warning" role="status">
                  {plan.capability.reason}
                </p>
              )}
              <dl>
                <div>
                  <dt>目标来源与包</dt>
                  <dd>
                    {plan.target.providerPackage.providerId} ·{" "}
                    <code>{plan.target.providerPackage.packageId}</code>
                  </dd>
                </div>
                {plan.target.locator && (
                  <div>
                    <dt>目标目录</dt>
                    <dd>
                      <code>{plan.target.locator}</code>
                    </dd>
                  </div>
                )}
                <div>
                  <dt>版本承诺</dt>
                  <dd>{plan.versionPromise}</dd>
                </div>
                <div>
                  <dt>配置影响</dt>
                  <dd>{plan.configurationChange}</dd>
                </div>
                <div>
                  <dt>配置原文指纹</dt>
                  <dd>
                    <code>{plan.configuration.path}</code>
                    <br />
                    {plan.configuration.exists ? (
                      <code>SHA-256 {plan.configuration.sha256}</code>
                    ) : (
                      "文件尚不存在"
                    )}
                  </dd>
                </div>
                <div>
                  <dt>权限</dt>
                  <dd>{plan.permission}</dd>
                </div>
                <div>
                  <dt>恢复边界</dt>
                  <dd>{plan.rollback}</dd>
                </div>
                <div>
                  <dt>有效期</dt>
                  <dd>
                    {expired
                      ? "已过期，请重新生成预览"
                      : `${Math.max(0, Math.ceil((plan.expiresAt - clock) / 1000))} 秒；清单、目标或配置变化也会使其失效`}
                  </dd>
                </div>
              </dl>
              {(plan.impact ?? []).length > 0 && (
                <section aria-label="操作影响">
                  <h3>操作影响</h3>
                  <ul>
                    {plan.impact.map((item, index) => (
                      <li key={index}>{item}</li>
                    ))}
                  </ul>
                </section>
              )}
              {(plan.configurationDiff ?? []).length > 0 && (
                <section aria-label="配置差异">
                  <h3>配置差异</h3>
                  {plan.configurationDiff!.map((diff, index) => (
                    <div key={index}>
                      <p>
                        <code>{diff.path}</code> · <code>{diff.field}</code>
                      </p>
                      <p>
                        修改前：<code>{diff.before ?? "未声明"}</code>
                      </p>
                      <p>
                        修改后：<code>{diff.after ?? "移除声明"}</code>
                      </p>
                    </div>
                  ))}
                </section>
              )}
              <ol>
                {plan.steps.map((step, index) => (
                  <li key={index}>
                    <strong>{step.title}</strong>
                    {step.program && (
                      <>
                        <p>
                          程序：<code>{step.program}</code>
                        </p>
                        <p>
                          参数：<code>{JSON.stringify(step.argv)}</code>
                        </p>
                      </>
                    )}
                    <p>
                      工作目录：<code>{step.cwd}</code>
                    </p>
                    <p>允许的环境键：{step.environmentKeys.join("、")}</p>
                  </li>
                ))}
              </ol>
              {plan.capability.canExecute &&
                !expired &&
                (executing || plan.snapshotRequestId === snapshotRequestId) && (
                  <button className="btn" disabled={executing} onClick={() => void execute()}>
                    {executing ? "执行中…" : EXECUTE_LABELS[plan.action]}
                  </button>
                )}
              {!plan.capability.canExecute && <p>此计划当前不能执行；现有管理入口继续保留。</p>}
            </section>
          )}
          {executing && (
            <button className="btn" disabled={cancelling} onClick={() => void cancel()}>
              {cancelling ? "等待取消完成…" : "请求取消"}
            </button>
          )}
        </div>
        <footer>
          <button className="btn-ghost" onClick={onClose}>
            关闭
          </button>
          {executing && <span>关闭后任务继续，可从全局任务面板查看与取消。</span>}
        </footer>
      </DialogContent>
    </Dialog>
  );
}
