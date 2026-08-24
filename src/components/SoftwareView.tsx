import { useState } from "react";
import { SOFTWARE, DownloadSoft } from "./ToolsView";

/** 工具徽标：资源在 public/tools/{name}.svg，缺失回退首字母 */
function SoftIcon({ name }: { name: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) {
    return <span className="tool-badge">{name.slice(0, 2).toUpperCase()}</span>;
  }
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

export default function SoftwareView() {
  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>软件渠道</h1>
          <p className="view-sub">
            非 mise 运行时的软件/服务：可托管但不提供版本下载，改由官方渠道获取
          </p>
        </div>
      </div>

      <div className="soft-grid">
        {SOFTWARE.map((s: DownloadSoft) => (
          <a
            className="soft-card"
            key={s.name}
            href={s.url}
            target="_blank"
            rel="noreferrer"
          >
            <SoftIcon name={s.name} />
            <div className="soft-info">
              <span className="soft-name">{s.name}</span>
              {s.desc && <span className="soft-desc">{s.desc}</span>}
            </div>
            <span className="soft-link">官方下载 ↔</span>
          </a>
        ))}
      </div>
    </div>
  );
}