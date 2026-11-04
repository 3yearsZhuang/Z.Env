// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { UserPresetsApi } from "../lib/useUserPresets";
import PresetLibrary, { type PresetChoice } from "./PresetLibrary";

vi.mock("../api", () => ({
  replaceTools: vi.fn(),
  presetImport: vi.fn(),
  presetParseTools: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

function presets(overrides: Partial<UserPresetsApi> = {}): UserPresetsApi {
  return {
    userPresets: [],
    persist: vi.fn(),
    save: vi.fn(),
    remove: vi.fn(),
    remember: vi.fn(() => true),
    ...overrides,
  };
}

function renderLibrary(overrides: Partial<Parameters<typeof PresetLibrary>[0]> = {}) {
  const onUse = vi.fn();
  const onExport = vi.fn();
  const props = {
    presets: presets(),
    useHint: "点击预览并装到整机",
    onUse,
    onExport,
    onImported: vi.fn(),
    onError: vi.fn(),
    ...overrides,
  };
  const view = render(<PresetLibrary {...props} />);
  return { ...view, onUse, onExport, props };
}

afterEach(cleanup);

describe("PresetLibrary", () => {
  it("点击内置预设卡片交出可用的 toml 载荷", () => {
    const { onUse } = renderLibrary();
    const cards = document.querySelectorAll<HTMLButtonElement>(".preset-card");
    expect(cards.length).toBeGreaterThan(0);

    fireEvent.click(cards[0]);
    expect(onUse).toHaveBeenCalledTimes(1);
    const choice = onUse.mock.calls[0][0] as PresetChoice;
    expect(choice.name).toBeTruthy();
    expect(choice.toml).toContain("[tools]");
  });

  it("安装通道被占用时禁用卡片并给出原因", () => {
    renderLibrary({ useDisabled: true, useDisabledHint: "快照重建进行中" });
    const card = document.querySelector<HTMLButtonElement>(".preset-card");
    expect(card?.disabled).toBe(true);
    expect(card?.title).toBe("快照重建进行中");
  });

  it("我的预设可导出与删除", () => {
    const remove = vi.fn();
    const { onExport } = renderLibrary({
      presets: presets({
        userPresets: [{ name: "工作环境", content: '[tools]\nnode = "22"', description: "自用" }],
        remove,
      }),
    });

    expect(screen.getByText("工作环境")).toBeTruthy();

    // 内置卡片也有导出按钮，「我的预设」渲染在其后，取最后一个。
    const exports = screen.getAllByTitle("导出为可分享的预设文件");
    fireEvent.click(exports[exports.length - 1]);
    expect(onExport).toHaveBeenCalledTimes(1);
    expect((onExport.mock.calls[0][0] as PresetChoice).toml).toContain("node");

    fireEvent.click(screen.getByTitle("删除该预设"));
    expect(remove).toHaveBeenCalledWith("工作环境");
  });
});
