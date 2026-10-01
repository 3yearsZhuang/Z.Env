// 环境预设的纯函数工具：mise.toml 文本与工具表的互转、导出文件名清理。
// 与「环境快照」的整机导出不同，预设只承载工具与版本。
import type { PresetTool } from "../api";

/** TOML 裸键合法字符；`npm:prettier` 这类含冒号的后端名必须加引号 */
const BARE_KEY = /^[A-Za-z0-9_-]+$/;

/** 工具名转 TOML 键：合法裸键保持原样，否则加引号 */
export function tomlKey(name: string): string {
  return BARE_KEY.test(name) ? name : `"${tomlString(name)}"`;
}

/** 转义为 TOML 基本字符串内容（反斜杠与双引号） */
export function tomlString(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

/** 工具表 → mise.toml 文本（内置预设与导入预设落盘时共用） */
export function presetToToml(tools: Record<string, string>): string {
  const rows = Object.entries(tools)
    .map(([k, v]) => `${tomlKey(k)} = "${tomlString(v)}"`)
    .join("\n");
  return `# 由 Z.Env 环境预设生成\n[tools]\n${rows}\n`;
}

/** 预设的工具数组 → 工具表（供 presetToToml 使用） */
export function toolsOf(tools: PresetTool[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of tools) out[t.name] = t.version;
  return out;
}

/** 生成安全的导出文件名（去掉路径分隔符与文件系统不友好字符） */
export function presetFileName(name: string): string {
  const cleaned = name
    .trim()
    .replace(/[/\\:*?"<>|]/g, "-")
    .replace(/\s+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^[-.]+|[-.]+$/g, "");
  return `${cleaned || "zenv-preset"}.zenv.toml`;
}

/** 用户预设（localStorage 存储形态）：content 为 mise.toml 文本 */
export interface UserPreset {
  name: string;
  content: string;
  description?: string;
}

/** 解析本地存储的用户预设：容忍旧数据（无 description）与损坏项 */
export function parseStoredPresets(raw: string | null): UserPreset[] {
  if (!raw) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const out: UserPreset[] = [];
  for (const item of parsed) {
    if (!item || typeof item !== "object") continue;
    const { name, content, description } = item as Partial<UserPreset>;
    if (typeof name !== "string" || !name.trim()) continue;
    if (typeof content !== "string") continue;
    out.push({
      name,
      content,
      ...(typeof description === "string" && description ? { description } : {}),
    });
  }
  return out;
}
