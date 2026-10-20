import { describe, expect, it } from "vitest";
import {
  applyPresetEdit,
  hasOtherSections,
  parseStoredPresets,
  presetFileName,
  presetNameClash,
  presetToToml,
  tomlKey,
  tomlString,
  toolsOf,
  upsertPreset,
  type UserPreset,
} from "./preset";

describe("tomlKey", () => {
  it("合法裸键保持原样", () => {
    expect(tomlKey("node")).toBe("node");
    expect(tomlKey("go-fumpt")).toBe("go-fumpt");
    expect(tomlKey("python3_12")).toBe("python3_12");
  });

  it("含冒号的后端名加引号（裸键不允许冒号）", () => {
    expect(tomlKey("npm:prettier")).toBe('"npm:prettier"');
    expect(tomlKey("cargo:ripgrep")).toBe('"cargo:ripgrep"');
  });
});

describe("tomlString", () => {
  it("转义反斜杠与双引号", () => {
    expect(tomlString('say "hi"')).toBe('say \\"hi\\"');
    expect(tomlString("a\\b")).toBe("a\\\\b");
  });
});

describe("presetToToml", () => {
  it("输出 [tools] 段并保留结尾换行", () => {
    const out = presetToToml({ node: "20" });
    expect(out).toBe('# 由 Z.Env 环境预设生成\n[tools]\nnode = "20"\n');
  });

  it("按需引用键，保持常见工具可读", () => {
    const out = presetToToml({ node: "20", "npm:prettier": "3" });
    expect(out).toContain('node = "20"');
    expect(out).toContain('"npm:prettier" = "3"');
  });
});

describe("toolsOf", () => {
  it("预设工具数组转工具表", () => {
    expect(
      toolsOf([
        { name: "node", version: "20" },
        { name: "go", version: "1.27" },
      ]),
    ).toEqual({ node: "20", go: "1.27" });
  });
});

describe("presetFileName", () => {
  it("补上 .zenv.toml 后缀", () => {
    expect(presetFileName("Node 前端")).toBe("Node-前端.zenv.toml");
  });

  it("清理路径分隔符与非法字符", () => {
    expect(presetFileName("../a/b:c*d")).toBe("a-b-c-d.zenv.toml");
  });

  it("空名回退为 zenv-preset", () => {
    expect(presetFileName("   ")).toBe("zenv-preset.zenv.toml");
  });
});

describe("parseStoredPresets", () => {
  it("读取正常条目并保留 description", () => {
    const raw = JSON.stringify([
      { name: "a", content: '[tools]\nnode = "20"\n', description: "前端" },
    ]);
    expect(parseStoredPresets(raw)).toEqual([
      { name: "a", content: '[tools]\nnode = "20"\n', description: "前端" },
    ]);
  });

  it("兼容旧数据（无 description）", () => {
    const raw = JSON.stringify([{ name: "old", content: "x" }]);
    expect(parseStoredPresets(raw)).toEqual([{ name: "old", content: "x" }]);
  });

  it("丢弃损坏项与非数组", () => {
    const raw = JSON.stringify([{ name: "ok", content: "x" }, { name: "缺 content" }, null, 42]);
    expect(parseStoredPresets(raw)).toEqual([{ name: "ok", content: "x" }]);
    expect(parseStoredPresets(JSON.stringify({ a: 1 }))).toEqual([]);
  });

  it("非法 JSON 与空值返回空数组", () => {
    expect(parseStoredPresets("{ not json")).toEqual([]);
    expect(parseStoredPresets(null)).toEqual([]);
  });
});

describe("hasOtherSections", () => {
  it("只有 [tools] 时为 false", () => {
    expect(hasOtherSections('[tools]\nnode = "20"\n')).toBe(false);
  });

  it("含 [env] 等其它表时为 true", () => {
    expect(hasOtherSections('[tools]\nnode = "20"\n\n[env]\nA = "b"\n')).toBe(true);
  });

  it("注释里的方括号不算表", () => {
    expect(hasOtherSections('# 见 [文档]\n[tools]\nnode = "20"\n')).toBe(false);
  });

  it("[tools.x] 子表属于工具段，不算「其它配置」", () => {
    expect(hasOtherSections('[tools]\nnode = "20"\n\n[tools.python]\nversion = "3"\n')).toBe(false);
  });
});

describe("presetNameClash", () => {
  const list: UserPreset[] = [
    { name: "A", content: "[tools]\n" },
    { name: "B", content: "[tools]\n" },
  ];

  it("撞上别人的名字算冲突", () => {
    expect(presetNameClash(list, "B", "A")?.name).toBe("B");
    expect(presetNameClash(list, "B")).toBeDefined();
  });

  it("编辑自身、名字没变时不算冲突", () => {
    expect(presetNameClash(list, "A", "A")).toBeUndefined();
  });

  it("新名字没人用则无冲突", () => {
    expect(presetNameClash(list, "C", "A")).toBeUndefined();
  });
});

describe("applyPresetEdit", () => {
  it("使用后端验证后的内容，重命名保持位置且去重", () => {
    const list: UserPreset[] = [
      { name: "A", content: "old" },
      { name: "B", content: "other" },
    ];
    const validated = "[tools] # pinned\nnode = '22'\n[env]\nA='中文'\n";
    const result = applyPresetEdit(
      list,
      { name: "B", description: " changed ", tools: [], original: { name: "A", content: "old" } },
      validated,
    );
    expect(result).toEqual([{ name: "B", description: "changed", content: validated }]);
  });
  it("新建追加，清空描述不会留下旧值", () => {
    const list: UserPreset[] = [{ name: "A", content: "old", description: "old" }];
    expect(applyPresetEdit(list, { name: "C", tools: [] }, "new").map((p) => p.name)).toEqual([
      "A",
      "C",
    ]);
    expect(applyPresetEdit(list, { name: "A", tools: [] }, "new")[0]).toEqual({
      name: "A",
      content: "new",
    });
  });
});

describe("upsertPreset", () => {
  const a: UserPreset = { name: "a", content: "[tools]" };
  const b: UserPreset = { name: "b", content: '[tools]\nnode = "20"' };

  it("新名字追加到列表末尾", () => {
    const next = upsertPreset([a], b);
    expect(next).toEqual([a, b]);
  });

  it("同名覆盖原地替换，卡片顺序不变", () => {
    const next = upsertPreset([a, b], { name: "a", content: '[tools]\ngo = "1.27"' });
    expect(next.map((p) => p.name)).toEqual(["a", "b"]);
    expect(next[0].content).toBe('[tools]\ngo = "1.27"');
  });

  it("覆盖只改动目标条目，其余条目原样保留", () => {
    const next = upsertPreset([a, b], { name: "b", content: "x" });
    expect(next[0]).toBe(a);
    expect(next[1]).not.toBe(b);
    expect(next[1].content).toBe("x");
  });
});
