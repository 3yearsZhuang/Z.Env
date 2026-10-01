// 环境预设库区块：内置技术栈卡片 + 我的预设卡片 + 新建 / 导入 / 编辑 / 导出。
// 「项目配置」页用它把预设载入项目，「环境」页用它把预设装到整机；
// 两页共用同一个 localStorage 预设库（经 useUserPresets）。
import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { errorMessage, presetImport, presetParseTools, type PresetFile } from "../api";
import { PRESETS } from "../data/presets";
import { hasOtherSections, presetToToml, replaceToolsSection, toolsOf } from "../lib/preset";
import type { UserPresetsApi } from "../lib/useUserPresets";
import PresetEditorDialog, { type PresetDraft } from "./PresetEditorDialog";

/** 卡片被点击后交给页面的载荷：toml 为 mise.toml 文本，工具清单由页面按需解析 */
export interface PresetChoice {
  name: string;
  description: string;
  toml: string;
}

interface Props {
  /** 页面级 useUserPresets() 实例（同页只能有一份，故由页面持有后传入） */
  presets: UserPresetsApi;
  /** 「我的预设」无描述时的卡片副标题，说明点击后会发生什么 */
  useHint: string;
  /** 点击卡片 */
  onUse: (choice: PresetChoice) => void;
  /** 点击卡片上的导出按钮 */
  onExport: (choice: PresetChoice) => void;
  /** 预设文件已导入并收录进预设库 */
  onImported: (file: PresetFile) => void;
  onError: (message: string) => void;
}

export default function PresetLibrary({
  presets,
  useHint,
  onUse,
  onExport,
  onImported,
  onError,
}: Props) {
  const { userPresets, persist, remove, remember } = presets;

  // 编辑器草稿：null = 关闭；isNew 区分新建与修改
  const [draft, setDraft] = useState<PresetDraft | null>(null);

  async function handleImport() {
    const picked = await open({
      multiple: false,
      title: "选择环境预设文件",
      filters: [{ name: "Z.Env 环境预设", extensions: ["toml"] }],
    });
    if (!picked || typeof picked !== "string") return;
    try {
      const file = await presetImport(picked);
      if (!remember(file)) return; // 用户取消了同名覆盖
      onImported(file);
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  /** 新建：从一份空白草稿开始 */
  function openCreate() {
    setDraft({ isNew: true, name: "", description: "", tools: [{ name: "", version: "" }] });
  }

  /** 以某份预设为模板另存（内置卡片点「编辑」走这里） */
  function openCopyAs(
    name: string,
    description: string,
    tools: { name: string; version: string }[],
  ) {
    setDraft({
      isNew: true,
      name: `${name} 副本`,
      description,
      tools: tools.length > 0 ? tools : [{ name: "", version: "" }],
    });
  }

  /** 修改已有预设：先把它的 [tools] 解析出来填进表单 */
  async function openEdit(name: string, content: string, description: string) {
    try {
      const tools = await presetParseTools(content);
      setDraft({
        isNew: false,
        name,
        description,
        tools: tools.length > 0 ? tools : [{ name: "", version: "" }],
        originalContent: content,
        originalName: name,
      });
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  /**
   * 落库。三种情况一次写入，避免 save/remove 连调时各自基于同一份过期快照互相覆盖：
   * 新建 / 同名覆盖 / 改名（删掉旧名条目）。
   */
  function handleEditorSave(values: {
    name: string;
    description: string;
    tools: { name: string; version: string }[];
  }) {
    if (!draft) return;
    const { name, description, tools } = values;
    const clash = userPresets.find((p) => p.name === name && p.name !== draft.originalName);
    if (clash && !window.confirm(`已存在同名预设「${name}」，覆盖它吗？`)) return;

    // 修改时只替换 [tools] 段，保住 [env] 之类的既有配置；新建则直接生成
    const content = draft.originalContent
      ? replaceToolsSection(draft.originalContent, toolsOf(tools))
      : presetToToml(toolsOf(tools));

    persist([
      ...userPresets.filter((p) => p.name !== name && p.name !== draft.originalName),
      { name, content, ...(description ? { description } : {}) },
    ]);
    setDraft(null);
  }

  return (
    <>
      <section className="panel">
        <div className="preset-head">
          <div>
            <h2 className="panel-title">环境预设</h2>
            <p className="setting-desc">选择技术栈一键生成全套环境，也可导入他人分享的预设文件</p>
          </div>
          <div className="preset-head-acts">
            <button className="btn" onClick={openCreate}>
              新建预设
            </button>
            <button className="btn" onClick={handleImport}>
              导入预设
            </button>
          </div>
        </div>
        <div className="preset-grid">
          {PRESETS.map((p) => {
            const choice: PresetChoice = {
              name: p.name,
              description: p.desc,
              toml: presetToToml(p.tools),
            };
            return (
              <div className="preset-card-wrap" key={p.id}>
                <button className="preset-card" onClick={() => onUse(choice)}>
                  <span className="preset-name">{p.name}</span>
                  <span className="preset-desc">{p.desc}</span>
                  <span className="preset-tools">
                    {Object.keys(p.tools).slice(0, 4).join(" · ")}
                  </span>
                </button>
                <div className="preset-acts">
                  <button
                    className="preset-act"
                    title="以这份预设为模板新建一份"
                    onClick={() =>
                      openCopyAs(
                        p.name,
                        p.desc,
                        Object.entries(p.tools).map(([name, version]) => ({ name, version })),
                      )
                    }
                  >
                    ✎
                  </button>
                  <button
                    className="preset-act"
                    title="导出为可分享的预设文件"
                    onClick={() => onExport(choice)}
                  >
                    ⤓
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </section>

      {userPresets.length > 0 && (
        <section className="panel">
          <h2 className="panel-title">我的预设</h2>
          <div className="preset-grid">
            {userPresets.map((up) => {
              const choice: PresetChoice = {
                name: up.name,
                description: up.description ?? "",
                toml: up.content,
              };
              return (
                <div className="preset-card-wrap" key={up.name}>
                  <button className="preset-card" onClick={() => onUse(choice)}>
                    <span className="preset-name">{up.name}</span>
                    <span className="preset-tools">{up.description ?? useHint}</span>
                  </button>
                  <div className="preset-acts">
                    <button
                      className="preset-act"
                      title="修改这份预设"
                      onClick={() => void openEdit(up.name, up.content, up.description ?? "")}
                    >
                      ✎
                    </button>
                    <button
                      className="preset-act"
                      title="导出为可分享的预设文件"
                      onClick={() => onExport(choice)}
                    >
                      ⤓
                    </button>
                    <button
                      className="preset-act danger"
                      title="删除该预设"
                      onClick={() => remove(up.name)}
                    >
                      ✕
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      )}

      <PresetEditorDialog
        draft={draft}
        hasExtraSections={draft?.originalContent ? hasOtherSections(draft.originalContent) : false}
        onClose={() => setDraft(null)}
        onSave={handleEditorSave}
      />
    </>
  );
}
