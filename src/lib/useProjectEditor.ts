import { useRef, useState } from "react";
import { readProjectConfig, writeProjectConfig, installAllProject } from "../api";

/** 路径、原始版本与草稿组成同一个会话；慢请求不能覆盖后载入的项目。 */
export function useProjectEditor() {
  const [state, setState] = useState({
    path: "",
    content: "",
    original: null as string | null,
    target: null as string | null,
  });
  const generation = useRef(0);
  const revision = useRef(0);
  function setPath(path: string) {
    revision.current++;
    generation.current++;
    setState((s) => ({ ...s, path, target: null }));
  }
  function setContent(value: React.SetStateAction<string>) {
    revision.current++;
    setState((s) => ({ ...s, content: typeof value === "function" ? value(s.content) : value }));
  }
  async function load(path: string) {
    revision.current++;
    const request = ++generation.current;
    const doc = await readProjectConfig(path);
    if (request !== generation.current) return false;
    revision.current++;
    setState({ path: doc.path, target: doc.path, content: doc.content, original: doc.original });
    return true;
  }
  async function save(install = false) {
    if (!state.target) throw new Error("请先载入项目，再保存或安装");
    const request = generation.current;
    let result;
    try {
      result = await (install ? installAllProject : writeProjectConfig)(
        state.target,
        state.content,
        state.original,
      );
    } catch (error) {
      // 配置可能已保存而安装失败；确认磁盘内容一致后更新基线，允许直接重试。
      if (install) {
        const current = await readProjectConfig(state.target).catch(() => null);
        if (request === generation.current && current?.original === state.content) {
          setState((s) => ({ ...s, original: state.content }));
        }
      }
      throw error;
    }
    if (request === generation.current) setState((s) => ({ ...s, original: state.content }));
    return result;
  }
  async function transform(edit: (content: string) => Promise<string>) {
    const expected = revision.current;
    const content = await edit(state.content);
    if (expected !== revision.current) throw new Error("配置已改变，请重新执行此操作");
    setContent(content);
  }
  return { ...state, setPath, setContent, load, save, transform };
}
