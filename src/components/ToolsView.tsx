import { useCallback, useEffect, useState } from "react";
import {
  listTools,
  useVersion,
  uninstallVersion,
  detectToolSources,
  ToolSource,
  errorMessage,
  ToolInfo,
} from "../api";
import InstallDialog from "./InstallDialog";

/** 常见运行时对应的官方 SVG 图标（存放于 public/tools/） */
const TOOL_ICON_PATHS: Record<string, string> = {
  node: "/tools/nodejs.svg",
  nodejs: "/tools/nodejs.svg",
  npm: "/tools/npm.svg",
  python: "/tools/python.svg",
  java: "/tools/java.svg",
  go: "/tools/go.svg",
  golang: "/tools/go.svg",
  rust: "/tools/rust.svg",
  cargo: "/tools/rust.svg",
  ruby: "/tools/ruby.svg",
  bun: "/tools/bun.svg",
  deno: "/tools/deno.svg",
  kotlin: "/tools/kotlin.svg",
  dotnet: "/tools/dotnet.svg",
  php: "/tools/php.svg",
  elixir: "/tools/elixir.svg",
  erlang: "/tools/erlang.svg",
  haskell: "/tools/haskell.svg",
  lua: "/tools/lua.svg",
  perl: "/tools/perl.svg",
  swift: "/tools/swift.svg",
  flutter: "/tools/flutter.svg",
  dart: "/tools/dart.svg",
  zig: "/tools/zig.svg",
  terraform: "/tools/terraform.svg",
  typescript: "/tools/typescript.svg",
  ts: "/tools/typescript.svg",
  javascript: "/tools/javascript.svg",
  js: "/tools/javascript.svg",
  c: "/tools/c.svg",
  cpp: "/tools/cpp.svg",
  cplusplus: "/tools/cpp.svg",
  csharp: "/tools/csharp.svg",
  cs: "/tools/csharp.svg",
  css: "/tools/css.svg",
  html: "/tools/html.svg",
  bash: "/tools/bash.svg",
  sh: "/tools/bash.svg",
  shell: "/tools/bash.svg",
  zsh: "/tools/bash.svg",
  powershell: "/tools/powershell.svg",
  pwsh: "/tools/powershell.svg",
  r: "/tools/r.svg",
  scala: "/tools/scala.svg",
  julia: "/tools/julia.svg",
  clojure: "/tools/clojure.svg",
  nim: "/tools/nim.svg",
  crystal: "/tools/crystal.svg",
  ocaml: "/tools/ocaml.svg",
  gcc: "/tools/gcc.svg",
  clang: "/tools/clang.svg",
  pnpm: "/tools/pnpm.svg",
  yarn: "/tools/yarn.svg",
  uv: "/tools/uv.svg",
  composer: "/tools/composer.svg",
  helm: "/tools/helm.svg",
  markdown: "/tools/markdown.svg",
  md: "/tools/markdown.svg",
  json: "/tools/json.svg",
  cmake: "/tools/cmake.svg",
  gradle: "/tools/gradle.svg",
  maven: "/tools/maven.svg",
  svelte: "/tools/svelte.svg",
  vue: "/tools/vue.svg",
  react: "/tools/react.svg",
};

/** 工具徽标：先走显式映射，再按 /tools/{name}.svg 通用查找，都没有则回退名称缩写 */
function ToolBadge({ name }: { name: string }) {
  const [failed, setFailed] = useState(false);
  const key = name.toLowerCase();
  // 支持形如 "nodejs"、"node.js" 的别名
  const explicit =
    TOOL_ICON_PATHS[key] ||
    TOOL_ICON_PATHS[key.replace(/[\W_]+/g, "")] ||
    null;
  const src = !failed && (explicit || `/tools/${key}.svg`);
  if (src && !failed) {
    return (
      <span className="tool-badge icon">
        <img
          src={src}
          alt={name}
          className="tool-badge-img"
          draggable={false}
          onError={() => setFailed(true)}
        />
      </span>
    );
  }
  return <span className="tool-badge">{name.slice(0, 2).toUpperCase()}</span>;
}

