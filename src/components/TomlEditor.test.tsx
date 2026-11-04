// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import TomlEditor from "./TomlEditor";

afterEach(cleanup);

describe("TomlEditor", () => {
  it("挂载出 CodeMirror 视图并渲染初始内容", () => {
    const { container } = render(<TomlEditor value={'[tools]\nnode = "22"'} onChange={vi.fn()} />);
    expect(container.querySelector(".cm-editor")).toBeTruthy();
    expect(container.querySelector(".cm-content")?.textContent).toContain("node");
    // 行号栏来自本次最小组装（basicSetup 已移除），顺带锁住扩展没有被删掉。
    expect(container.querySelector(".cm-gutters")).toBeTruthy();
  });

  it("外部 value 变化时替换文档内容", () => {
    const { container, rerender } = render(<TomlEditor value={"a = 1"} onChange={vi.fn()} />);
    expect(container.querySelector(".cm-content")?.textContent).toContain("a = 1");

    rerender(<TomlEditor value={"b = 2"} onChange={vi.fn()} />);
    const text = container.querySelector(".cm-content")?.textContent ?? "";
    expect(text).toContain("b = 2");
    expect(text).not.toContain("a = 1");
  });
});
