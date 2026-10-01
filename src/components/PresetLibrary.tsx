// 环境预设库区块：内置技术栈卡片 + 我的预设卡片 + 导入按钮。
// 「项目配置」页用它把预设载入项目，「环境」页用它把预设装到整机；
// 两页共用同一个 localStorage 预设库（经 useUserPresets）。
import { open } from "@tauri-apps/plugin-dialog";
import { errorMessage, presetImport, type PresetFile } from "../api";
import { PRESETS } from "../data/presets";
import { presetToToml } from "../lib/preset";
import type { UserPresetsApi } from "../lib/useUserPresets";

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
  const { userPresets, remove, remember } = presets;

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

  return (
    <>
      <section className="panel">
        <div className="preset-head">
          <div>
            <h2 className="panel-title">环境预设</h2>
            <p className="setting-desc">选择技术栈一键生成全套环境，也可导入他人分享的预设文件</p>
          </div>
          <button className="btn" onClick={handleImport}>
            导入预设
          </button>
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
    </>
  );
}
