import { useEffect, useState } from "react";
import { toolCatalog, type EntrySupport, type ToolCatalog } from "../api/toolCatalog";
import { catalogEvidence, type CatalogReadState } from "../lib/catalogEvidence";
import "./ToolCatalogPanel.css";
const PROVIDERS = {
  mise: "mise",
  brew: "Homebrew",
  nvm: "nvm",
  pyenv: "pyenv",
  uv: "uv",
  path: "PATH",
};
const SUPPORT = { integrated: "入口已接入", notIntegrated: "尚未接入", unsupported: "当前不支持" };
function Capability({
  name,
  value,
  showReason = true,
}: {
  name: string;
  value: EntrySupport;
  showReason?: boolean;
}) {
  return (
    <li>
      <span>
        {name}：{SUPPORT[value.state]}
      </span>
      {showReason && <p>{value.reason}</p>}
    </li>
  );
}
interface Props {
  active: boolean;
  read: CatalogReadState;
  onShowNode: () => void;
  onShowPython?: () => void;
  pythonRead?: CatalogReadState;
  onShowCompatibility: () => void;
}
export default function ToolCatalogPanel({
  active,
  read,
  onShowNode,
  onShowPython,
  pythonRead,
  onShowCompatibility,
}: Props) {
  const [catalog, setCatalog] = useState<ToolCatalog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [revision, setRevision] = useState(0);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  useEffect(() => {
    if (!active) return;
    let stopped = false;
    setLoading(true);
    setError(null);
    void toolCatalog()
      .then((c) => {
        if (!stopped) setCatalog(c);
      })
      .catch((e) => {
        if (!stopped) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!stopped) setLoading(false);
      });
    return () => {
      stopped = true;
    };
  }, [active, revision]);
  const stale = loading || Boolean(error) || !active;
  const tools =
    catalog?.tools
      .map((tool) => ({
        tool,
        evidence: catalogEvidence(
          tool,
          tool.definition.toolId === "python"
            ? (pythonRead ?? { snapshot: null, loading: false, error: null })
            : read,
        ),
      }))
      .filter(({ tool, evidence }) => {
        const key = search.trim().toLowerCase();
        return (
          [
            tool.definition.displayName,
            tool.definition.toolId,
            ...tool.definition.aliases,
            ...tool.definition.executableNames,
          ].some((s) => s.toLowerCase().includes(key)) &&
          (filter === "all" || filter === evidence.state)
        );
      }) ?? [];
  return (
    <section className="tool-catalog" aria-label="统一工具目录" aria-busy={loading}>
      <div className="tool-catalog-heading">
        <div>
          <h2>工具目录</h2>
          <p>工具定义、机器实例和操作权限分别核对。</p>
        </div>
        <button
          className="btn"
          disabled={!active || loading}
          onClick={() => setRevision((n) => n + 1)}
        >
          重新读取目录
        </button>
      </div>
      {loading && <p role="status">正在读取工具目录…</p>}
      {error && <p role="alert">工具目录读取失败：{error}。旧目录仅供参考，请重新读取后继续。</p>}
      {catalog && (
        <>
          <p className="tool-catalog-note">{catalog.coverageNote}</p>
          <p>
            当前平台：{{ macos: "macOS", windows: "Windows", linux: "Linux" }[catalog.platform]}
            。接入状态只说明流程入口；具体操作仍须生成计划并核对目标。
          </p>
          <div className="tool-catalog-controls">
            <label>
              搜索工具
              <input
                className="input"
                type="search"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="名称、别名或命令名"
              />
            </label>
            <label>
              实例证据
              <select className="input" value={filter} onChange={(e) => setFilter(e.target.value)}>
                <option value="all">全部工具</option>
                <option value="observed">已观察到实例</option>
                <option value="notObserved">完整来源未观察到</option>
                <option value="unknown">状态未知</option>
              </select>
            </label>
          </div>
          <p role="status">
            显示 {tools.length} / {catalog.tools.length} 项工具定义
          </p>
          {!tools.length && (
            <p>
              没有匹配工具；这不表示系统未安装。
              <button
                className="btn"
                onClick={() => {
                  setSearch("");
                  setFilter("all");
                }}
              >
                清除筛选
              </button>
            </p>
          )}
          <ul className="tool-catalog-grid">
            {tools.map(({ tool, evidence }) => (
              <li key={tool.definition.toolId} className="tool-catalog-card">
                <h3>
                  {tool.definition.displayName}{" "}
                  <span>{tool.category === "runtime" ? "语言与运行时" : "命令行工具"}</span>
                </h3>
                <p>{tool.description}</p>
                <p>
                  工具身份：<code>{tool.definition.toolId}</code>
                </p>
                <p className="tool-catalog-evidence">
                  {stale
                    ? "旧目录 · 待重新读取"
                    : evidence.state === "observed"
                      ? `已观察到 ${evidence.count} 个实例`
                      : evidence.state === "notObserved"
                        ? "当前来源未观察到安装"
                        : "安装状态未知"}
                </p>
                <p>{evidence.reason}</p>
                <div className="tool-catalog-actions">
                  {tool.definition.toolId === "node" &&
                  tool.channels.some((c) => c.inventory.state === "integrated") ? (
                    <button className="btn" disabled={stale} onClick={onShowNode}>
                      查看 Node 本机实例与计划
                    </button>
                  ) : tool.definition.toolId === "python" &&
                    onShowPython &&
                    tool.channels.some((c) => c.inventory.state === "integrated") ? (
                    <button className="btn" disabled={stale} onClick={onShowPython}>
                      查看 Python 本机实例
                    </button>
                  ) : (
                    <button className="btn" disabled={stale} onClick={onShowCompatibility}>
                      查看兼容渠道与手动入口
                    </button>
                  )}
                  <a href={tool.documentationUrl} target="_blank" rel="noreferrer">
                    官方文档
                  </a>
                </div>
                <details>
                  <summary>{tool.definition.displayName} 接入范围</summary>
                  <p>
                    项目核对：{SUPPORT[tool.projectCheck.state]}。{tool.projectCheck.reason}
                  </p>
                  {tool.channels.map((c) => {
                    const commonReason = [
                      c.inventory,
                      c.installPlan,
                      c.uninstallPlan,
                      c.bindingPlan,
                    ].every((value) => value.reason === c.inventory.reason);
                    return (
                      <section
                        key={c.providerId}
                        aria-label={`${tool.definition.displayName} ${PROVIDERS[c.providerId]} 接入范围`}
                      >
                        <h4>{PROVIDERS[c.providerId]}</h4>
                        {c.packages.length ? (
                          <p>
                            已登记包身份（非版本可用列表）：
                            {c.packages.map((p) => (
                              <code key={p.packageId}>
                                {p.packageId} · {p.kind}{" "}
                              </code>
                            ))}
                          </p>
                        ) : (
                          <p>此渠道的包与版本尚未核对，不提供统一操作。</p>
                        )}
                        {commonReason && <p>{c.inventory.reason}</p>}
                        <ul className="tool-catalog-capabilities">
                          <Capability
                            name="来源读取"
                            value={c.inventory}
                            showReason={!commonReason}
                          />
                          <Capability
                            name="安装计划"
                            value={c.installPlan}
                            showReason={!commonReason}
                          />
                          <Capability
                            name="卸载计划"
                            value={c.uninstallPlan}
                            showReason={!commonReason}
                          />
                          <Capability
                            name="绑定计划"
                            value={c.bindingPlan}
                            showReason={!commonReason}
                          />
                        </ul>
                      </section>
                    );
                  })}
                </details>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
