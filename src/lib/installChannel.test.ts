import { describe, expect, it } from "vitest";
import { installChannelGate } from "./installChannel";

describe("installChannelGate", () => {
  it("两路都空闲：不阻塞", () => {
    const gate = installChannelGate(false, false, "snapshot");
    expect(gate).toEqual({ selfBusy: false, blocked: false, blockHint: null });
  });

  it("自身在跑：标记 selfBusy，不算被占用", () => {
    const gate = installChannelGate(true, false, "snapshot");
    expect(gate).toEqual({ selfBusy: true, blocked: false, blockHint: null });
  });

  it("另一路在跑：阻塞并给出带对方名称的提示", () => {
    const gate = installChannelGate(false, true, "snapshot");
    expect(gate.blocked).toBe(true);
    expect(gate.blockHint).toBe("「快照重建」进行中，请等它结束后再试");
  });

  it("反向视角提示对方名称（预设安装占用时）", () => {
    const gate = installChannelGate(false, true, "preset");
    expect(gate.blockHint).toBe("「环境预设安装」进行中，请等它结束后再试");
  });

  it("两路同时在跑（理论不该发生）：以自身在跑为准，不算被占用", () => {
    const gate = installChannelGate(true, true, "snapshot");
    expect(gate.selfBusy).toBe(true);
    expect(gate.blocked).toBe(false);
  });
});
