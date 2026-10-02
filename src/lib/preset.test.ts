import { describe, expect, it } from "vitest";
import {
  applyPresetEdit,
  hasOtherSections,
  parseStoredPresets,
  presetFileName,
  presetNameClash,
  presetToToml,
  replaceToolsSection,
  tomlKey,
  tomlString,
  toolsOf,
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

describe("replaceToolsSection", () => {
  it("只换 [tools] 段，[env] 与注释原样保留", () => {
    const src = '# 我的配置\n[tools]\nnode = "20"\n\n[env]\nEDITOR = "vim"\n';
    expect(replaceToolsSection(src, { node: "22", go: "1.27" })).toBe(
      '# 我的配置\n[tools]\nnode = "22"\ngo = "1.27"\n\n[env]\nEDITOR = "vim"\n',
    );
  });

  it("没有 [tools] 段时追加到末尾", () => {
    const src = '[env]\nEDITOR = "vim"\n';
    expect(replaceToolsSection(src, { node: "20" })).toBe(
      '[env]\nEDITOR = "vim"\n\n[tools]\nnode = "20"\n',
    );
  });

  it("[tools] 位于末尾时正常替换", () => {
    expect(replaceToolsSection('# c\n[tools]\nnode = "20"\n', { node: "22" })).toBe(
      '# c\n[tools]\nnode = "22"\n',
    );
  });

  it("空文本只产出 [tools] 段", () => {
    expect(replaceToolsSection("", { node: "20" })).toBe('[tools]\nnode = "20"\n');
  });

  it("含冒号的工具名加引号", () => {
    expect(replaceToolsSection('[tools]\na = "1"\n', { "npm:prettier": "3" })).toBe(
      '[tools]\n"npm:prettier" = "3"\n',
    );
  });

  it("[tools] 后面紧跟另一个表时不吞掉空行", () => {
    const src = '[tools]\nnode = "20"\n[settings]\nexperimental = true\n';
    expect(replaceToolsSection(src, { node: "22" })).toBe(
      '[tools]\nnode = "22"\n[settings]\nexperimental = true\n',
    );
  });

  it("[tools.x] 子表与 [tools] 段一并重建，不再残留", () => {
    const src =
      '[tools]\nnode = "20"\n\n[tools.python]\nversion = "3.13"\n\n[env]\nEDITOR = "vim"\n';
    expect(replaceToolsSection(src, { node: "22", python: "3.13" })).toBe(
      '[tools]\nnode = "22"\npython = "3.13"\n\n[env]\nEDITOR = "vim"\n',
    );
  });

  it("只有 [tools.x] 子表时替换后转为内联写法", () => {
    const src = '[tools.python]\nversion = "3.13"\n\n[env]\nA = "b"\n';
    expect(replaceToolsSection(src, { python: "3.13" })).toBe(
      '[env]\nA = "b"\n\n[tools]\npython = "3.13"\n',
    );
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
  const list: UserPreset[] = [
    { name: "A", content: '[tools]\nnode = "20"\n', description: "旧描述" },
    { name: "B", content: '[tools]\ngo = "1.27"\n' },
  ];
  const tool = (name: string, version: string) => ({ name, version });

  it("新建：追加一条，不动既有的", () => {
    const next = applyPresetEdit(list, {
      name: "C",
      description: "新",
      tools: [tool("rust", "stable")],
    });
    expect(next).toHaveLength(3);
    expect(next.map((p) => p.name)).toEqual(["A", "B", "C"]);
    expect(next[2].content).toBe('# 由 Z.Env 环境预设生成\n[tools]\nrust = "stable"\n');
    expect(next[2].description).toBe("新");
  });

  it("新建重名：覆盖，不产生两份", () => {
    const next = applyPresetEdit(list, { name: "B", tools: [tool("go", "1.28")] });
    expect(next).toHaveLength(2);
    expect(next.filter((p) => p.name === "B")).toHaveLength(1);
    expect(next.find((p) => p.name === "B")?.content).toBe(
      '# 由 Z.Env 环境预设生成\n[tools]\ngo = "1.28"\n',
    );
  });

  it("修改同名：只换内容，条目数与顺序不变", () => {
    const next = applyPresetEdit(list, {
      name: "A",
      description: "改后",
      tools: [tool("node", "22")],
      original: { name: "A", content: '[tools]\nnode = "20"\n' },
    });
    expect(next.map((p) => p.name)).toEqual(["A", "B"]);
    expect(next[0].content).toBe('[tools]\nnode = "22"\n');
    expect(next[0].description).toBe("改后");
  });

  it("改名：旧名条目消失、新名出现，总数不变且不换位置", () => {
    const next = applyPresetEdit(list, {
      name: "A2",
      tools: [tool("node", "20")],
      original: { name: "A", content: '[tools]\nnode = "20"\n' },
    });
    expect(next).toHaveLength(2);
    expect(next.map((p) => p.name)).toEqual(["A2", "B"]);
  });

  it("修改时保住 [env] 等 [tools] 以外的配置", () => {
    const withEnv =
      '# 我的配置\n[tools]\nnode = "20"\n\n[env]\nEDITOR = "vim"\n_.path = ["/opt/bin"]\n';
    const next = applyPresetEdit([{ name: "A", content: withEnv }], {
      name: "A",
      tools: [tool("node", "22")],
      original: { name: "A", content: withEnv },
    });
    expect(next[0].content).toContain('node = "22"');
    expect(next[0].content).toContain('EDITOR = "vim"');
    expect(next[0].content).toContain('_.path = ["/opt/bin"]');
    expect(next[0].content).toContain("# 我的配置");
  });

  it("描述被清空时不残留旧描述", () => {
    const next = applyPresetEdit(list, {
      name: "A",
      description: "   ",
      tools: [tool("node", "20")],
      original: { name: "A", content: '[tools]\nnode = "20"\n' },
    });
    expect(next[0].description).toBeUndefined();
    expect("description" in next[0]).toBe(false);
  });

  it("名称与描述两端空白会被裁掉", () => {
    const next = applyPresetEdit([], {
      name: "  X  ",
      description: " 说明 ",
      tools: [tool("node", "20")],
    });
    expect(next[0].name).toBe("X");
    expect(next[0].description).toBe("说明");
  });

  it("删掉全部工具后只剩空 [tools] 段，不留多余空行", () => {
    const next = applyPresetEdit([], { name: "empty", tools: [] });
    expect(next[0].content).toBe("# 由 Z.Env 环境预设生成\n[tools]\n");
  });
});
