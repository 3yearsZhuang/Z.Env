/** Test-only materialization of the shared JSON pointer mutations. */
export function materializeRecipeFixture(
  bases: Record<string, unknown>,
  sample: { base: string; changes: { op: string; pointer: string; value?: unknown }[] },
): unknown {
  let result = structuredClone(bases[sample.base]);
  for (const change of sample.changes) {
    if (change.pointer === "") {
      if (change.op !== "set") throw new Error("Invalid fixture root operation");
      result = structuredClone(change.value);
      continue;
    }
    const keys = change.pointer
      .slice(1)
      .split("/")
      .map((key) => key.replace(/~1/g, "/").replace(/~0/g, "~"));
    let parent = result as Record<string, unknown>;
    for (const key of keys.slice(0, -1)) parent = parent[key] as Record<string, unknown>;
    const key = keys[keys.length - 1];
    if (change.op === "remove") delete parent[key];
    else if (change.op === "set") parent[key] = structuredClone(change.value);
    else throw new Error("Unsupported fixture operation");
  }
  return result;
}
