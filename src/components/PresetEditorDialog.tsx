// 预设编辑器：新建 / 修改一份自定义预设（名称 + 描述 + 工具与版本清单）。
// 只负责收集表单，落库由 PresetLibrary 统一处理（要区分新建、覆盖与改名三种情况）。
import { useEffect, useState } from "react";
import type { PresetTool } from "../api";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";

/** 编辑器初始值；isNew 决定标题文案与重名校验口径 */
export interface PresetDraft {
  isNew: boolean;
  name: string;
  description: string;
  tools: PresetTool[];
  /** 被编辑预设的原始 mise.toml 文本（用于判断是否含 [tools] 以外的配置） */
  originalContent?: string;
  /** 原始预设名（改名时要删掉旧条目） */
  originalName?: string;
}

interface Props {
  draft: PresetDraft | null;
  /** 提示该预设保留了 [tools] 以外的配置段 */
  hasExtraSections?: boolean;
  onClose: () => void;
  onSave: (values: { name: string; description: string; tools: PresetTool[] }) => void;
}

export default function PresetEditorDialog({ draft, hasExtraSections, onClose, onSave }: Props) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [tools, setTools] = useState<PresetTool[]>([]);
  const [error, setError] = useState<string | null>(null);

  // 每次打开都用草稿重置表单，避免上一次的残留
  useEffect(() => {
    if (!draft) return;
    setName(draft.name);
    setDescription(draft.description);
    setTools(draft.tools.length > 0 ? draft.tools : [{ name: "", version: "" }]);
    setError(null);
  }, [draft]);

  function setTool(i: number, patch: Partial<PresetTool>) {
    setTools((prev) => prev.map((t, idx) => (idx === i ? { ...t, ...patch } : t)));
  }

  function handleSave() {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("请填写预设名称");
      return;
    }
    const valid = tools
      .map((t) => ({ name: t.name.trim(), version: t.version.trim() }))
      .filter((t) => t.name || t.version);
    if (valid.length === 0) {
      setError("至少填一个工具");
      return;
    }
    const half = valid.find((t) => !t.name || !t.version);
    if (half) {
      setError("每个工具的工具名与版本都要填全");
      return;
    }
    // 同名工具会互相覆盖，这里挡在最前面而不是静默丢一个
    const names = new Set(valid.map((t) => t.name));
    if (names.size !== valid.length) {
      setError("同一个工具只能出现一次");
      return;
    }
    onSave({ name: trimmed, description: description.trim(), tools: valid });
  }

  return (
    <Dialog open={draft !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="w-[560px] p-0">
        <div className="dialog-head">
          <DialogTitle>{draft?.isNew ? "新建预设" : "修改预设"}</DialogTitle>
          <DialogDescription className="setting-desc">
            一份预设 = 一组工具与版本；保存后可在「环境」页装到整机，或在「项目配置」页装进项目。
          </DialogDescription>
        </div>

        <div className="dialog-body">
          <label className="field-label" htmlFor="preset-name">
            名称
          </label>
          <input
            id="preset-name"
            className="input"
            value={name}
            placeholder="例如：我的 Node 工具链"
            onChange={(e) => setName(e.target.value)}
          />

          <label className="field-label" htmlFor="preset-desc">
            描述（可选）
          </label>
          <input
            id="preset-desc"
            className="input"
            value={description}
            placeholder="一句话说明这套环境用来做什么"
            onChange={(e) => setDescription(e.target.value)}
          />

          <label className="field-label">工具与版本</label>
          <div className="preset-edit-list">
            {tools.map((t, i) => (
              <div className="preset-edit-row" key={i}>
                <input
                  className="input"
                  value={t.name}
                  placeholder="工具名，如 node / npm:prettier"
                  onChange={(e) => setTool(i, { name: e.target.value })}
                />
                <input
                  className="input"
                  value={t.version}
                  placeholder="版本，如 20 / latest"
                  onChange={(e) => setTool(i, { version: e.target.value })}
                />
                <button
                  className="preset-act danger"
                  title="移除这个工具"
                  onClick={() => setTools((prev) => prev.filter((_, idx) => idx !== i))}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
          <button
            className="btn xs"
            onClick={() => setTools((prev) => [...prev, { name: "", version: "" }])}
          >
            + 添加工具
          </button>

          <p className="setting-desc" style={{ marginTop: 10 }}>
            工具名支持 mise 后端名（如 <code>npm:prettier</code>）；版本可写具体版本或{" "}
            <code>latest</code>。
          </p>
          {hasExtraSections && (
            <p className="setting-desc" style={{ marginTop: 6 }}>
              这份预设还含 <code>[tools]</code> 以外的配置（如 <code>[env]</code>
              ），保存时会原样保留，只替换工具清单。
            </p>
          )}
          {error && (
            <div className="banner error" style={{ marginTop: 10 }} onClick={() => setError(null)}>
              ⚠ {error}
            </div>
          )}
        </div>

        <div className="dialog-foot">
          <button className="btn-ghost" onClick={onClose}>
            取消
          </button>
          <span style={{ flex: 1 }} />
          <button className="btn primary" onClick={handleSave}>
            保存
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
