// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { open } from "@tauri-apps/plugin-dialog";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  discoverProjects,
  readProjectConfig,
  writeProjectConfig,
  type DiscoveredProject,
} from "../api";
import ProjectsView from "./ProjectsView";
import type { TomlEditorProps } from "./TomlEditor";

vi.mock("../api", () => ({
  discoverProjects: vi.fn(),
  readProjectConfig: vi.fn(),
  writeProjectConfig: vi.fn(),
  installAllProject: vi.fn(),
  detectToolSources: vi.fn(),
  stableBinPath: vi.fn(),
  bindProjectPath: vi.fn(),
  errorMessage: (error: unknown) => String(error),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("./PresetLibrary", () => ({ default: () => <div>预设库</div> }));
vi.mock("./ProjectEnvironmentPanel", () => ({
  default: ({ onChanged }: { onChanged: () => void }) => (
    <button onClick={onChanged}>完成项目环境操作</button>
  ),
}));
vi.mock("./TomlEditor", () => ({
  default: ({ value, onChange }: TomlEditorProps) => (
    <textarea
      aria-label="配置内容"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const projects: DiscoveredProject[] = Array.from({ length: 56 }, (_, index) => ({
  name: `project-${index + 1}`,
  path: `/work/project-${index + 1}`,
  hasMiseToml: true,
  tools: ["node"],
  missingTools: [],
}));

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(discoverProjects).mockResolvedValue(projects);
  vi.mocked(readProjectConfig).mockImplementation(async (path) => ({
    path: `${path}/mise.toml`,
    content: '[tools]\nnode = "22"',
    original: '[tools]\nnode = "22"',
  }));
  vi.mocked(writeProjectConfig).mockResolvedValue("保存成功");
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(cleanup);

describe("项目发现与编辑流程", () => {
  it("扫描失败可以重试，项目选择器失败也明确反馈", async () => {
    const user = userEvent.setup();
    vi.mocked(discoverProjects).mockRejectedValueOnce("无法扫描目录");
    vi.mocked(open).mockRejectedValueOnce("目录选择器不可用");
    render(<ProjectsView />);
    expect((await screen.findByRole("alert")).textContent).toContain("无法扫描目录");
    await user.click(screen.getByRole("button", { name: "重新扫描" }));
    await screen.findByRole("button", { name: "载入 project-1" });
    expect(screen.queryByRole("alert")).toBeNull();
    await user.click(screen.getByRole("button", { name: "选择文件夹" }));
    expect((await screen.findByRole("alert")).textContent).toContain("选择项目目录失败");
  });

  it("分页显示完整项目集合，搜索能找到第 50 项之后的项目", async () => {
    const user = userEvent.setup();
    render(<ProjectsView />);
    await screen.findByRole("button", { name: "载入 project-1" });
    await user.click(screen.getByRole("button", { name: "下一页项目" }));
    expect(screen.getByRole("button", { name: "载入 project-7" })).toBeTruthy();
    const search = screen.getByRole("searchbox", { name: "搜索项目名称或路径" });
    await user.type(search, "project-56");
    expect(screen.getByRole("button", { name: "载入 project-56" })).toBeTruthy();
    await user.clear(search);
    await user.type(search, "不存在的项目");
    expect(screen.getByText("没有匹配的项目")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "清除搜索" }));
    expect(screen.getByRole("button", { name: "载入 project-1" })).toBeTruthy();
  });

  it("保存期间禁止切换目标和重复提交，完成后恢复编辑", async () => {
    const user = userEvent.setup();
    let finish!: (value: string) => void;
    vi.mocked(writeProjectConfig).mockReturnValue(new Promise((resolve) => (finish = resolve)));
    render(<ProjectsView />);
    await user.click(await screen.findByRole("button", { name: "载入 project-1" }));
    const editor = await screen.findByRole("textbox", { name: "配置内容" });
    fireEvent.change(editor, { target: { value: '[tools]\nnode = "24"' } });
    const save = screen.getByRole("button", { name: "保存配置" });
    fireEvent.click(save);
    fireEvent.click(save);
    expect(writeProjectConfig).toHaveBeenCalledOnce();
    expect(writeProjectConfig).toHaveBeenCalledWith(
      "/work/project-1/mise.toml",
      '[tools]\nnode = "24"',
      '[tools]\nnode = "22"',
    );
    expect((screen.getByLabelText("项目目录或配置文件") as HTMLInputElement).disabled).toBe(true);
    expect(
      (screen.getByRole("button", { name: "载入 project-2" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    fireEvent.change(editor, { target: { value: "保存时输入" } });
    expect((editor as HTMLTextAreaElement).value).toBe('[tools]\nnode = "24"');
    await act(async () => finish("保存成功"));
    expect((screen.getByRole("button", { name: "保存配置" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
    expect(screen.getByText("与载入内容一致")).toBeTruthy();
  });

  it("读取另一个项目失败时保留草稿，撤销旧保存目标", async () => {
    const user = userEvent.setup();
    render(<ProjectsView />);
    await user.click(await screen.findByRole("button", { name: "载入 project-1" }));
    const editor = await screen.findByRole("textbox", { name: "配置内容" });
    fireEvent.change(editor, { target: { value: "需要保留的草稿" } });
    vi.mocked(readProjectConfig).mockRejectedValueOnce("读取被拒绝");
    await user.click(screen.getByRole("button", { name: "载入 project-2" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("读取被拒绝"));
    expect((editor as HTMLTextAreaElement).value).toBe("需要保留的草稿");
    expect((screen.getByRole("button", { name: "保存配置" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect(screen.getByText("草稿 · 待读取项目")).toBeTruthy();
    expect(writeProjectConfig).not.toHaveBeenCalled();
  });
  it("环境操作后保留草稿并阻止覆盖外部配置，重新读取后恢复保存", async () => {
    const user = userEvent.setup();
    render(<ProjectsView />);
    await user.click(await screen.findByRole("button", { name: "载入 project-1" }));
    const editor = await screen.findByRole("textbox", { name: "配置内容" });
    fireEvent.change(editor, { target: { value: "保留草稿" } });
    await user.click(await screen.findByRole("button", { name: "完成项目环境操作" }));
    expect((editor as HTMLTextAreaElement).value).toBe("保留草稿");
    expect((screen.getByRole("button", { name: "保存配置" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect(writeProjectConfig).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "重新读取" }));
    await waitFor(() => expect((editor as HTMLTextAreaElement).value).toContain('node = "22"'));
    expect((screen.getByRole("button", { name: "保存配置" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });
});
