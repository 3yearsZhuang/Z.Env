import { describe, expect, it } from "vitest";
import { PRESETS } from "./presets";

describe("内置环境预设", () => {
  it("id 唯一", () => {
    const ids = PRESETS.map((p) => p.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("每项都有名称、描述与至少一个工具", () => {
    for (const p of PRESETS) {
      expect(p.name.trim()).not.toBe("");
      expect(p.desc.trim()).not.toBe("");
      expect(Object.keys(p.tools).length).toBeGreaterThan(0);
    }
  });

  it("工具版本均为非空字符串", () => {
    for (const p of PRESETS) {
      for (const [tool, version] of Object.entries(p.tools)) {
        expect(tool.trim()).not.toBe("");
        expect(version.trim()).not.toBe("");
      }
    }
  });
});
