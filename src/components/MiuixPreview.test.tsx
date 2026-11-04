// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import MiuixPreview from "./MiuixPreview";

afterEach(cleanup);

describe("MiuixPreview", () => {
  it("是纯视觉样例：渲染侧栏导航与概览卡片，不接任何后端命令", () => {
    const { container } = render(<MiuixPreview />);
    expect(screen.getByText("Z.Env")).toBeTruthy();
    expect(screen.getByRole("button", { name: /系统概览/ })).toBeTruthy();
    expect(screen.getByText("Node 22.23.3")).toBeTruthy();
    expect(container.querySelector(".miuix")).toBeTruthy();
  });

  it("切换主题只在根节点加 dark 类", () => {
    const { container } = render(<MiuixPreview />);
    const root = container.querySelector(".miuix")!;
    expect(root.className).not.toContain("dark");
    fireEvent.click(screen.getByRole("button", { name: /深色模式/ }));
    expect(container.querySelector(".miuix")!.className).toContain("dark");
  });

  it("开关按钮暴露 switch 语义与选中状态", () => {
    render(<MiuixPreview />);
    const toggle = screen.getByRole("switch");
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle);
    expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("false");
  });
});