/** 常用支持的工具候选（用于展示未安装项）；同时供设置页自检覆盖使用 */
export const KNOWN_TOOLS = [
  // 语言 / 运行时
  "node", "python", "ruby", "go", "rust", "java", "kotlin", "dotnet",
  "php", "swift", "dart", "flutter", "haskell", "lua", "perl", "zig",
  "nim", "crystal", "scala", "julia", "clojure", "c", "cpp", "csharp",
  "r", "v", "ocaml", "typescript", "javascript", "d", "elm", "groovy",
  "haxe", "lisp", "objectivec", "prolog", "racket", "reason", "solidity",
  "pascal", "ada", "applescript", "fortran", "coq", "clojurescript",
  "coffeescript",
  // JS 生态
  "npm", "bun", "deno", "pnpm", "yarn", "nodejs",
  // 构建 / 编译
  "cmake", "gradle", "maven", "make", "ninja", "gcc", "clang", "protoc",
  "cargo", "nix",
  // 包管理 / 开发工具
  "uv", "composer", "gofumpt", "staticcheck", "jq", "yq", "helm", "kustomize",
  "buf", "actionlint", "just", "biome", "volta", "sops", "opentofu", "vault",
  "packer", "pypy", "ipython", "jupyter",
  // Shell
  "bash", "powershell", "shellcheck", "fish", "nu",
  // 配置 / 云 / 数据
  "terraform", "kubernetes", "kubectl", "graphql", "yaml", "toml",
  "curl", "wget",
  // 前端框架 / 标记
  "vue", "react", "svelte", "markdown", "json",
  // 数据库 / 数据服务
  "mysql", "postgresql", "redis", "mongodb", "sqlite", "mariadb",
  "clickhouse", "mssql", "cassandra", "couchdb", "elasticsearch", "influxdb",
  // 云 / DevOps / 容器 / 配置
  "awscli", "gcloud", "az", "gh", "git", "git-lfs", "docker",
  "ansible", "terragrunt", "kubeconform", "k9s", "kind", "k3d",
  "minikube", "helmfile", "argocd", "flux", "istioctl", "consul", "nomad",
  "age", "envsubst",
  // Go 生态工具
  "golangci-lint", "pkl", "air", "mockery", "swag", "gopls", "goimports",
  // Python 生态工具
  "poetry", "pipx", "rye", "pdm", "hatch", "ruff", "pre-commit", "twine",
  "virtualenv", "zizmor",
  // 前端工程与测试
  "eslint", "prettier", "jest", "vitest", "nx", "turbo", "vite", "webpack",
  "rollup", "parcel", "gulp", "babel",
  // 代码质量 / 监控
  "coverage", "sonar", "sentry", "talisman",
  // 更多语言运行时
  "fsharp", "raku", "idris", "pike", "smalltalk", "tcl", "rexx",
  "cobol", "eiffel", "logo",
];

/** 工具分类 */
const TOOL_CATS: Record<string, string[]> = {
  "语言运行时": [
    "node", "python", "ruby", "go", "rust", "java", "kotlin", "dotnet",
    "php", "swift", "dart", "flutter", "haskell", "lua", "perl", "zig",
    "nim", "crystal", "scala", "julia", "clojure", "clojurescript", "c", "cpp",
    "csharp", "r", "v", "ocaml", "typescript", "javascript", "d", "elm",
    "groovy", "haxe", "lisp", "objectivec", "prolog", "racket", "reason",
    "solidity", "pascal", "ada", "applescript", "fortran", "coq", "fsharp",
    "raku", "idris", "pike", "smalltalk", "tcl", "rexx", "cobol", "eiffel",
    "logo",
  ],
  "JS 生态": ["npm", "bun", "deno", "pnpm", "yarn", "nodejs", "coffeescript"],
  "前端框架": ["vue", "react", "svelte"],
  "前端工程": [
    "eslint", "prettier", "jest", "vitest", "nx", "turbo", "vite", "webpack",
    "rollup", "parcel", "gulp", "babel",
  ],
  "构建工具": [
    "cmake", "gradle", "maven", "make", "ninja", "gcc", "clang", "protoc",
    "cargo", "nix", "terraform",
  ],
  "Shell": ["bash", "powershell", "shellcheck", "fish", "nu"],
  "包管理/工具": [
    "uv", "composer", "gofumpt", "staticcheck", "jq", "yq", "helm", "kustomize",
    "buf", "actionlint", "just", "biome", "volta", "sops", "opentofu", "vault",
    "packer", "pypy", "ipython", "jupyter",
  ],
  "数据库/服务": [
    "mysql", "postgresql", "redis", "mongodb", "sqlite", "mariadb",
    "clickhouse", "mssql", "cassandra", "couchdb", "elasticsearch", "influxdb",
  ],
  "云/DevOps": [
    "kubernetes", "kubectl", "graphql", "yaml", "toml", "curl", "wget",
    "awscli", "gcloud", "az", "gh", "git", "git-lfs", "docker", "ansible",
    "terragrunt", "kubeconform", "k9s", "kind", "k3d", "minikube", "helmfile",
    "argocd", "flux", "istioctl", "consul", "nomad", "age", "envsubst",
  ],
  "Go 工具": [
    "golangci-lint", "pkl", "air", "mockery", "swag", "gopls", "goimports",
  ],
  "Python 工具": [
    "poetry", "pipx", "rye", "pdm", "hatch", "ruff", "pre-commit", "twine",
    "virtualenv", "zizmor",
  ],
  "代码质量": ["coverage", "sonar", "sentry", "talisman"],
  "标记/数据": ["markdown", "json"],
};
const CAT_KEYS = Object.keys(TOOL_CATS);

