import { invoke } from "@tauri-apps/api/core";
import type { Platform, ProviderId, ProviderPackage, ToolDefinition } from "./inventory";
export type EntryState = "integrated" | "notIntegrated" | "unsupported";
export interface EntrySupport {
  state: EntryState;
  reason: string;
}
export interface CatalogChannel {
  providerId: ProviderId;
  packages: ProviderPackage[];
  inventory: EntrySupport;
  installPlan: EntrySupport;
  uninstallPlan: EntrySupport;
  bindingPlan: EntrySupport;
}
export interface CatalogTool {
  definition: ToolDefinition;
  category: "runtime" | "cli";
  description: string;
  documentationUrl: string;
  projectCheck: EntrySupport;
  channels: CatalogChannel[];
}
export interface ToolCatalog {
  schemaVersion: 1;
  platform: Platform;
  coverageNote: string;
  tools: CatalogTool[];
}
const providers = ["mise", "brew", "nvm", "pyenv", "uv", "path"];
const record = (v: unknown): v is Record<string, unknown> =>
  Boolean(v && typeof v === "object" && !Array.isArray(v));
const text = (v: unknown): v is string => typeof v === "string" && v.length > 0 && v.length <= 4096;
const strings = (v: unknown) => Array.isArray(v) && v.length <= 32 && v.every(text);
function support(v: unknown) {
  return (
    record(v) &&
    ["integrated", "notIntegrated", "unsupported"].includes(String(v.state)) &&
    text(v.reason)
  );
}
/** 拒绝未来格式及不完整能力账本，不将读取失败转换成可操作的空目录。 */
export function validateToolCatalog(value: unknown): ToolCatalog {
  const fail = () => {
    throw new Error("工具目录格式不受支持或能力记录不完整");
  };
  if (
    !record(value) ||
    value.schemaVersion !== 1 ||
    !["macos", "windows", "linux"].includes(String(value.platform)) ||
    !text(value.coverageNote) ||
    !Array.isArray(value.tools) ||
    value.tools.length < 1 ||
    value.tools.length > 64
  )
    return fail();
  const ids = new Set<string>();
  const aliases = new Map<string, string>();
  for (const item of value.tools) {
    if (!record(item) || !record(item.definition)) return fail();
    const d = item.definition;
    if (
      !text(d.toolId) ||
      !/^[a-z][a-z0-9-]{0,63}$/.test(d.toolId) ||
      ids.has(d.toolId) ||
      !text(d.displayName) ||
      !strings(d.aliases) ||
      !strings(d.executableNames) ||
      !["runtime", "cli"].includes(String(item.category)) ||
      !text(item.description) ||
      !support(item.projectCheck) ||
      !text(item.documentationUrl) ||
      !Array.isArray(item.channels) ||
      item.channels.length < 1 ||
      item.channels.length > 8
    )
      return fail();
    ids.add(d.toolId);
    for (const raw of [d.toolId, ...(d.aliases as string[])]) {
      const key = raw.toLowerCase();
      if (aliases.has(key) && aliases.get(key) !== d.toolId) return fail();
      aliases.set(key, d.toolId);
    }
    try {
      const url = new URL(item.documentationUrl);
      if (url.protocol !== "https:" || url.username || url.password) return fail();
    } catch {
      return fail();
    }
    const channelIds = new Set<string>();
    for (const channel of item.channels) {
      if (
        !record(channel) ||
        !providers.includes(String(channel.providerId)) ||
        channelIds.has(String(channel.providerId)) ||
        ![channel.inventory, channel.installPlan, channel.uninstallPlan, channel.bindingPlan].every(
          support,
        ) ||
        !Array.isArray(channel.packages) ||
        channel.packages.length > 16
      )
        return fail();
      channelIds.add(String(channel.providerId));
      for (const pkg of channel.packages)
        if (
          !record(pkg) ||
          pkg.providerId !== channel.providerId ||
          pkg.platform !== value.platform ||
          !text(pkg.packageId) ||
          !["runtime", "formula", "cask", "executable"].includes(String(pkg.kind))
        )
          return fail();
    }
  }
  return value as unknown as ToolCatalog;
}
export async function toolCatalog(): Promise<ToolCatalog> {
  return validateToolCatalog(await invoke("tool_catalog"));
}
