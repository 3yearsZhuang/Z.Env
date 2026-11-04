// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { doctorFix, doctorRun, type DoctorCheck } from "../api";
import DoctorPanel from "./DoctorPanel";

vi.mock("../api", () => ({
  doctorRun: vi.fn(),
  doctorFix: vi.fn(),
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

const checks: DoctorCheck[] = [
  {
    id: "mise",
    title: "mise 可用",
    level: "ok",
    detail: "mise 2024.1.1",
    hint: "",
    fixable: false,
  },
  {
    id: "path",
    title: "PATH 重复目录",
    level: "warn",
    detail: "发现 2 个重复目录",
    hint: "移除重复项",
    fixable: true,
  },
  {
    id: "shell",
    title: "shell 未接入",
    level: "fail",
    detail: "未在 ~/.zshrc 中找到 mise activate",
    hint: "手动执行 mise activate",
    fixable: false,
  },
];

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(doctorRun).mockResolvedValue(checks);
  vi.mocked(doctorFix).mockResolvedValue("已修复 PATH 重复");
});

afterEach(cleanup);

describe("DoctorPanel", () => {
  it("挂载即巡检，按等级显示总览 chips", async () => {
    render(<DoctorPanel />);
    await waitFor(() => expect(doctorRun).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("mise 可用 · 正常")).toBeTruthy();
    expect(screen.getByText("PATH 重复目录 · 注意")).toBeTruthy();
    expect(screen.getByText("shell 未接入 · 异常")).toBeTruthy();
  });

  it("全部正常时给出结论，不列异常项", async () => {
    vi.mocked(doctorRun).mockResolvedValue([checks[0]]);
    render(<DoctorPanel />);
    expect(await screen.findByText(/1 项检查全部正常/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "一键修复" })).toBeNull();
  });

  it("只给可修复项提供修复入口，修复后重跑并回调", async () => {
    const onFixed = vi.fn();
    render(<DoctorPanel onFixed={onFixed} />);
    await screen.findByText("PATH 重复目录 · 注意");

    const fixButtons = screen.getAllByRole("button", { name: "一键修复" });
    expect(fixButtons).toHaveLength(1);
    fireEvent.click(fixButtons[0]);

    await waitFor(() => expect(doctorFix).toHaveBeenCalledWith("path"));
    expect(onFixed).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("已修复 PATH 重复")).toBeTruthy();
    expect(doctorRun).toHaveBeenCalledTimes(2);
  });

  it("修复失败时保留原列表并说明原因", async () => {
    vi.mocked(doctorFix).mockRejectedValueOnce(new Error("写入被拒绝"));
    render(<DoctorPanel />);
    await screen.findByText("PATH 重复目录 · 注意");
    fireEvent.click(screen.getByRole("button", { name: "一键修复" }));
    expect(await screen.findByText("修复失败：写入被拒绝")).toBeTruthy();
    expect(doctorRun).toHaveBeenCalledTimes(1);
  });

  it("巡检本身失败时不假装正常", async () => {
    vi.mocked(doctorRun).mockRejectedValueOnce(new Error("探针超时"));
    render(<DoctorPanel />);
    expect(await screen.findByText("体检失败：探针超时")).toBeTruthy();
    expect(screen.queryByText(/项检查全部正常/)).toBeNull();
  });
});
