import { toolCatalog, type CatalogTool, type ToolCatalog } from "../api/toolCatalog";

/** 统一工具目录的进程级缓存。
 *
 * `tool_catalog` 是纯只读命令，同一会话内不会变；软件支持列表与（原先的）能力台账
 * 各自读一次就会重复请求。这里合并并发请求并在会话内复用。
 */
let cached: ToolCatalog | null = null;
let inflight: Promise<ToolCatalog> | null = null;

export function readToolCatalog(options: { force?: boolean } = {}): Promise<ToolCatalog> {
  if (!options.force && cached) return Promise.resolve(cached);
  if (inflight) return inflight;
  inflight = toolCatalog().then(
    (catalog) => {
      cached = catalog;
      inflight = null;
      return catalog;
    },
    (error: unknown) => {
      inflight = null;
      throw error;
    },
  );
  return inflight;
}

export function clearToolCatalogCache(): void {
  cached = null;
  inflight = null;
}

/** 按显式 toolId 与别名解析目录项；不做包名或可执行文件名的猜测。 */
export function findCatalogTool(catalog: ToolCatalog | null, name: string): CatalogTool | null {
  if (!catalog) return null;
  const key = name.trim().toLowerCase();
  return (
    catalog.tools.find(
      (tool) =>
        tool.definition.toolId.toLowerCase() === key ||
        tool.definition.aliases.some((alias) => alias.toLowerCase() === key),
    ) ?? null
  );
}
