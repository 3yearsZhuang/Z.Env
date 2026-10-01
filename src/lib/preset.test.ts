import { describe, expect, it } from "vitest";
import {
  parseStoredPresets,
  presetFileName,
  presetToToml,
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
