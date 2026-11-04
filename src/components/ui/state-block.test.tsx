// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { FolderOpen } from "lucide-react";
import { afterEach, describe, expect, it } from "vitest";
import { StateBlock } from "./state-block";

afterEach(cleanup);

describe("StateBlock", () => {
  it("lg 形态用 role=status 输出图标、标题、说明与动作", () => {
    render(
      <StateBlock
        icon={FolderOpen}
        title="还没有发现项目"
        detail="可以在下方手动输入路径。"
        action={<button type="button">选择项目目录</button>}
      />,
    );
    const block = screen.getByRole("status");
    expect(block.className).toContain("state-block-lg");
    expect(block.querySelector("svg")).toBeTruthy();
    expect(screen.getByText("还没有发现项目").tagName).toBe("STRONG");
    expect(screen.getByText("可以在下方手动输入路径。").tagName).toBe("P");
    expect(screen.getByRole("button", { name: "选择项目目录" })).toBeTruthy();
  });

  it("sm/md 形态只输出文本提示，不再重复 role=status 的内容", () => {
    const { rerender } = render(<StateBlock size="sm">暂无日志</StateBlock>);
    const small = screen.getByRole("status");
    expect(small.className).toContain("state-block-sm");
    expect(small.querySelector("svg")).toBeNull();

    rerender(<StateBlock size="md">正在读取环境…</StateBlock>);
    expect(screen.getByRole("status").className).toContain("state-block-md");
  });

  it("读取中与边框/高度修饰可叠加", () => {
    render(<StateBlock icon={FolderOpen} spinning bordered tall title="正在发现项目" />);
    const block = screen.getByRole("status");
    expect(block.className).toContain("bordered");
    expect(block.className).toContain("tall");
    expect(block.querySelector("svg")?.getAttribute("class")).toContain("state-spinning");
  });
});
