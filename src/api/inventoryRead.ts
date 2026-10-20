let tail: Promise<unknown> | null = null;
/** 清单与项目核对共用后端读取槽；切页取消后等待实际退出，再分派下一项读取。 */
export function inventoryRead<T>(start: () => Promise<T>): Promise<T> {
  const result = tail ? tail.then(start, start) : start();
  const settled = result.then(
    () => undefined,
    () => undefined,
  );
  tail = settled;
  void settled.then(() => {
    if (tail === settled) tail = null;
  });
  return result;
}
