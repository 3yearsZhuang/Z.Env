// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import PresetEditorDialog, { type PresetDraft } from "./PresetEditorDialog";

afterEach(cleanup);

const newDraft: PresetDraft = {
  isNew: true,
  name: "",
  description: "",
  tools: [{ name: "", version: "" }],
};

function renderDialog(draft: PresetDraft | null, extra: Record<string, unknown> = {}) {
  const onSave = vi.fn();
  const onClose = vi.fn();
  render(<PresetEditorDialog draft={draft} onSave={onSave} onClose={onClose} {...extra} />);
  return { onSave, onClose };
}

describe("PresetEditorDialog", () => {
  it("draft 为 null 时不渲染对话框", () => {
    renderDialog(null);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("名称必填：为空时提示且不落库", () => {
    const { onSave } = renderDialog(newDraft);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(screen.getByRole("alert").textContent).toContain("请填写预设名称");
    expect(onSave).not.toHaveBeenCalled();
  });

  it("工具名与版本必须成对填写", () => {
    const { onSave } = renderDialog({ ...newDraft, name: "我的工具链" });
    fireEvent.change(screen.getByPlaceholderText(/工具名/), { target: { value: "node" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(screen.getByRole("alert").textContent).toContain("每个工具的工具名与版本都要填全");
    expect(onSave).not.toHaveBeenCalled();
  });

  it("同一工具不能重复，避免静默互相覆盖", () => {
    const { onSave } = renderDialog({
      ...newDraft,
      name: "重复工具",
      tools: [
        { name: "node", version: "20" },
        { name: "node", version: "22" },
      ],
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(screen.getByRole("alert").textContent).toContain("同一个工具只能出现一次");
    expect(onSave).not.toHaveBeenCalled();
  });

  it("合法输入按 trim 后的值落库，并可增删工具行", () => {
    const { onSave } = renderDialog({
      isNew: false,
      name: "  工作环境  ",
      description: "  自用  ",
      tools: [{ name: " node ", version: " 22 " }],
      originalContent: '[tools]\nnode = "22"',
      originalName: "旧名字",
    });

    fireEvent.click(screen.getByRole("button", { name: "+ 添加工具" }));
    expect(screen.getAllByTitle("移除这个工具")).toHaveLength(2);
    fireEvent.click(screen.getAllByTitle("移除这个工具")[1]);

    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(onSave).toHaveBeenCalledWith({
      name: "工作环境",
      description: "自用",
      tools: [{ name: "node", version: "22" }],
    });
    // 修改既有预设时标题为「修改预设」，并提示会保留 [tools] 以外的配置
    expect(screen.getByText("修改预设")).toBeTruthy();
  });

  it("有额外配置段时明确告知会原样保留", () => {
    renderDialog(
      {
        isNew: false,
        name: "带 env 的预设",
        description: "",
        tools: [{ name: "node", version: "22" }],
      },
      { hasExtraSections: true },
    );
    expect(screen.getByText(/保存时会原样保留，只替换工具清单/)).toBeTruthy();
  });
});
