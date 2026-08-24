import { useRef } from "react";

const APP_NAME = "Z.Env";
const APP_TAGLINE = "跨平台 mise 图形化管理器";
const APP_VERSION = "0.6.7";

const STACK = [
  "Tauri 2",
  "React + TypeScript",
  "Rust",
  "CSS (亚克力 / 液态玻璃)",
  "mise · asdf · GitHub Releases · 官方生态源",
];

const LINKS: { label: string; url: string }[] = [
  { label: "GitHub: 3yearsZhuang", url: "https://github.com/3yearsZhuang" },
  { label: "mise 官方文档", url: "https://mise.jdx.dev" },
  { label: "mise install", url: "https://mise.jdx.dev/getting-started.html" },
  { label: "GitHub: mise", url: "https://github.com/jdx/mise" },
];

/** 触发彩蛋的连续点击次数 */
const EASTER_CLICKS = 6;

interface Props {
  onEaster: () => void;
}

export default function SettingsView({ onEaster }: Props) {
  const clickCount = useRef(0);

  const handleVersionClick = () => {
    clickCount.current += 1;
    if (clickCount.current >= EASTER_CLICKS) {
      clickCount.current = 0;
      onEaster();
    }
  };

  return (
    <div className="view">
      <div className="view-head">
        <div>
          <h1>设置</h1>
          <p className="view-sub">关于与偏好</p>
        </div>
      </div>

      <section className="about-hero">
        <div className="about-mark">
          <img src="/zenv-icon.svg" alt={APP_NAME} draggable={false} />
        </div>
        <div className="about-title">{APP_NAME}</div>
        <div className="about-version">
          版本{" "}
          <code onClick={handleVersionClick} title="?">
            {APP_VERSION}
          </code>
        </div>
        <p className="about-desc">
          {APP_TAGLINE}。将 <strong>mise</strong> 包装为可视化界面，统合系统资源监控、
          运行时工具管理与项目环境预设。
        </p>
      </section>

      <section className="panel">
        <h2 className="panel-title">技术栈</h2>
        <div className="about-tags">
          {STACK.map((s) => (
            <span className="pill muted" key={s}>
              {s}
            </span>
          ))}
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">相关链接</h2>
        <div className="about-links">
          {LINKS.map((l) => (
            <a key={l.url} href={l.url} target="_blank" rel="noreferrer">
              {l.label} ↔
            </a>
          ))}
        </div>
      </section>

      <p className="about-foot">
        CC BY-NC-SA 4.0 · 用 <strong>mise</strong> 管理你的运行时，让环境回归简单。
      </p>
    </div>
  );
}