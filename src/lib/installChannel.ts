// 安装通道互斥:「环境预设 → 装到整机」与「快照重建」都走同一条 mise 安装通道
// （mise:install-progress 流式事件）。并发时两份日志互相串台，两条 mise install
// 也可能互踩——同一时刻只允许一路占用通道。

export type InstallChannelSource = "preset" | "snapshot";

/** 各入口的用户可读名称，用于占用提示 */
const SOURCE_LABEL: Record<InstallChannelSource, string> = {
  preset: "环境预设安装",
  snapshot: "快照重建",
};

export interface InstallChannelGate {
  /** 本入口自身在跑 */
  selfBusy: boolean;
  /** 通道被另一入口占用：本入口应禁用安装入口 */
  blocked: boolean;
  /** 被占用时给用户的原因文案；其余情况为 null */
  blockHint: string | null;
}

/** 纯函数：按本入口视角判定通道占用状态（互斥判定只有这一个实现，组件直接消费） */
export function installChannelGate(
  selfBusy: boolean,
  otherBusy: boolean,
  other: InstallChannelSource,
): InstallChannelGate {
  const blocked = !selfBusy && otherBusy;
  return {
    selfBusy,
    blocked,
    blockHint: blocked ? `「${SOURCE_LABEL[other]}」进行中，请等它结束后再试` : null,
  };
}
