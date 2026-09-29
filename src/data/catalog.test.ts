import { describe, expect, it } from "vitest";
import { CAT_KEYS_ALL, KNOWN_RUNTIMES, SOFTWARE, categoryOf } from "./catalog";

describe("catalog", () => {
  it("categoryOf 未收录的工具归入“其他”", () => {
    expect(categoryOf("definitely-not-a-tool")).toBe("其他");
  });

  it("KNOWN_RUNTIMES 与 SOFTWARE 不重叠（软件由支持列表页单独展示）", () => {
    const soft = new Set(SOFTWARE.map((s) => s.name));
    expect(KNOWN_RUNTIMES.filter((n) => soft.has(n))).toEqual([]);
  });

  it("分类标签列表以“其他”兜底", () => {
    expect(CAT_KEYS_ALL[CAT_KEYS_ALL.length - 1]).toBe("其他");
  });
});
