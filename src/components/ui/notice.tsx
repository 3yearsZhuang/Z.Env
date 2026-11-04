import type { ComponentType, CSSProperties, ReactNode } from "react";
import { CheckCircle2, CircleAlert, Info, TriangleAlert, X } from "lucide-react";
import { cn } from "../../lib/utils";

type Tone = "error" | "warn" | "success" | "info";

const ICON: Record<Tone, ComponentType<{ size?: number; "aria-hidden"?: boolean }>> = {
  error: CircleAlert,
  warn: TriangleAlert,
  success: CheckCircle2,
  info: Info,
};

const TONE_LABEL: Record<Tone, string> = {
  error: "错误",
  warn: "警告",
  success: "成功",
  info: "提示",
};

export interface NoticeProps {
  tone?: Tone;
  children: ReactNode;
  /** 提供后显示可键盘操作的关闭按钮；不要再用可点击的 div 当关闭入口。 */
  onDismiss?: () => void;
  className?: string;
  style?: CSSProperties;
}

/** 统一的页面级提示条：沿用 .banner 视觉，错误/警告用 role="alert"，
 *  其余用 role="status"，图标 aria-hidden，关闭入口是真正的 button。 */
export function Notice({ tone = "info", children, onDismiss, className, style }: NoticeProps) {
  const Icon = ICON[tone];
  const assertive = tone === "error" || tone === "warn";
  return (
    <div
      className={cn("banner", tone, "notice", className)}
      role={assertive ? "alert" : "status"}
      style={style}
    >
      <Icon size={15} aria-hidden={true} />
      <span className="notice-text">{children}</span>
      {onDismiss && (
        <button
          type="button"
          className="notice-dismiss"
          onClick={onDismiss}
          aria-label={`关闭${TONE_LABEL[tone]}提示`}
        >
          <X size={14} aria-hidden={true} />
        </button>
      )}
    </div>
  );
}