/** 可托管但非 mise 运行时的软件/服务：分离到“软件渠道”页，并提供官方下载入口 */
export interface DownloadSoft {
  name: string;
  url: string;
  desc?: string;
}
export const SOFTWARE: DownloadSoft[] = [
  { name: "docker", url: "https://docs.docker.com/get-docker/", desc: "容器平台" },
  { name: "kubectl", url: "https://kubernetes.io/docs/tasks/tools/", desc: "Kubernetes CLI" },
  { name: "kind", url: "https://kind.sigs.k8s.io/", desc: "本地 Kubernetes" },
  { name: "k3d", url: "https://k3d.io/", desc: "本地 Kubernetes" },
  { name: "minikube", url: "https://minikube.sigs.k8s.io/docs/start/", desc: "本地 Kubernetes" },
  { name: "k9s", url: "https://k9scli.io/", desc: "K8s 终端 UI" },
  { name: "helmfile", url: "https://github.com/helmfile/helmfile", desc: "Helm 编排" },
  { name: "kubeconform", url: "https://github.com/yannh/kubeconform", desc: "K8s 校验" },
  { name: "argocd", url: "https://argoproj.github.io/cd/", desc: "GitOps CD" },
  { name: "istioctl", url: "https://istio.io/latest/docs/setup/install/", desc: "服务网格 CLI" },
  { name: "consul", url: "https://www.consul.io/downloads", desc: "服务发现" },
  { name: "nomad", url: "https://www.nomadproject.io/downloads", desc: "调度器" },
  { name: "mysql", url: "https://dev.mysql.com/downloads/mysql/", desc: "数据库" },
  { name: "postgresql", url: "https://www.postgresql.org/download/", desc: "数据库" },
  { name: "redis", url: "https://redis.io/download/", desc: "缓存/数据库" },
  { name: "mongodb", url: "https://www.mongodb.com/try/download/community", desc: "文档数据库" },
  { name: "sqlite", url: "https://www.sqlite.org/download.html", desc: "嵌入式数据库" },
  { name: "mariadb", url: "https://mariadb.org/download/", desc: "数据库" },
  { name: "clickhouse", url: "https://clickhouse.com/docs/en/install", desc: "分析数据库" },
  { name: "mssql", url: "https://www.microsoft.com/en-us/sql-server/sql-server-downloads", desc: "关系数据库" },
  { name: "cassandra", url: "https://cassandra.apache.org/_/download.html", desc: "分布式 DB" },
  { name: "couchdb", url: "https://couchdb.apache.org/#download", desc: "文档数据库" },
  { name: "elasticsearch", url: "https://www.elastic.co/downloads/elasticsearch", desc: "搜索/分析" },
  { name: "influxdb", url: "https://www.influxdata.com/downloads/", desc: "时序数据库" },
  { name: "awscli", url: "https://aws.amazon.com/cli/", desc: "AWS CLI" },
  { name: "gcloud", url: "https://cloud.google.com/sdk/docs/install", desc: "Google Cloud CLI" },
  { name: "az", url: "https://learn.microsoft.com/cli/azure/install-azure-cli", desc: "Azure CLI" },
  { name: "gh", url: "https://github.com/cli/cli/releases", desc: "GitHub CLI" },
  { name: "git-lfs", url: "https://git-lfs.com/", desc: "Git 大文件" },
  { name: "age", url: "https://github.com/FiloSottile/age", desc: "文件加密" },
  { name: "envsubst", url: "https://github.com/a8m/envsubst", desc: "环境变量替换" },
  { name: "wget", url: "https://www.gnu.org/software/wget/", desc: "网络下载" },
  { name: "curl", url: "https://curl.se/download.html", desc: "网络传输" },
  { name: "sonar", url: "https://www.sonarsource.com/products/sonarqube/downloads/", desc: "代码质量" },
  { name: "sentry", url: "https://sentry.io/", desc: "错误监控" },
  { name: "talisman", url: "https://github.com/thought-machine/talisman", desc: "秘钥守护" },
];
export const SOFT_SET = new Set(SOFTWARE.map((s) => s.name));

