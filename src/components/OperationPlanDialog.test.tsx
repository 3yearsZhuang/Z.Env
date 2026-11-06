// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { operationCancel } from "../api";
import {
  operationExecute,
  operationPlan,
  operationBindings,
  type OperationPlan,
  type ExecutionRecord,
} from "../api/operationPlan";
import { operationPlanV2 } from "../api/operationPlanV2";
import OperationPlanDialog from "./OperationPlanDialog";
vi.mock("../api", () => ({ operationCancel: vi.fn(), errorMessage: (e: unknown) => String(e) }));
vi.mock("../api/operationPlan", () => ({
  operationExecute: vi.fn(),
  operationPlan: vi.fn(),
  operationBindings: vi.fn(),
}));
vi.mock("../api/operationPlanV2", () => ({ operationPlanV2: vi.fn() }));
function plan(): OperationPlan {
  return {
    schemaVersion: 1,
    impact: [],
    planId: "server-plan",
    createdAt: Date.now(),
    expiresAt: Date.now() + 120000,
    snapshotGeneration: "1",
    snapshotRequestId: "snapshot",
    action: "verify",
    target: {
      providerPackage: { providerId: "nvm", packageId: "node", kind: "runtime", platform: "macos" },
      installationId: "instance",
      locator: "/fixture/node",
    },
    capability: { canPreview: true, canExecute: true, reason: null },
    versionPromise: "精确 22.1.0",
    configuration: {
      path: "/fixture/config.toml",
      canonicalPath: null,
      exists: false,
      sha256: null,
    },
    configurationChange: "不改写配置",
    permission: "不提权",
    rollback: "不修改原安装",
    steps: [
      {
        kind: "preflight",
        title: "核对目标",
        program: null,
        argv: [],
        cwd: "/fixture",
        environmentKeys: ["HOME", "PATH"],
      },
    ],
  };
}
const result: ExecutionRecord = {
  schemaVersion: 1,
  id: "server-plan",
  action: "verify",
  status: "completed",
  createdAt: 1,
  updatedAt: 2,
  steps: [],
  outcome: "verified",
};
const finished = vi.fn();
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(operationPlan).mockResolvedValue(plan());
  vi.mocked(operationPlanV2).mockImplementation(async (envelope) => ({
    schemaVersion: 2,
    subject: envelope.subject,
    plan: await (envelope.subject.toolId === "python"
      ? operationPlan(envelope.request, "python")
      : operationPlan(envelope.request)),
    stepGraph: [],
  }));
  vi.mocked(operationExecute).mockResolvedValue(result);
  vi.mocked(operationCancel).mockResolvedValue();
  vi.mocked(operationBindings).mockResolvedValue([]);
});
afterEach(async () => {
  cleanup();
  // Radix 在卸载后异步归还焦点；让回调在当前 jsdom realm 销毁前退出。
  await new Promise((resolve) => setTimeout(resolve, 0));
});
function show() {
  return render(
    <OperationPlanDialog
      snapshotRequestId="snapshot"
      installationId="instance"
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
}
describe("操作计划预览与执行", () => {
  it("绑定明确提交方式和项目范围，显示字段差异并区分受控验证与终端生效", async () => {
    const user = userEvent.setup();
    const preview = plan();
    preview.action = "bind";
    preview.configurationDiff = [
      { path: "/project/mise.toml", field: "tools.node", before: "'20'", after: "'22.1.0'" },
    ];
    vi.mocked(operationPlan).mockResolvedValue(preview);
    vi.mocked(operationExecute).mockResolvedValue({ ...result, action: "bind", outcome: "bound" });
    show();
    await user.selectOptions(screen.getByRole("combobox", { name: "计划操作" }), "bind");
    await user.selectOptions(screen.getByRole("combobox", { name: "绑定方式" }), "managed");
    await user.selectOptions(screen.getByRole("combobox", { name: "绑定作用范围" }), "project");
    expect((screen.getByRole("button", { name: "生成预览" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    await user.type(screen.getByRole("textbox", { name: "绑定项目路径" }), "/project");
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    expect(operationPlan).toHaveBeenCalledExactlyOnceWith({
      snapshotRequestId: "snapshot",
      intent: {
        action: "bind",
        installationId: "instance",
        binding: { strategy: "managed", scope: "project", projectPath: "/project" },
      },
    });
    expect(await screen.findByRole("region", { name: "配置差异" })).toHaveProperty(
      "textContent",
      expect.stringContaining("tools.node"),
    );
    expect(operationExecute).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "执行绑定" }));
    await screen.findByText(/用户终端和 IDE 的实际生效尚未确认/);
    expect(finished).toHaveBeenCalledOnce();
  });
  it("项目 PATH 自动限定项目范围，更换方式会丢弃已有预览", async () => {
    const user = userEvent.setup();
    show();
    await user.selectOptions(screen.getByRole("combobox", { name: "计划操作" }), "bind");
    await user.selectOptions(screen.getByRole("combobox", { name: "绑定方式" }), "projectPath");
    expect(
      (screen.getByRole("combobox", { name: "绑定作用范围" }) as HTMLSelectElement).value,
    ).toBe("project");
    expect(
      (screen.getByRole("combobox", { name: "绑定作用范围" }) as HTMLSelectElement).disabled,
    ).toBe(true);
    expect(operationPlan).not.toHaveBeenCalled();
  });
  it("可从独立入口读取并解除中断的绑定，原安装保留", async () => {
    const user = userEvent.setup();
    vi.mocked(operationBindings).mockResolvedValue([
      {
        bindingId: "binding-server",
        providerPackage: plan().target.providerPackage,
        version: "22.1.0",
        locator: "/missing/original",
        strategy: "direct",
        scope: "global",
        configurationPath: "/global/config.toml",
        state: "pending",
      },
    ]);
    const preview = plan();
    preview.action = "unbind";
    preview.target.installationId = "old-instance";
    preview.target.locator = "/missing/original";
    vi.mocked(operationPlan).mockResolvedValue(preview);
    vi.mocked(operationExecute).mockResolvedValue({
      ...result,
      action: "unbind",
      outcome: "unbound",
    });
    render(
      <OperationPlanDialog
        snapshotRequestId="snapshot"
        installationId={null}
        initialAction="unbind"
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    await screen.findByRole("option", { name: /中断或待确认/ });
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    expect(operationPlan).toHaveBeenCalledExactlyOnceWith({
      snapshotRequestId: "snapshot",
      intent: { action: "unbind", bindingId: "binding-server" },
    });
    await user.click(await screen.findByRole("button", { name: "执行解除绑定" }));
    await screen.findByText(/本绑定已解除；原安装仍由原来源管理/);
    expect(finished).toHaveBeenCalledOnce();
  });
  it("读取绑定失败可见且没有解除执行入口", async () => {
    vi.mocked(operationBindings).mockRejectedValue("绑定记录不可读");
    render(
      <OperationPlanDialog
        snapshotRequestId="snapshot"
        installationId={null}
        initialAction="unbind"
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    await screen.findByRole("alert");
    expect(screen.getByText("绑定记录不可读")).toBeTruthy();
    expect(operationPlan).not.toHaveBeenCalled();
    expect((screen.getByRole("button", { name: "生成预览" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });
  it("挂载不调用后端，明确生成预览后只执行签发 ID，成功通知重新读取", async () => {
    const user = userEvent.setup();
    show();
    expect(operationPlan).not.toHaveBeenCalled();
    expect(operationExecute).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    expect(await screen.findByRole("region", { name: "后端操作计划" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "执行重新验证" }));
    expect(operationExecute).toHaveBeenCalledExactlyOnceWith("server-plan");
    await screen.findByText(/原实例已重新验证/);
    expect(finished).toHaveBeenCalledOnce();
  });
  it("写能力未开放时说明原因，不提供执行入口", async () => {
    const user = userEvent.setup();
    const blocked = plan();
    blocked.action = "uninstall";
    blocked.capability = { canPreview: false, canExecute: false, reason: "未确认删除归属" };
    blocked.steps = [];
    vi.mocked(operationPlan).mockResolvedValue(blocked);
    show();
    await user.selectOptions(screen.getByRole("combobox", { name: "计划操作" }), "uninstall");
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    expect(await screen.findByText("未确认删除归属")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "执行重新验证" })).toBeNull();
    expect(operationExecute).not.toHaveBeenCalled();
  });
  it.each([
    ["install", "执行安装", "installed", /安装已通过来源重扫与版本验证/],
    ["uninstall", "执行卸载", "uninstalled", /已确认所选实例移除/],
  ] as const)(
    "%s 展示渠道影响并在验证通过后反馈对应结果",
    async (action, label, outcome, message) => {
      const user = userEvent.setup();
      const preview = plan();
      preview.action = action;
      preview.target.providerPackage.providerId = "mise";
      if (action === "install") {
        preview.target.installationId = null;
        preview.target.locator = null;
      }
      preview.impact = ["依赖可能安装或升级；项目和终端不会自动切换"];
      vi.mocked(operationPlan).mockResolvedValue(preview);
      vi.mocked(operationExecute).mockResolvedValue({ ...result, action, outcome });
      render(
        <OperationPlanDialog
          snapshotRequestId="snapshot"
          installationId={action === "install" ? null : "instance"}
          onClose={vi.fn()}
          onFinished={finished}
        />,
      );
      if (action === "install")
        await user.type(screen.getByRole("textbox", { name: "Node 精确版本" }), "22.1.0");
      else
        await user.selectOptions(screen.getByRole("combobox", { name: "计划操作" }), "uninstall");
      await user.click(screen.getByRole("button", { name: "生成预览" }));
      expect(await screen.findByRole("region", { name: "操作影响" })).toHaveProperty(
        "textContent",
        expect.stringContaining("依赖可能安装或升级"),
      );
      expect(operationExecute).not.toHaveBeenCalled();
      await user.click(screen.getByRole("button", { name: label }));
      await screen.findByText(message);
      expect(operationExecute).toHaveBeenCalledExactlyOnceWith("server-plan");
      expect(finished).toHaveBeenCalledOnce();
    },
  );
  it.each(["partial", "cancelled", "interrupted"] as const)(
    "%s 即使附带 installed 结果也不能显示安装成功",
    async (status) => {
      const user = userEvent.setup();
      const preview = plan();
      preview.action = "install";
      preview.target.installationId = null;
      preview.target.locator = null;
      preview.target.providerPackage.providerId = "mise";
      vi.mocked(operationPlan).mockResolvedValue(preview);
      vi.mocked(operationExecute).mockResolvedValue({
        ...result,
        action: "install",
        status,
        outcome: "installed",
      });
      render(
        <OperationPlanDialog
          snapshotRequestId="snapshot"
          installationId={null}
          onClose={vi.fn()}
          onFinished={finished}
        />,
      );
      await user.type(screen.getByRole("textbox", { name: "Node 精确版本" }), "22.1.0");
      await user.click(screen.getByRole("button", { name: "生成预览" }));
      await user.click(await screen.findByRole("button", { name: "执行安装" }));
      await screen.findByText(/本次操作未确认完成/);
      expect(screen.queryByText(/安装已通过来源重扫与版本验证/)).toBeNull();
      expect(finished).toHaveBeenCalledOnce();
    },
  );
  it("计划过期或清单更换时不能执行，重新预览保留新的清单身份", async () => {
    const user = userEvent.setup();
    const expired = plan();
    expired.expiresAt = Date.now() - 1;
    vi.mocked(operationPlan).mockResolvedValueOnce(expired);
    const { rerender } = show();
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    expect(await screen.findByText("已过期，请重新生成预览")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "执行重新验证" })).toBeNull();
    rerender(
      <OperationPlanDialog
        snapshotRequestId="new-snapshot"
        installationId="instance"
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    expect(screen.queryByRole("region", { name: "后端操作计划" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    await screen.findByRole("alert");
    expect(
      vi.mocked(operationPlan).mock.calls[vi.mocked(operationPlan).mock.calls.length - 1]?.[0]
        .snapshotRequestId,
    ).toBe("new-snapshot");
    expect(operationExecute).not.toHaveBeenCalled();
  });
  it("更换意图会丢弃迟到的预览，旧请求不锁住新选择", async () => {
    const user = userEvent.setup();
    let old!: (p: OperationPlan) => void;
    vi.mocked(operationPlan).mockReturnValueOnce(
      new Promise((resolve) => {
        old = resolve;
      }),
    );
    show();
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "计划操作" }), "bind");
    await act(async () => old(plan()));
    expect(screen.queryByRole("region", { name: "后端操作计划" })).toBeNull();
    expect((screen.getByRole("button", { name: "生成预览" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });
  it("同步重复执行和取消不会重发，取消请求期间等待真实终态", async () => {
    const user = userEvent.setup();
    let end!: (r: ExecutionRecord) => void;
    vi.mocked(operationExecute).mockReturnValue(
      new Promise((resolve) => {
        end = resolve;
      }),
    );
    show();
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    const button = await screen.findByRole("button", { name: "执行重新验证" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(operationExecute).toHaveBeenCalledOnce();
    const cancel = screen.getByRole("button", { name: "请求取消" });
    fireEvent.click(cancel);
    fireEvent.click(cancel);
    await waitFor(() => expect(operationCancel).toHaveBeenCalledExactlyOnceWith("server-plan"));
    expect(screen.getByRole("button", { name: "等待取消完成…" })).toBeTruthy();
    expect(finished).not.toHaveBeenCalled();
    await act(async () => end({ ...result, status: "cancelled", outcome: "cancelled" }));
    expect(finished).toHaveBeenCalledOnce();
    expect(screen.queryByText(/原实例已重新验证/)).toBeNull();
  });
  it("安装预览区分 mise 精确版本与 brew 公式承诺，不提交补丁版给 brew", async () => {
    const user = userEvent.setup();
    const preview = plan();
    preview.action = "install";
    preview.target.installationId = null;
    preview.target.locator = null;
    preview.target.providerPackage.providerId = "mise";
    preview.capability = { canPreview: true, canExecute: false, reason: "安装执行尚未开放" };
    vi.mocked(operationPlan).mockResolvedValue(preview);
    render(
      <OperationPlanDialog
        snapshotRequestId="snapshot"
        installationId={null}
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    await user.type(screen.getByRole("textbox", { name: "Node 精确版本" }), "22.1.0");
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    expect(operationPlan).toHaveBeenLastCalledWith({
      snapshotRequestId: "snapshot",
      intent: { action: "install", providerId: "mise", packageId: "node", version: "22.1.0" },
    });
    await user.selectOptions(screen.getByRole("combobox", { name: "安装来源" }), "brew");
    await user.clear(screen.getByRole("textbox", { name: "Node formula" }));
    await user.type(screen.getByRole("textbox", { name: "Node formula" }), "node@22");
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    expect(operationPlan).toHaveBeenLastCalledWith({
      snapshotRequestId: "snapshot",
      intent: { action: "install", providerId: "brew", packageId: "node@22", version: null },
    });
    expect(operationExecute).not.toHaveBeenCalled();
  });
});

it("执行已结束时迟到的取消失败不覆盖成功反馈", async () => {
  const user = userEvent.setup();
  let finish!: (r: ExecutionRecord) => void;
  let failCancel!: (e: unknown) => void;
  vi.mocked(operationExecute).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  vi.mocked(operationCancel).mockReturnValue(
    new Promise((_, reject) => {
      failCancel = reject;
    }),
  );
  show();
  await user.click(screen.getByRole("button", { name: "生成预览" }));
  await user.click(await screen.findByRole("button", { name: "执行重新验证" }));
  await user.click(screen.getByRole("button", { name: "请求取消" }));
  await act(async () => finish(result));
  await screen.findByText(/原实例已重新验证/);
  await act(async () => failCancel("迟到的取消错误"));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText(/原实例已重新验证/)).toBeTruthy();
});

function pythonPlan(): OperationPlan {
  const value = plan();
  value.toolId = "python";
  value.target.providerPackage = {
    providerId: "mise",
    packageId: "python",
    kind: "runtime",
    platform: "macos",
  };
  return value;
}
it("Python 验证显式绑定工具，复用签发 ID 并拒绝其他工具的执行结果", async () => {
  const user = userEvent.setup();
  vi.mocked(operationPlan).mockResolvedValue(pythonPlan());
  vi.mocked(operationExecute).mockResolvedValue(result);
  render(
    <OperationPlanDialog
      toolId="python"
      snapshotRequestId="snapshot"
      installationId="instance"
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  expect(screen.getByRole("heading", { name: "Python 操作计划" })).toBeTruthy();
  expect(screen.getAllByRole("option").map((e) => e.textContent)).toEqual([
    "重新验证此实例",
    "卸载此实例",
    "绑定此实例",
  ]);
  await user.click(screen.getByRole("button", { name: "生成预览" }));
  expect(operationPlan).toHaveBeenLastCalledWith(
    { snapshotRequestId: "snapshot", intent: { action: "verify", installationId: "instance" } },
    "python",
  );
  await user.click(await screen.findByRole("button", { name: "执行重新验证" }));
  await screen.findByText(/执行结果与当前工具计划不匹配/);
  expect(screen.queryByText(/原实例已重新验证/)).toBeNull();
});
it("Python 绑定预览提交项目作用域与精确实例", async () => {
  const user = userEvent.setup();
  vi.mocked(operationPlan).mockImplementation(async () => {
    const value = pythonPlan();
    value.action = "bind";
    value.capability = { canPreview: true, canExecute: true, reason: null };
    return value;
  });
  render(
    <OperationPlanDialog
      toolId="python"
      snapshotRequestId="snapshot"
      installationId="instance"
      initialAction="bind"
      initialBinding={{ strategy: "direct", scope: "project", projectPath: "/work/project" }}
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  expect((screen.getByRole("combobox", { name: "计划操作" }) as HTMLSelectElement).value).toBe(
    "bind",
  );
  expect((screen.getByRole("combobox", { name: "绑定方式" }) as HTMLSelectElement).value).toBe(
    "direct",
  );
  expect((screen.getByRole("combobox", { name: "绑定作用范围" }) as HTMLSelectElement).value).toBe(
    "project",
  );
  expect((screen.getByRole("textbox", { name: "绑定项目路径" }) as HTMLInputElement).value).toBe(
    "/work/project",
  );
  await user.click(screen.getByRole("button", { name: "生成预览" }));
  expect(operationPlan).toHaveBeenLastCalledWith(
    {
      snapshotRequestId: "snapshot",
      intent: {
        action: "bind",
        installationId: "instance",
        binding: { strategy: "direct", scope: "project", projectPath: "/work/project" },
      },
    },
    "python",
  );
});
it("Python 解除绑定使用显式收据并跳过 Node 绑定列表", async () => {
  const user = userEvent.setup();
  vi.mocked(operationPlan).mockImplementation(async () => {
    const value = pythonPlan();
    value.action = "unbind";
    value.capability = { canPreview: true, canExecute: true, reason: null };
    return value;
  });
  render(
    <OperationPlanDialog
      toolId="python"
      snapshotRequestId="snapshot"
      installationId="instance"
      initialAction="unbind"
      initialBindingId="python-binding-1"
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  expect((screen.getByRole("combobox", { name: "计划操作" }) as HTMLSelectElement).value).toBe(
    "unbind",
  );
  expect(screen.queryByRole("combobox", { name: "已有 Node 绑定" })).toBeNull();
  expect(screen.getByText(/将解除本应用写入的绑定并恢复原配置/)).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "生成预览" }));
  expect(operationPlan).toHaveBeenLastCalledWith(
    {
      snapshotRequestId: "snapshot",
      intent: { action: "unbind", bindingId: "python-binding-1" },
    },
    "python",
  );
});
it("Python 四渠道预览区分精确版本和 formula，没有写执行入口", async () => {
  const user = userEvent.setup();
  vi.mocked(operationPlan).mockImplementation(async (request) => {
    const value = pythonPlan();
    value.action = "install";
    value.target.installationId = null;
    value.target.locator = null;
    if (request.intent.action === "install") {
      value.target.providerPackage.providerId = request.intent.providerId;
      value.target.providerPackage.packageId = request.intent.packageId;
      value.target.providerPackage.kind =
        request.intent.providerId === "brew" ? "formula" : "runtime";
    }
    value.capability = { canPreview: true, canExecute: false, reason: "安装执行未开放" };
    return value;
  });
  render(
    <OperationPlanDialog
      toolId="python"
      snapshotRequestId="snapshot"
      installationId={null}
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  const source = screen.getByRole("combobox", { name: "安装来源" });
  expect(screen.getAllByRole("option").map((e) => e.textContent)).toEqual([
    "安装 Python",
    "mise",
    "Homebrew",
    "pyenv",
    "uv",
  ]);
  await user.type(screen.getByRole("textbox", { name: "Python 精确版本" }), "3.13.15");
  for (const providerId of ["mise", "pyenv", "uv"]) {
    await user.selectOptions(source, providerId);
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    await screen.findByText("安装执行未开放");
    expect(operationPlan).toHaveBeenLastCalledWith(
      {
        snapshotRequestId: "snapshot",
        intent: { action: "install", providerId, packageId: "python", version: "3.13.15" },
      },
      "python",
    );
    expect(screen.queryByRole("button", { name: "执行安装" })).toBeNull();
  }
  await user.selectOptions(source, "brew");
  const formula = screen.getByRole("textbox", { name: "Python formula" });
  await user.clear(formula);
  await user.type(formula, "python@3.13");
  await user.click(screen.getByRole("button", { name: "生成预览" }));
  await screen.findByText("安装执行未开放");
  expect(operationPlan).toHaveBeenLastCalledWith(
    {
      snapshotRequestId: "snapshot",
      intent: { action: "install", providerId: "brew", packageId: "python@3.13", version: null },
    },
    "python",
  );
  expect(operationExecute).not.toHaveBeenCalled();
});
it.each(["foreign", "provider"])("Python 拒绝 %s 计划回复", async (mode) => {
  const user = userEvent.setup();
  const value = mode === "foreign" ? plan() : pythonPlan();
  value.action = "install";
  value.target.providerPackage.providerId = mode === "provider" ? "uv" : "mise";
  vi.mocked(operationPlan).mockResolvedValue(value);
  render(
    <OperationPlanDialog
      toolId="python"
      snapshotRequestId="snapshot"
      installationId={null}
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  await user.type(screen.getByRole("textbox", { name: "Python 精确版本" }), "3.13.15");
  await user.click(screen.getByRole("button", { name: "生成预览" }));
  await screen.findByText(/计划与当前请求不匹配/);
  expect(screen.queryByRole("button", { name: "执行安装" })).toBeNull();
  expect(operationExecute).not.toHaveBeenCalled();
});

it.each(["install", "uninstall"] as const)(
  "Python 原生 mise %s 只有后端核定后执行，并按真实终态反馈",
  async (action) => {
    const user = userEvent.setup();
    const value = pythonPlan();
    value.action = action;
    if (action === "install") {
      value.target.installationId = null;
      value.target.locator = null;
    }
    vi.mocked(operationPlan).mockResolvedValue(value);
    vi.mocked(operationExecute).mockResolvedValue({
      ...result,
      toolId: "python",
      action,
      outcome: action === "install" ? "installed" : "uninstalled",
    });
    render(
      <OperationPlanDialog
        toolId="python"
        snapshotRequestId="snapshot"
        installationId={action === "install" ? null : "instance"}
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    if (action === "install")
      await user.type(screen.getByRole("textbox", { name: "Python 精确版本" }), "3.12.12");
    else await user.selectOptions(screen.getByRole("combobox", { name: "计划操作" }), "uninstall");
    await user.click(screen.getByRole("button", { name: "生成预览" }));
    await user.click(
      await screen.findByRole("button", { name: action === "install" ? "执行安装" : "执行卸载" }),
    );
    await screen.findByText(
      action === "install" ? /安装已通过来源重扫与版本验证/ : /已确认所选实例移除/,
    );
    expect(operationExecute).toHaveBeenCalledExactlyOnceWith("server-plan");
    expect(finished).toHaveBeenCalledOnce();
  },
);

it.each([
  ["node", "homebrew/core/node@22", "node@22"],
  ["python", "homebrew/core/python@3.13", "python@3.13"],
  ["python", "python3", "homebrew/core/python3"],
] as const)(
  "%s 原生 Homebrew 入口保留包名，并显式请求 v2 工具计划",
  async (toolId, nativeId, canonicalId) => {
    const value = toolId === "python" ? pythonPlan() : plan();
    value.action = "install";
    value.target = {
      providerPackage: {
        providerId: "brew",
        packageId: canonicalId,
        kind: "formula",
        platform: "macos",
      },
      installationId: null,
      locator: null,
    };
    vi.mocked(operationPlan).mockResolvedValue(value);
    render(
      <OperationPlanDialog
        toolId={toolId}
        snapshotRequestId="snapshot"
        installationId={null}
        initialAction="install"
        initialProviderId="brew"
        initialPackageId={nativeId}
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    expect(
      (
        screen.getByRole("textbox", {
          name: toolId === "python" ? "Python formula" : "Node formula",
        }) as HTMLInputElement
      ).value,
    ).toBe(nativeId);
    fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
    await screen.findByRole("button", { name: "执行安装" });
    expect(operationPlanV2).toHaveBeenCalledExactlyOnceWith({
      schemaVersion: 2,
      subject: { kind: "tool", toolId },
      request: {
        snapshotRequestId: "snapshot",
        intent: { action: "install", providerId: "brew", packageId: nativeId, version: null },
      },
    });
    expect(operationExecute).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("combobox", { name: "安装来源" }), {
      target: { value: "mise" },
    });
    expect(screen.queryByRole("button", { name: "执行安装" })).toBeNull();
    fireEvent.change(screen.getByRole("combobox", { name: "安装来源" }), {
      target: { value: "brew" },
    });
    expect(
      (
        screen.getByRole("textbox", {
          name: toolId === "python" ? "Python formula" : "Node formula",
        }) as HTMLInputElement
      ).value,
    ).toBe(toolId);
  },
);

it.each([
  "snapshot",
  "action",
  "tool",
  "provider",
  "package",
  "tap",
  "kind",
  "platform",
  "installation",
  "locator",
  "missing-target",
] as const)("安装拒绝不匹配的 %s 回复", async (mode) => {
  const value = pythonPlan();
  value.action = "install";
  value.target = {
    providerPackage: {
      providerId: "brew",
      packageId: "python@3.13",
      kind: "formula",
      platform: "macos",
    },
    installationId: null,
    locator: null,
  };
  if (mode === "snapshot") value.snapshotRequestId = "foreign-snapshot";
  if (mode === "action") value.action = "uninstall";
  if (mode === "tool") value.toolId = "node";
  if (mode === "provider") value.target.providerPackage.providerId = "mise";
  if (mode === "package") value.target.providerPackage.packageId = "python@3.12";
  if (mode === "tap") value.target.providerPackage.packageId = "foreign/tap/python@3.13";
  if (mode === "kind") value.target.providerPackage.kind = "runtime";
  if (mode === "platform") value.target.providerPackage.platform = "linux";
  if (mode === "installation") value.target.installationId = "old-instance";
  if (mode === "locator") value.target.locator = "/old/python";
  if (mode === "missing-target") value.target = undefined as unknown as OperationPlan["target"];
  vi.mocked(operationPlan).mockResolvedValue(value);
  render(
    <OperationPlanDialog
      toolId="python"
      snapshotRequestId="snapshot"
      installationId={null}
      initialProviderId="brew"
      initialPackageId="python@3.13"
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  await screen.findByText(/计划与当前请求不匹配/);
  expect(screen.queryByRole("region", { name: "后端操作计划" })).toBeNull();
  expect(operationExecute).not.toHaveBeenCalled();
});

it("v2 subject 与目标工具不一致时不接受计划", async () => {
  vi.mocked(operationPlanV2).mockResolvedValue({
    schemaVersion: 2,
    subject: { kind: "tool", toolId: "python" },
    plan: plan(),
    stepGraph: [],
  });
  show();
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  await screen.findByText(/计划与当前请求不匹配/);
  expect(screen.queryByRole("button", { name: "执行重新验证" })).toBeNull();
});

it.each(["verify", "uninstall", "bind", "unbind"] as const)(
  "Python %s 拒绝另一实例的计划",
  async (action) => {
    const value = pythonPlan();
    value.action = action;
    value.target.installationId = "foreign-instance";
    vi.mocked(operationPlan).mockResolvedValue(value);
    render(
      <OperationPlanDialog
        toolId="python"
        snapshotRequestId="snapshot"
        installationId="instance"
        initialAction={action === "bind" || action === "unbind" ? action : undefined}
        initialBinding={{ strategy: "direct", scope: "project", projectPath: "/project" }}
        initialBindingId={action === "unbind" ? "binding" : undefined}
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    if (action === "uninstall")
      fireEvent.change(screen.getByRole("combobox", { name: "计划操作" }), {
        target: { value: "uninstall" },
      });
    fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
    await screen.findByText(/计划与当前请求不匹配/);
    expect(screen.queryByRole("region", { name: "后端操作计划" })).toBeNull();
  },
);

it.each(["mise", "uv", "pyenv", "brew"] as const)(
  "Python %s 保留后端拒绝能力，签发可执行计划后才安装",
  async (providerId) => {
    const value = pythonPlan();
    value.action = "install";
    value.target = {
      providerPackage: {
        providerId,
        packageId: providerId === "brew" ? "python3" : "python",
        kind: providerId === "brew" ? "formula" : "runtime",
        platform: "macos",
      },
      installationId: null,
      locator: null,
    };
    value.capability = { canPreview: true, canExecute: false, reason: "尚未满足来源条件" };
    vi.mocked(operationPlan).mockResolvedValue(value);
    render(
      <OperationPlanDialog
        toolId="python"
        snapshotRequestId="snapshot"
        installationId={null}
        initialProviderId={providerId}
        initialPackageId={value.target.providerPackage.packageId}
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    if (providerId !== "brew")
      fireEvent.change(screen.getByRole("textbox", { name: "Python 精确版本" }), {
        target: { value: "3.13.15" },
      });
    fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
    await screen.findByText("尚未满足来源条件");
    expect(screen.queryByRole("button", { name: "执行安装" })).toBeNull();
    vi.mocked(operationPlan).mockResolvedValue({
      ...value,
      capability: { canPreview: true, canExecute: true, reason: null },
    });
    vi.mocked(operationExecute).mockResolvedValue({
      ...result,
      toolId: "python",
      action: "install",
      outcome: "installed",
    });
    fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
    fireEvent.click(await screen.findByRole("button", { name: "执行安装" }));
    await screen.findByText(/安装已通过来源重扫与版本验证/);
    expect(operationExecute).toHaveBeenCalledExactlyOnceWith("server-plan");
  },
);

it("快照变化使已有预览不可执行", async () => {
  const { rerender } = show();
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  await screen.findByRole("button", { name: "执行重新验证" });
  rerender(
    <OperationPlanDialog
      snapshotRequestId="changed-snapshot"
      installationId="instance"
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  expect(screen.queryByRole("button", { name: "执行重新验证" })).toBeNull();
  expect(operationExecute).not.toHaveBeenCalled();
});

it("用户显式关闭执行计划后，后台完成仍通知刷新且不重复执行", async () => {
  const close = vi.fn();
  let finish!: (result: ExecutionRecord) => void;
  vi.mocked(operationExecute).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const { unmount } = render(
    <OperationPlanDialog
      snapshotRequestId="snapshot"
      installationId="instance"
      onClose={close}
      onFinished={finished}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  fireEvent.click(await screen.findByRole("button", { name: "执行重新验证" }));
  fireEvent.click(screen.getByText("关闭", { selector: "button" }));
  expect(close).toHaveBeenCalledOnce();
  unmount();
  await act(async () => finish(result));
  expect(finished).toHaveBeenCalledOnce();
  expect(operationExecute).toHaveBeenCalledExactlyOnceWith("server-plan");
});

it("关闭计划将焦点归还导航前的可见入口", async () => {
  const opener = document.createElement("button");
  opener.textContent = "工具卡入口";
  document.body.append(opener);
  const { unmount } = render(
    <OperationPlanDialog
      snapshotRequestId="snapshot"
      installationId="instance"
      openerElement={opener}
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  unmount();
  await waitFor(() => expect(document.activeElement).toBe(opener));
  opener.remove();
});

it.each([
  "hidden",
  "collapsed",
  "display",
  "removed",
  "disabled",
  "fieldset",
  "inert",
  "unfocusable",
] as const)("导航入口 %s 时焦点退回可见的对应运行时切换", async (mode) => {
  const holder = document.createElement(
    mode === "collapsed" ? "details" : mode === "fieldset" ? "fieldset" : "div",
  );
  const opener = document.createElement(mode === "unfocusable" ? "span" : "button");
  if (mode === "disabled") (opener as HTMLButtonElement).disabled = true;
  if (mode === "fieldset") (holder as HTMLFieldSetElement).disabled = true;
  if (mode === "inert") holder.setAttribute("inert", "");
  if (mode === "hidden") holder.hidden = true;
  if (mode === "display") holder.style.display = "none";
  if (mode === "collapsed") holder.append(document.createElement("summary"));
  holder.append(opener);
  document.body.append(holder);
  const hiddenSwitch = document.createElement("button");
  hiddenSwitch.dataset.runtimeSwitch = "node";
  hiddenSwitch.hidden = true;
  const fallback = document.createElement("button");
  fallback.dataset.runtimeSwitch = "node";
  document.body.append(hiddenSwitch, fallback);
  const { unmount } = render(
    <OperationPlanDialog
      snapshotRequestId="snapshot"
      installationId="instance"
      openerElement={opener}
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  if (mode === "removed") holder.remove();
  unmount();
  await waitFor(() => expect(document.activeElement).toBe(fallback));
  holder.remove();
  hiddenSwitch.remove();
  fallback.remove();
});

it("执行中的快照变化保留原计划显示，取消仍提交原签发任务 ID", async () => {
  let finish!: (result: ExecutionRecord) => void;
  vi.mocked(operationExecute).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const { rerender } = show();
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  fireEvent.click(await screen.findByRole("button", { name: "执行重新验证" }));
  rerender(
    <OperationPlanDialog
      snapshotRequestId="changed-snapshot"
      installationId="instance"
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  expect(screen.getByRole("region", { name: "后端操作计划" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "执行中…" })).toHaveProperty("disabled", true);
  fireEvent.click(screen.getByRole("button", { name: "请求取消" }));
  await waitFor(() => expect(operationCancel).toHaveBeenCalledExactlyOnceWith("server-plan"));
  expect(screen.getByRole("button", { name: "等待取消完成…" })).toHaveProperty("disabled", true);
  await act(async () => finish({ ...result, status: "cancelled", outcome: "cancelled" }));
  expect(finished).toHaveBeenCalledOnce();
});

it.each([
  ["node", "windows", false],
  ["node", "linux", false],
  ["python", "windows", false],
  ["python", "linux", false],
  ["node", "windows", true],
  ["node", "linux", true],
  ["python", "windows", true],
  ["python", "linux", true],
] as const)(
  "%s %s 安装计划 canExecute=%s 保留只读平台边界",
  async (toolId, platform, canExecute) => {
    const value = toolId === "python" ? pythonPlan() : plan();
    value.action = "install";
    value.target = {
      providerPackage: { providerId: "mise", packageId: toolId, kind: "runtime", platform },
      installationId: null,
      locator: null,
    };
    const reason =
      toolId === "python"
        ? "Python 计划仅支持 macOS 与绝对来源路径"
        : "新操作计划当前仅支持 macOS Node 试点";
    value.capability = { canPreview: false, canExecute, reason };
    value.steps = [];
    vi.mocked(operationPlan).mockResolvedValue(value);
    render(
      <OperationPlanDialog
        toolId={toolId}
        snapshotRequestId="snapshot"
        installationId={null}
        onClose={vi.fn()}
        onFinished={finished}
      />,
    );
    fireEvent.change(
      screen.getByRole("textbox", {
        name: toolId === "python" ? "Python 精确版本" : "Node 精确版本",
      }),
      { target: { value: toolId === "python" ? "3.13.15" : "22.1.0" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
    if (canExecute) {
      await screen.findByText(/计划与当前请求不匹配/);
      expect(screen.queryByRole("region", { name: "后端操作计划" })).toBeNull();
    } else {
      await screen.findByText(reason);
      expect(screen.getByRole("region", { name: "后端操作计划" })).toBeTruthy();
      expect(screen.queryByRole("alert")).toBeNull();
    }
    expect(screen.queryByRole("button", { name: "执行安装" })).toBeNull();
    expect(operationExecute).not.toHaveBeenCalled();
  },
);

it.each(["node", "python"] as const)("%s 不接受未知平台的只读安装回复", async (toolId) => {
  const value = toolId === "python" ? pythonPlan() : plan();
  value.action = "install";
  value.target = {
    providerPackage: {
      providerId: "mise",
      packageId: toolId,
      kind: "runtime",
      platform: "unknown" as OperationPlan["target"]["providerPackage"]["platform"],
    },
    installationId: null,
    locator: null,
  };
  value.capability = { canPreview: false, canExecute: false, reason: "平台未开放" };
  vi.mocked(operationPlan).mockResolvedValue(value);
  render(
    <OperationPlanDialog
      toolId={toolId}
      snapshotRequestId="snapshot"
      installationId={null}
      onClose={vi.fn()}
      onFinished={finished}
    />,
  );
  fireEvent.change(
    screen.getByRole("textbox", {
      name: toolId === "python" ? "Python 精确版本" : "Node 精确版本",
    }),
    { target: { value: toolId === "python" ? "3.13.15" : "22.1.0" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  await screen.findByText(/计划与当前请求不匹配/);
  expect(screen.queryByText("平台未开放")).toBeNull();
  expect(screen.queryByRole("button", { name: "执行安装" })).toBeNull();
});
