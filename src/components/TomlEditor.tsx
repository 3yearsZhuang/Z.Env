// CodeMirror 体积较大且仅项目配置页使用：独立成模块由 ProjectsView 懒加载，
// 使编辑器、StreamLanguage 与 TOML 语言包整体落在异步 chunk，主 bundle 不再背上这部分体积。
import CodeMirror from "@uiw/react-codemirror";
import { StreamLanguage } from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";

export interface TomlEditorProps {
  value: string;
  dark: boolean;
  onChange: (value: string) => void;
}

export default function TomlEditor({ value, dark, onChange }: TomlEditorProps) {
  return (
    <CodeMirror
      value={value}
      height="380px"
      theme={dark ? "dark" : "light"}
      extensions={[StreamLanguage.define(toml)]}
      basicSetup={{ foldGutter: false, searchKeymap: false }}
      onChange={onChange}
      style={{ fontSize: 13 }}
    />
  );
}
