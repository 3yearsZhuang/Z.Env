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
  return `# 由 Z.Env 环境预设生成\n[tools]\n${rows}${rows ? "\n" : ""}`;
}

/** 预设的工具数组 → 工具表（供 presetToToml 使用） */
export function toolsOf(tools: PresetTool[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of tools) out[t.name] = t.version;
  return out;
}

/** `[tools]` 段文本（不含结尾换行） */
function toolsBlock(tools: Record<string, string>): string {
  const rows = Object.entries(tools)
    .map(([k, v]) => `${tomlKey(k)} = "${tomlString(v)}"`)
    .join("\n");
  return rows ? `[tools]\n${rows}` : "[tools]";
}

/** `[tools.<name>]` 子表头（mise 工具表的另一种写法） */
function isToolsSubTable(line: string): boolean {
  const t = line.trim();
  return t.startsWith("[tools.") && t.endsWith("]");
}

/** 删除所有 `[tools.<name>]` 子表块：其工具已并入新的 `[tools]` 段，留着会造成键重复 */
function dropToolsSubTables(lines: string[]): string[] {
  const out: string[] = [];
  let skipping = false;
  for (const l of lines) {
    const t = l.trim();
    if (t.startsWith("[")) {
      // 任何表头都终结上一个块；`[tools.*]` 自身进入跳过态
      skipping = isToolsSubTable(t);
    }
    if (skipping && t !== "") continue; // 子表头与属性行丢弃，空行留待统一压缩
    out.push(l);
  }
  return out;
}

/**
 * 用新的工具表替换 mise.toml 文本里的 `[tools]` 段，其余内容（`[env]`、`_.path`、注释等）
 * 原样保留——用户在编辑器里存下的预设可能带这些配置，改写时不能丢。
 * 原文本没有 `[tools]` 段时追加到末尾。
 * `[tools.<name>]` 子表与 `[tools]` 段表达同一份工具，替换后一并重建为内联写法，
 * 否则新内联键与旧子表同名会构成非法 TOML（键重复）。
 */
export function replaceToolsSection(toml: string, tools: Record<string, string>): string {
  const block = toolsBlock(tools);
  const lines = toml.split("\n");
  const start = lines.findIndex((l) => l.trim() === "[tools]");
  if (start < 0) {
    const head = dropToolsSubTables(lines).join("\n").replace(/^\n+/, "").replace(/\s+$/, "");
    return head ? `${head}\n\n${block}\n` : `${block}\n`;
  }
  // 段范围：`[tools]` 行到下一个顶层表头；末尾空行留给后面，保住与下一段之间的分隔
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].trim().startsWith("[")) {
      end = i;
      break;
    }
  }
  while (end > start + 1 && lines[end - 1].trim() === "") end--;
  return dropToolsSubTables([...lines.slice(0, start), ...block.split("\n"), ...lines.slice(end)])
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/^\n+/, "");
}

/** 文本是否含 `[tools]` 以外的顶层表（这些段在编辑时会被保留，仅用于提示） */
export function hasOtherSections(toml: string): boolean {
  return toml.split("\n").some((l) => {
    const t = l.trim();
    return t.startsWith("[") && t !== "[tools]" && !isToolsSubTable(t);
  });
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

/** 一次预设编辑（新建或修改）的输入 */
export interface PresetEdit {
  name: string;
  description?: string;
  tools: PresetTool[];
  /** 修改既有预设时给出原始条目；新建时省略 */
  original?: { name: string; content: string };
}

/**
 * 同名覆盖时**原地替换**（卡片顺序不变），新名字追加到列表末尾。
 * `save` 直存与导入收录共用——过滤重名再追加的写法会把覆盖的条目挪到列表末尾，
 * 与 applyPresetEdit 的原地替换行为不一致。
 */
export function upsertPreset(list: UserPreset[], entry: UserPreset): UserPreset[] {
  const at = list.findIndex((p) => p.name === entry.name);
  if (at < 0) return [...list, entry];
  const next = [...list];
  next[at] = entry;
  return next;
}

/**
 * 保存前判断是否会覆盖「别人」：编辑自身同名不算冲突，改名撞上别的预设才算。
 * 返回被撞上的那条，没有则 undefined。
 */
export function presetNameClash(
  list: UserPreset[],
  name: string,
  originalName?: string,
): UserPreset | undefined {
  return list.find((p) => p.name === name && p.name !== originalName);
}

/**
 * 应用一次预设编辑，返回新的预设库列表。三种情况在同一处处理：
 * 新建（追加）、同名覆盖、改名（移除旧名条目）——调用方只需写入一次，
 * 不会出现 save/remove 连调时各自基于同一份过期快照互相覆盖的问题。
 *
 * 修改既有条目时**原地替换**：卡片顺序不变，改完不会跳到列表末尾。
 */
export function applyPresetEdit(list: UserPreset[], edit: PresetEdit): UserPreset[] {
  const name = edit.name.trim();
  const tools = toolsOf(edit.tools);
  // 修改只替换 [tools] 段，保住 [env] / _.path 等既有配置；新建直接生成
  const content = edit.original
    ? replaceToolsSection(edit.original.content, tools)
    : presetToToml(tools);
  const description = edit.description?.trim();
  const entry: UserPreset = { name, content, ...(description ? { description } : {}) };

  // 原地替换：命中原始条目（改名时靠 original.name 找）或同名条目
  const at = list.findIndex((p) => p.name === edit.original?.name || p.name === name);
  if (at >= 0) {
    const next = [...list];
    next[at] = entry;
    // 极端情况：改名后与另一条重名，去掉那条，保证名字唯一
    return next.filter((p, i) => i === at || p.name !== name);
  }
  return [...list, entry];
}
