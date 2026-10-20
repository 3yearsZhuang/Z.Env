// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useProjectEditor } from "./useProjectEditor";
import { readProjectConfig, writeProjectConfig, installAllProject } from "../api";
vi.mock("../api", () => ({
  readProjectConfig: vi.fn(),
  writeProjectConfig: vi.fn(),
  installAllProject: vi.fn(),
}));
beforeEach(() => {
  vi.resetAllMocks();
});
describe("项目编辑会话", () => {
  it("异步 PATH 修改不会覆盖等待期间的新编辑", async () => {
    const { result } = renderHook(useProjectEditor);
    act(() => result.current.setContent("old"));
    let finish!: (content: string) => void;
    let transform!: Promise<void>;
    act(() => {
      transform = result.current.transform(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
    });
    act(() => result.current.setContent("new edit"));
    await act(async () => {
      finish("old with path");
      await expect(transform).rejects.toThrow("配置已改变");
    });
    expect(result.current.content).toBe("new edit");
  });
  it("安装失败后已保存的配置可直接重试", async () => {
    vi.mocked(readProjectConfig).mockResolvedValueOnce({
      path: "/A/mise.toml",
      content: "old",
      original: "old",
    });
    vi.mocked(installAllProject)
      .mockRejectedValueOnce(new Error("download failed"))
      .mockResolvedValueOnce("done");
    const { result } = renderHook(useProjectEditor);
    await act(() => result.current.load("/A"));
    act(() => result.current.setContent("saved"));
    vi.mocked(readProjectConfig).mockResolvedValue({
      path: "/A/mise.toml",
      content: "saved",
      original: "saved",
    });
    await act(async () => {
      await expect(result.current.save(true)).rejects.toThrow("download failed");
    });
    await act(() => result.current.save(true));
    expect(installAllProject).toHaveBeenLastCalledWith("/A/mise.toml", "saved", "saved");
  });
  it("先载入 A 再发现 B，保存和安装只能写 B", async () => {
    vi.mocked(readProjectConfig).mockImplementation(async (path) => ({
      path: `${path}/mise.toml`,
      content: path,
      original: path,
    }));
    vi.mocked(writeProjectConfig).mockResolvedValue("saved");
    vi.mocked(installAllProject).mockResolvedValue("installed");
    const { result } = renderHook(useProjectEditor);
    await act(() => result.current.load("/A"));
    await act(() => result.current.load("/B"));
    act(() => result.current.setContent("edited B"));
    await act(() => result.current.save());
    expect(writeProjectConfig).toHaveBeenCalledWith("/B/mise.toml", "edited B", "/B");
    await act(() => result.current.save(true));
    expect(installAllProject).toHaveBeenCalledWith("/B/mise.toml", "edited B", "edited B");
  });
  it("路径输入改变后必须重新载入，不能误用旧草稿", async () => {
    vi.mocked(readProjectConfig).mockResolvedValue({
      path: "/A/mise.toml",
      content: "A",
      original: "A",
    });
    const { result } = renderHook(useProjectEditor);
    await act(() => result.current.load("/A"));
    act(() => result.current.setPath("/B"));
    await expect(result.current.save()).rejects.toThrow("载入项目");
    expect(writeProjectConfig).not.toHaveBeenCalled();
  });
  it("慢的旧载入请求不能覆盖后来选中的项目", async () => {
    let finishA!: (value: { path: string; content: string; original: string }) => void;
    vi.mocked(readProjectConfig).mockImplementation((path) =>
      path === "/A"
        ? new Promise((resolve) => {
            finishA = resolve;
          })
        : Promise.resolve({ path: "/B/mise.toml", content: "B", original: "B" }),
    );
    const { result } = renderHook(useProjectEditor);
    let a!: Promise<boolean>;
    act(() => {
      a = result.current.load("/A");
    });
    await act(() => result.current.load("/B"));
    await act(async () => {
      finishA({ path: "/A/mise.toml", content: "A", original: "A" });
      await a;
    });
    expect(result.current.target).toBe("/B/mise.toml");
    expect(result.current.content).toBe("B");
  });
});
