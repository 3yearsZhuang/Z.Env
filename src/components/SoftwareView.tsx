import { useEffect, useMemo, useState, type ReactNode } from "react";
import { CAT_KEYS_ALL, KNOWN_RUNTIMES, SOFTWARE, categoryOf } from "../data/catalog";
import {
  detectSystemInstalled,
  errorMessage,
  getEnvInfo,
  installSystemPackage,
  listTools,
  onSysInstallProgress,
} from "../api";
import InstallDialog from "./InstallDialog";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";
import type { ToolInfo } from "../api";

/** 渠道安装弹窗：仅官方下载的软件，提供官方下载与各包管理器安装命令 */
export function ChannelDialog({
  name,
  url,
  desc,
  available,
  onClose,
}: {
  name: string;
  url?: string;
  desc?: string;
  available: Set<string>;
  onClose: () => void;
}) {
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="w-[420px] p-0">
        <div className="dialog-head">
          <DialogTitle>{name}</DialogTitle>
        </div>
        <div className="dialog-body">
          {desc && <p className="dialog-tip">{desc}</p>}
          {url && (
            <a
              className="btn"
              style={{ display: "inline-flex", marginBottom: 12 }}
              href={url}
              target="_blank"
              rel="noreferrer"
            >
              前往官方下载 →
            </a>
          )}
          <div className="field-label">通过包管理器安装（点击命令复制）</div>
          <div className="channel-list">
            {PKG_MANAGERS.filter((pm) => available.has(pm)).map((pm) => {
              const cmd = pkgCommand(pm, name);
              return (
                <button
                  key={pm}
                  className="channel-row on"
                  title={cmd}
                  onClick={() => copyText(cmd)}
                >
                  <span className="channel-name">{pm}</span>
                  <code className="channel-cmd">{cmd}</code>
                  <span className="channel-copy">复制</span>
                </button>
              );
            })}
          </div>
        </div>
        <div className="dialog-foot">
          <button className="btn-ghost" onClick={onClose}>
            关闭
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

interface Item {
  name: string;
  kind: "runtime" | "download";
  cat: string;
  url?: string;
  desc?: string;
}

// 与“运行时工具”页共用同一套标签系统（TOOL_CATS），勿在此重复定义分类
function softwareCat(name: string): string {
  return categoryOf(name);
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

/** 各包管理器对应的安装命令（默认包名 = 工具名，按各系统规范生成） */
function pkgCommand(pm: PkgName, name: string): string {
  switch (pm) {
    case "brew":
      return `brew install ${name}`;
    case "winget":
      return `winget install ${name}`;
    case "apt":
      return `sudo apt install -y ${name}`;
    case "pacman":
      return `sudo pacman -S --noconfirm ${name}`;
  }
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
  const [available, setAvailable] = useState<Set<string>>(new Set());
  const [channel, setChannel] = useState<Item | null>(null);
  // 系统包管理器原生安装任务（每次只允许一个进行中任务）
  const [sysJob, setSysJob] = useState<{
    manager: string;
    name: string;
    status: "run" | "ok" | "err";
    line: string;
  } | null>(null);

  // 订阅系统包管理器原生安装进度
  useEffect(() => {
    let un: (() => void) | undefined;
    onSysInstallProgress((p) =>
      setSysJob((prev) =>
        prev && prev.manager === p.manager && prev.name === p.name
          ? { ...prev, line: p.line }
          : prev,
      ),
    ).then((fn) => {
      un = fn;
    });
    return () => {
      un?.();
    };
  }, []);

  // 已通过系统包管理器安装的工具（manager -> 工具名集合），用于原生安装后持久标记“已安装”
  const [sysInstalled, setSysInstalled] = useState<Record<string, Set<string>>>({});

  // 通过系统包管理器原生安装
  const doInstall = (pm: PkgName, name: string) => {
    if (sysJob && sysJob.status === "run") return;
    setSysJob({ manager: pm, name, status: "run", line: "" });
    installSystemPackage(pm, name)
      .then(async () => {
        // 重新探测该管理器已装列表，并自动刷新对应工具状态
        const names = await detectSystemInstalled(pm).catch(() => []);
        setSysInstalled((m) => {
          const next = new Set(m[pm] || []);
          names.forEach((n) => next.add(n));
          next.add(name);
          return { ...m, [pm]: next };
        });
        setSysJob({ manager: pm, name, status: "ok", line: "安装完成，状态已更新" });
        refreshInstalled();
      })
      .catch((e) => setSysJob({ manager: pm, name, status: "err", line: errorMessage(e) }));
  };

  // 检测本机可用的包管理器，并真实探测其已装列表（跨启动持久化“已安装”状态）
  useEffect(() => {
    let mounted = true;
    getEnvInfo()
      .then(async (e) => {
        const availNames = e.pkg.filter((p) => p.available).map((p) => p.name);
        if (!mounted) return;
        setAvailable(new Set(availNames));
        const rec: Record<string, Set<string>> = {};
        await Promise.all(
          availNames.map(async (pm) => {
            try {
              rec[pm] = new Set(await detectSystemInstalled(pm));
            } catch {
              rec[pm] = new Set();
            }
          }),
        );
        if (mounted) setSysInstalled(rec);
      })
      .catch(() => {});
    return () => {
      mounted = false;
    };
  }, []);

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
      // 与“运行时工具”页共用同一套标签系统
      arr.push({ name, kind: "runtime", cat: categoryOf(name) });
    }
    for (const s of SOFTWARE) {
      arr.push({
        name: s.name,
        kind: "download",
        cat: softwareCat(s.name),
        url: s.url,
        desc: s.desc,
      });
    }
    return arr;
  }, []);

  const filtered = items.filter((it) => {
    if (cat !== "全部" && it.cat !== cat) return false;
    if (query) {
      const q = query.toLowerCase();
      if (!it.name.toLowerCase().includes(q) && !(it.desc || "").toLowerCase().includes(q))
        return false;
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
        {["全部", ...CAT_KEYS_ALL].map((c) => (
          <button key={c} className={`chip ${cat === c ? "active" : ""}`} onClick={() => setCat(c)}>
            {c}
          </button>
        ))}
      </div>

      <div className="soft-grid">
        {filtered.map((it) => {
          // 各可用包管理器芯片：点击直接原生安装，右键或悬停的复制钮可复制命令
          const chips = PKG_MANAGERS.filter((pm) => available.has(pm)).map((pm) => {
            const cmd = pkgCommand(pm, it.name);
            const st =
              sysJob && sysJob.manager === pm && sysJob.name === it.name ? sysJob.status : null;
            const isHere = !!sysInstalled[pm]?.has(it.name);
            const stateCls =
              st === "run"
                ? " working"
                : isHere || st === "ok"
                  ? " ok"
                  : st === "err"
                    ? " err"
                    : "";
            return (
              <span
                key={pm}
                className={"pkg-chip on" + stateCls}
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  doInstall(pm, it.name);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  copyText(cmd);
                }}
                title={
                  st === "run"
                    ? `${cmd} · 正在安装…`
                    : isHere
                      ? `已通过 ${pm} 安装`
                      : st === "err"
                        ? `${cmd} · 上次安装失败`
                        : `${cmd} · 点击原生安装，右键复制命令`
                }
              >
                {pm}
                {st === "run" ? "…" : ""}
                <span
                  className="chip-copy"
                  title="复制安装命令"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    copyText(cmd);
                  }}
                >
                  ⧉
                </span>
              </span>
            );
          });
          // 包管理器行：下载类无 mise 徽标；运行时类在行首放“已安装/可安装”徽标
          const pkgRow = (lead?: ReactNode) => (
            <span className="pkg-row">
              {lead}
              {chips}
            </span>
          );

          if (it.kind === "download") {
            return (
              <button
                className="soft-card soft-install"
                key={it.name}
                onClick={() => setChannel(it)}
                title="选择安装渠道（官方下载/包管理器）"
              >
                <SoftIcon name={it.name} />
                <div className="soft-info">
                  <span className="soft-name">{it.name}</span>
                  {it.desc && <span className="soft-desc">{it.desc}</span>}
                  {pkgRow()}
                </div>
                <span className="soft-link">安装 +</span>
              </button>
            );
          }

          const isInstalled = installed.has(it.name);
          return (
            <button
              className="soft-card soft-install"
              key={it.name}
              onClick={() => setInstallFor({ name: it.name, versions: [], active_versions: [] })}
              title={isInstalled ? "查看已安装版本" : "用 mise 安装"}
            >
              <SoftIcon name={it.name} />
              <div className="soft-info">
                <span className="soft-name">{it.name}</span>
                <span className="soft-desc">可由 mise 安装</span>
                {pkgRow(
                  <span className={"inst-badge" + (isInstalled ? " yes" : "")}>
                    {isInstalled ? "已安装" : "可安装"}
                  </span>,
                )}
              </div>
              <span className="soft-link">{isInstalled ? "管理" : "安装 +"}</span>
            </button>
          );
        })}
        {filtered.length === 0 && <div className="empty small">没有匹配的项目</div>}
      </div>

      {sysJob && (
        <div className="sys-job">
          <span className={"sys-dot " + sysJob.status} />
          <b>
            {sysJob.manager} · {sysJob.name}
          </b>
          <code className="sys-line">
            {sysJob.line || (sysJob.status === "run" ? "正在执行安装…" : "")}
          </code>
          <button className="btn-ghost icon-only" onClick={() => setSysJob(null)} title="关闭">
            ×
          </button>
        </div>
      )}

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
      {channel && (
        <ChannelDialog
          name={channel.name}
          url={channel.url}
          desc={channel.desc}
          available={available}
          onClose={() => setChannel(null)}
        />
      )}
    </div>
  );
}
