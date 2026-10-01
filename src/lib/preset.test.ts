import { describe, expect, it } from "vitest";
import {
  hasOtherSections,
  parseStoredPresets,
  presetFileName,
  presetToToml,
  replaceToolsSection,
  tomlKey,
  tomlString,
  toolsOf,
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
});
