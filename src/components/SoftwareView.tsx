import { useEffect, useMemo, useState } from "react";
import { KNOWN_RUNTIMES, SOFTWARE, TOOL_CATS } from "./ToolsView";
import { listTools } from "../api";
import InstallDialog from "./InstallDialog";
import type { ToolInfo } from "../api";

interface Item {
  name: string;
  kind: "runtime" | "download";
  cat: string;
  url?: string;
  desc?: string;
}

// 统一分类（运行时与软件共用）
const CAT_ORDER = [
  "语言运行时",
  "JS/前端",
  "数据库",
  "云/容器",
  "Shell",
  "构建/包管理",
  "Python",
  "Go",
  "其他",
];
const CAT_NORM: Record<string, string> = {
  语言运行时: "语言运行时",
  "JS 生态": "JS/前端",
  前端框架: "JS/前端",
  前端工程: "JS/前端",
  构建工具: "构建/包管理",
  Shell: "Shell",
  "包管理/工具": "构建/包管理",
  "Go 工具": "Go",
  "Python 工具": "Python",
  代码质量: "其他",
  标记数据: "其他",
};

function softwareCat(name: string): string {
  const s = name.toLowerCase();
  if (/(mysql|postgres|redis|mongo|sqlite|mariadb|clickhouse|mssql|cassandra|couchdb|elastic|influx)/.test(s))
    return "数据库";
  if (/(docker|kube|aws|gcloud|azure|argocd|consul|nomad|istio|minikube|k3d|kind|envsubst|age|helmfile|coredns)/.test(s))
    return "云/容器";
  if (/(vue|react|svelte|eslint|jest|vitest|nx|vite|webpack|rollup|parcel|gulp|babel)/.test(s))
    return "JS/前端";
  if (/(bash|fish|nu|zsh)/.test(s)) return "Shell";
  if (/(pypy|ipython|jupyter|twine|virtualenv)/.test(s)) return "Python";
  if (/(gopls|goimports)/.test(s)) return "Go";
  if (/(haskell|nim|ocaml|typescript|csharp|lisp|objectivec|prolog|reason|pascal|ada|fortran|coq|clojurescript|coffeescript|fsharp|raku|idris|pike|smalltalk|tcl|cobol|eiffel|gcc|cargo|dlang|openssl)/.test(s))
    return "语言运行时";
  return "其他";
}

/** 徽标：有 /tools/{name}.svg 显示图标，缺失回退首字母 */
function SoftIcon({ name }: { name: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <span className="tool-badge">{name.slice(0, 2).toUpperCase()}</span>;
  return (
    <span className="tool-badge icon">
      <img
        src={`/tools/${name}.svg`}
        alt={name}
        className="tool-badge-img"
        draggable={false}
        onError={() => setFailed(true)}
      />
    </span>
  );
}

const PKG_MANAGERS = ["brew", "winget", "apt", "pacman"] as const;
type PkgName = (typeof PKG_MANAGERS)[number];

/** 各包管理器对应的安装命令（默认包名 = 工具名） */
function pkgCommand(pm: PkgName, name: string): string {
  return `${pm} install ${name}`;
}

/** 复制命令到剪贴板 */
async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    /* 忽略 */
  }
}

/** 支持列表页 */
export default function SoftwareView() {
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState("全部");
  const [installFor, setInstallFor] = useState<ToolInfo | null>(null);
  const [installed, setInstalled] = useState<Set<string>>(new Set());

  const refreshInstalled = () => {
    listTools()
      .then((ts) => setInstalled(new Set(ts.map((t) => t.name))))
      .catch(() => {});
  };
  useEffect(() => {
    refreshInstalled();
  }, []);

  const items = useMemo<Item[]>(() => {
    const arr: Item[] = [];
    for (const name of KNOWN_RUNTIMES) {
      let c = "其他";
      for (const [k, names] of Object.entries(TOOL_CATS)) {
        if (names.includes(name)) {
          c = CAT_NORM[k] || "其他";
          break;
        }
      }
      arr.push({ name, kind: "runtime", cat: c });
    }
    for (const s of SOFTWARE) {
      arr.push({ name: s.name, kind: "download", cat: softwareCat(s.name), url: s.url, desc: s.desc });
    }
    return arr;
  }, []);

  const filtered = items.filter((it) => {
    if (cat !== "全部" && it.cat !== cat) return false;
    if (query) {
      const q = query.toLowerCase();
      if (!it.name.toLowerCase().includes(q) && !(it.desc || "").toLowerCase().includes(q)) return false;
    }
    return true;
  });

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>支持列表</h1>
          <p className="view-sub">
            全部受支持的运行时与软件：运行时可直接用 mise 安装，其它软件提供官方下载渠道
          </p>
        </div>
      </div>

      <div className="support-toolbar">
        <input
          className="input support-search"
          placeholder="搜索支持的软件或运行时…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <div className="filter-chips support-chips">
        {["全部", ...CAT_ORDER].map((c) => (
          <button
            key={c}
            className={`chip ${cat === c ? "active" : ""}`}
            onClick={() => setCat(c)}
          >
            {c}
          </button>
        ))}
      </div>

      <div className="soft-grid">
        {filtered.map((it) =>
          it.kind === "download" ? (
            <a
              className="soft-card"
              key={it.name}
              href={it.url}
              target="_blank"
              rel="noreferrer"
            >
              <SoftIcon name={it.name} />
              <div className="soft-info">
                <span className="soft-name">{it.name}</span>
                {it.desc && <span className="soft-desc">{it.desc}</span>}
                <span className="pkg-row" title="点击复制安装命令">
                  {PKG_MANAGERS.map((pm) => (
                    <span
                      key={pm}
                      className="pkg-chip"
                      onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        copyText(pkgCommand(pm, it.name));
                      }}
                      title={`${pkgCommand(pm, it.name)} · 点击复制`}
                    >
                      {pm}
                    </span>
                  ))}
                </span>
              </div>
              <span className="soft-link">官方下载 ↔</span>
            </a>
          ) : (
            (() => {
              const isInstalled = installed.has(it.name);
              return (
                <button
                  className="soft-card soft-install"
                  key={it.name}
                  onClick={() =>
                    setInstallFor({ name: it.name, versions: [], active_versions: [] })
                  }
                  title={isInstalled ? "查看已安装版本" : "用 mise 安装"}
                >
                  <SoftIcon name={it.name} />
                  <div className="soft-info">
                    <span className="soft-name">
                      {it.name}
                      <span className={"inst-badge" + (isInstalled ? " yes" : "")}>
                        {isInstalled ? "已安装" : "可安装"}
                      </span>
                    </span>
                    <span className="soft-desc">mise 管理</span>
                  </div>
                  <span className="soft-link">{isInstalled ? "管理" : "安装 +"}</span>
                </button>
              );
            })()
          )
        )}
        {filtered.length === 0 && (
          <div className="empty small">没有匹配的项目</div>
        )}
      </div>

      {installFor && (
        <InstallDialog
          tool={installFor}
          onClose={() => setInstallFor(null)}
          onDone={() => {
            setInstallFor(null);
            refreshInstalled();
          }}
        />
      )}
    </div>
  );
}