// 纯标记/概念（无安装实体），从展示移除，不属运行时也不属软件下载
const HIDDEN_SET = new Set(["markdown", "json", "yaml", "toml", "logo", "coverage"]);

/** 仅运行时集合（供展示与设置页自检；排除软件与纯格式项） */
export const KNOWN_RUNTIMES = KNOWN_TOOLS.filter(
  (n) => !SOFT_SET.has(n) && !HIDDEN_SET.has(n)
);

export default function ToolsView() {
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [sources, setSources] = useState<ToolSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [installFor, setInstallFor] = useState<ToolInfo | null>(null);
  const [catFilter, setCatFilter] = useState<string>("全部");
  const [compact, setCompact] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [toolsRes, sourcesRes] = await Promise.all([
        listTools(),
        detectToolSources(),
      ]);
      setTools(toolsRes);
      setSources(sourcesRes);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function handleActivate(tool: string, version: string, global: boolean) {
    setBusy(`${tool}@${version}`);
    setNotice(null);
    try {
      await useVersion(tool, version, global);
      setNotice(global ? "已设为全局默认版本" : "已在当前项目启用该版本");
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleUninstall(tool: string, version: string) {
    setBusy(`${tool}@${version}`);
    setNotice(null);
    try {
      await uninstallVersion(tool, version);
      setNotice("卸载完成");
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  // 已由 mise 托管
  const installedSet = new Set(tools.map((t) => t.name));
  // 其他工具托管 -> 按工具聚合
  const externalByTool = new Map<string, ToolSource[]>();
  for (const s of sources) {
    const list = externalByTool.get(s.tool) || [];
    list.push(s);
    externalByTool.set(s.tool, list);
  }
  // 候选工具中尚未被 mise 托管的（排除软件下载项与纯格式项）
  const others = KNOWN_RUNTIMES.filter((n) => !installedSet.has(n));
  // 未安装的候选也按统一卡片展示（0 个版本），允许安装。
// 排序：已安装 > 仅托管 > 未安装，组内按名称。
const rank = (t: ToolInfo) =>
  t.versions.length > 0
    ? 0
    : externalByTool.get(t.name)?.length
    ? 1
    : 2;
const merged: ToolInfo[] = [
  ...tools,
  ...others.map((name) => ({ name, versions: [], active_versions: [] })),
].sort((a, b) => {
  const r = rank(a) - rank(b);
  return r !== 0 ? r : a.name.localeCompare(b.name);
});
// 分类筛选
const filtered =
  catFilter === "全部"
    ? merged
    : merged.filter((t) => (TOOL_CATS[catFilter] || []).includes(t.name));
// 紧凑模式状态色
const statusOf = (t: ToolInfo) =>
  t.versions.length > 0 ? "installed" : externalByTool.get(t.name)?.length ? "managed" : "uninstalled";
const statusLabel = (t: ToolInfo) =>
  t.versions.length > 0 ? "已安装" : externalByTool.get(t.name)?.length ? "仅托管" : "未安装";

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>运行时工具</h1>
          <p className="view-sub">由 mise 管理的所有运行时与已安装版本</p>
        </div>
        <button
          className="btn-ghost"
          onClick={() => setCompact((c) => !c)}
          title="切换紧凑模式"
        >
          {compact ? "▦ 列表" : "▣ 紧凑"}
        </button>
      </div>

      <div className="toolbar">
        <div className="filter-chips">
          <button
            className={`chip ${catFilter === "全部" ? "active" : ""}`}
            onClick={() => setCatFilter("全部")}
          >
            全部
          </button>
          {CAT_KEYS.map((c) => (
            <button
              key={c}
              className={`chip ${catFilter === c ? "active" : ""}`}
              onClick={() => setCatFilter(c)}
            >
              {c}
            </button>
          ))}
        </div>
      </div>

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

      {loading && merged.length === 0 ? (
        <div className="empty">正在读取环境…</div>
      ) : compact ? (
        <div className="tool-grid compact">
          {CAT_KEYS.map((cat) => {
            const grp = filtered.filter((t) => (TOOL_CATS[cat] || []).includes(t.name));
            if (grp.length === 0) return null;
            return (
              <div className="compact-group" key={cat}>
                <span className="compact-group-label">{cat}</span>
                <div className="compact-row">
                  {grp.map((tool) => (
                    <button
                      key={tool.name}
                      className={`tc-chip ${statusOf(tool)}`}
                      onClick={() => setInstallFor(tool)}
                      title={`${tool.name} · ${statusLabel(tool)}`}
                    >
                      <ToolBadge name={tool.name} />
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <>
          <div className="tool-grid">
              {filtered.map((tool) => {
                const isOpen = expanded === tool.name;
                const activeSet = new Set(tool.active_versions);
                const ext = externalByTool.get(tool.name);
                const isInstalled = tool.versions.length > 0;
                const isManaged = !isInstalled && Boolean(ext?.length);
                return (
                  <div
                    className={`tool-card ${isOpen ? "open" : ""}`}
                    key={tool.name}
                  >
                    <button
                      className="tool-card-head"
                      onClick={() => setExpanded(isOpen ? null : tool.name)}
                    >
                      <ToolBadge name={tool.name} />
                      <span className="tool-name">{tool.name}</span>
                      <span
                        className={`pill state ${isInstalled ? "active" : isManaged ? "managed" : "uninstalled"}`}
                      >
                        {isInstalled ? "已安装" : isManaged ? "仅托管" : "未安装"}
                      </span>
                      {tool.active_versions.length > 0 && (
                        <span className="pill active">
                          ▲ {tool.active_versions.join(", ")}
                        </span>
                      )}
                      <span className="tool-count">
                        {tool.versions.length} 个版本
                      </span>
                      <span className="chevron">{isOpen ? "▾" : "▸"}</span>
                    </button>

                    {isOpen && (
                      <div className="tool-card-body">
                        <div className="version-list">
                          {tool.versions.length === 0 && (
                            <div className="empty small">
                              {externalByTool.get(tool.name)?.length
                                ? `由 ${externalByTool
                                    .get(tool.name)!
                                    .map((s) => s.manager)
                                    .join(", ")} 托管 · 尚未用 mise 安装`
                                : "尚未安装版本"}
                            </div>
                          )}
                          {tool.versions.map((v) => {
                            const isActive = activeSet.has(v.version);
                            const busyKey = `${tool.name}@${v.version}`;
                            return (
                              <div className="version-row" key={v.version}>
                                <div className="version-info">
                                  <span className="version-no">{v.version}</span>
                                  {isActive && <span className="pill active small">当前</span>}
                                  {v.requested_version &&
                                    v.requested_version !== v.version && (
                                      <span className="pill muted small">
                                        {v.requested_version}
                                      </span>
                                    )}
                                </div>
                                <div className="version-actions">
                                  <button
                                    className={`btn xs ${!isActive ? "primary" : ""}`}
                                    disabled={busy === busyKey}
                                    onClick={() =>
                                      handleActivate(tool.name, v.version, true)
                                    }
                                    title={
                                      isActive
                                        ? "此版本已是全局默认"
                                        : "设为全局默认版本"
                                    }
                                  >
                                    {isActive ? "全局" : "设为全局"}
                                  </button>
                                  <button
                                    className="btn xs danger"
                                    disabled={busy === busyKey && tool.versions.length === 1}
                                    onClick={() =>
                                      handleUninstall(tool.name, v.version)
                                    }
                                  >
                                    卸载
                                  </button>
                                </div>
                              </div>
                            );
                          })}
                        </div>
                        <button
                          className="btn add"
                          onClick={() => setInstallFor(tool)}
                        >
                          + 安装其他版本
                        </button>
                      </div>
                    )}
              </div>
            );
          })}
        </div>
          </>
      )}

      {installFor && (
        <InstallDialog
          tool={installFor}
          external={externalByTool.get(installFor.name)}
          onClose={() => setInstallFor(null)}
          onDone={async () => {
            await refresh();
            setNotice("安装完成");
          }}
        />
      )}
    </div>
  );
}