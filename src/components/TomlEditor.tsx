// CodeMirror 体积较大且仅项目配置页使用：独立成模块由 ProjectsView 懒加载，
// 使编辑器、StreamLanguage 与 TOML 语言包整体落在异步 chunk，主 bundle 不再背上这部分体积。
// 这里只组装真正需要的扩展（行号、历史、括号匹配、语法高亮），不引入 basicSetup
// 全家桶（autocomplete / lint / search / 多光标指令等），chunk 体积约为原来的三分之一。
import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import {
  StreamLanguage,
  bracketMatching,
  defaultHighlightStyle,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";

export interface TomlEditorProps {
  value: string;
  onChange: (value: string) => void;
}

/** 主题直接吃设计令牌：明暗由 <html>.dark 驱动，调用方不再需要传 dark。 */
const zenvTheme = EditorView.theme({
  "&": {
    height: "380px",
    fontSize: "var(--fs-base)",
    backgroundColor: "var(--card)",
    color: "var(--foreground)",
    border: "1px solid var(--border)",
    borderRadius: "var(--r-md)",
    overflow: "hidden",
  },
  "&.cm-focused": { outline: "none", borderColor: "var(--primary)" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.6" },
  ".cm-content": { padding: "10px 0" },
  ".cm-gutters": {
    backgroundColor: "transparent",
    color: "var(--muted-foreground)",
    border: "none",
  },
  ".cm-activeLine": { backgroundColor: "var(--surface-2)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent" },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
    backgroundColor: "var(--accent)",
  },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
});

export default function TomlEditor({ value, onChange }: TomlEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  // 回调走 ref：编辑器实例只在挂载时创建一次，避免每次渲染重建并丢光标。
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    const instance = new EditorView({
      parent,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          highlightActiveLine(),
          history(),
          drawSelection(),
          indentOnInput(),
          bracketMatching(),
          StreamLanguage.define(toml),
          syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
          keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
          zenvTheme,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChangeRef.current(update.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = instance;
    return () => {
      instance.destroy();
      view.current = null;
    };
    // 仅在挂载时创建；value 同步与回调分别由下面的 effect / ref 负责。
  }, []);

  // 外部改动（切换项目、载入文件、取消编辑）时替换文档内容，保留编辑器实例。
  useEffect(() => {
    const instance = view.current;
    if (!instance) return;
    const current = instance.state.doc.toString();
    if (current !== value) {
      instance.dispatch({ changes: { from: 0, to: current.length, insert: value } });
    }
  }, [value]);

  return <div className="toml-editor" ref={host} />;
}
