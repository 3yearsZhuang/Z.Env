import type { ComponentType, ReactNode } from "react";
import { cn } from "../../lib/utils";

type Size = "sm" | "md" | "lg";

export interface StateBlockProps {
  /** sm/md 是纯文本提示；lg 是「图标 + 标题 + 说明 + 动作」块（默认）。 */
  size?: Size;
  icon?: ComponentType<{ size?: number; className?: string; "aria-hidden"?: boolean }>;
  /** 图标旋转（读取中）。 */
  spinning?: boolean;
  title?: ReactNode;
  detail?: ReactNode;
  action?: ReactNode;
  /** lg 的两种修饰：虚线边框、最小高度（项目列表里用）。 */
  bordered?: boolean;
  tall?: boolean;
  children?: ReactNode;
  className?: string;
}

/** 空态 / 读取中 / 无结果的统一形态。
 *
 * 此前 `.project-empty`、`.ops-empty`、`.support-empty` 与 `.empty` 是四套各自写死的几何，
 * 间距和圆角都不一致；这里收敛为同一组件与同一组令牌。 */
export function StateBlock({
  size = "lg",
  icon: Icon,
  spinning,
  title,
  detail,
  action,
  bordered,
  tall,
  children,
  className,
}: StateBlockProps) {
  const content = children ?? detail;
  if (size === "sm" || size === "md") {
    return (
      <div className={cn("state-block", `state-block-${size}`, className)} role="status">
        {content}
      </div>
    );
  }
  return (
    <div
      className={cn(
        "state-block",
        "state-block-lg",
        bordered && "bordered",
        tall && "tall",
        className,
      )}
      role="status"
    >
      {Icon && (
        <Icon
          size={spinning ? 25 : 28}
          className={spinning ? "state-spinning" : undefined}
          aria-hidden={true}
        />
      )}
      {title && <strong>{title}</strong>}
      {content && <p>{content}</p>}
      {action}
    </div>
  );
}
