// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listRegistry, listTools } from "../api";
import EasterEgg from "./EasterEgg";

vi.mock("../api", () => ({
  listTools: vi.fn(),
  listRegistry: vi.fn(),
}));

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(listTools).mockResolvedValue([]);
  vi.mocked(listRegistry).mockResolvedValue([]);
});

afterEach(cleanup);

describe("EasterEgg 无障碍", () => {
  it("是模态对话框并把焦点收进面板", async () => {
    render(<EasterEgg onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(dialog.getAttribute("aria-labelledby")).toBe("easter-title");
    expect(document.activeElement).toBe(dialog);
    expect(screen.getByRole("button", { name: "关闭彩蛋面板" })).toBeTruthy();
  });

  it("Esc 关闭", () => {
    const onClose = vi.fn();
    render(<EasterEgg onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Tab 焦点循环停留在面板内", () => {
    render(<EasterEgg onClose={vi.fn()} />);
    const buttons = screen.getAllByRole("button");
    const last = buttons[buttons.length - 1];
    last.focus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(document.activeElement).toBe(buttons[0]);
  });
});
