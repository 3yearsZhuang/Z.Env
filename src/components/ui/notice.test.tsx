// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Notice } from "./notice";

afterEach(cleanup);

describe("Notice", () => {
  it("错误与警告使用 role=alert，其余使用 role=status", () => {
    const { unmount } = render(<Notice tone="error">出错了</Notice>);
    expect(screen.getByRole("alert").textContent).toContain("出错了");
    unmount();

    render(<Notice tone="success">已完成</Notice>);
    expect(screen.getByRole("status").textContent).toContain("已完成");
  });

  it("关闭入口是可键盘操作的按钮，并带无歧义标签", () => {
    const onDismiss = vi.fn();
    render(
      <Notice tone="error" onDismiss={onDismiss}>
        读取失败
      </Notice>,
    );
    const close = screen.getByRole("button", { name: "关闭错误提示" });
    fireEvent.click(close);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("不传 onDismiss 时不渲染关闭入口", () => {
    render(<Notice tone="info">提示</Notice>);
    expect(screen.queryByRole("button")).toBeNull();
  });
});
