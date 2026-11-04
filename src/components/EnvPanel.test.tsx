// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { envCenterRemove, envCenterSet, type EnvCenterSnapshot } from "../api";
import EnvPanel from "./EnvPanel";

vi.mock("../api", () => ({
  envCenterSet: vi.fn(),
  envCenterRemove: vi.fn(),
  errorMessage: (error: unknown) => String(error),
}));

const snap: EnvCenterSnapshot = {
  configPath: "/Users/me/.config/mise/config.toml",
  exists: true,
  entries: [
    { key: "HTTP_PROXY", value: "http://127.0.0.1:7890", kind: "simple" },
    { key: "PATH_EXTRA", value: "a\nb", kind: "complex" },
  ],
  conflicts: [],
  systemEnv: [
    { key: "HOME", value: "/Users/me" },
    { key: "SHELL", value: "/bin/zsh" },
  ],
};

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(envCenterSet).mockResolvedValue("已写入");
  vi.mocked(envCenterRemove).mockResolvedValue("已移除");
});

afterEach(cleanup);

describe("EnvPanel", () => {
  it("简单值可编辑删除，高级值只读标记", () => {
    render(<EnvPanel snap={snap} reload={vi.fn()} />);
    expect(screen.getByText("高级值")).toBeTruthy();
    // 两个条目里只有 simple 的那条带操作按钮
    expect(screen.getAllByRole("button", { name: "编辑" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "删除" })).toHaveLength(1);
    expect(screen.getByText(/config\.toml/)).toBeTruthy();
  });

  it("新增变量写入成功后清空输入并刷新快照", async () => {
    const reload = vi.fn();
    render(<EnvPanel snap={snap} reload={reload} />);

    fireEvent.change(screen.getByPlaceholderText("变量名（如 HTTP_PROXY）"), {
      target: { value: "NODE_OPTIONS" },
    });
    fireEvent.change(screen.getByPlaceholderText("值"), {
      target: { value: "--max-old-space-size=4096" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    await waitFor(() =>
      expect(envCenterSet).toHaveBeenCalledWith("NODE_OPTIONS", "--max-old-space-size=4096"),
    );
    expect(reload).toHaveBeenCalledTimes(1);
    expect(screen.getByText("已写入")).toBeTruthy();
    expect(screen.getByPlaceholderText("变量名（如 HTTP_PROXY）")).toHaveProperty("value", "");
  });

  it("写入失败时用警告提示条展示原因且不清空输入", async () => {
    vi.mocked(envCenterSet).mockRejectedValueOnce(new Error("配置被外部修改"));
    render(<EnvPanel snap={snap} reload={vi.fn()} />);

    fireEvent.change(screen.getByPlaceholderText("变量名（如 HTTP_PROXY）"), {
      target: { value: "FOO" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("配置被外部修改");
    // 关闭入口是真正的按钮
    fireEvent.click(screen.getByRole("button", { name: "关闭警告提示" }));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByPlaceholderText("变量名（如 HTTP_PROXY）")).toHaveProperty("value", "FOO");
  });

  it("删除条目调用后端并刷新", async () => {
    const reload = vi.fn();
    render(<EnvPanel snap={snap} reload={reload} />);
    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    await waitFor(() => expect(envCenterRemove).toHaveBeenCalledWith("HTTP_PROXY"));
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("系统变量默认折叠，展开后可搜索过滤", () => {
    render(<EnvPanel snap={snap} reload={vi.fn()} />);
    expect(screen.queryByText("/bin/zsh")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "展开" }));
    expect(screen.getByText("/bin/zsh")).toBeTruthy();

    fireEvent.change(screen.getByPlaceholderText("搜索变量名或值…"), { target: { value: "home" } });
    expect(screen.getByText("/Users/me")).toBeTruthy();
    expect(screen.queryByText("/bin/zsh")).toBeNull();
  });
});
