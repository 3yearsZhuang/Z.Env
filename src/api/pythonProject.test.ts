import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  pythonBindings,
  pythonProject,
  validatePythonBindings,
  validatePythonProject,
  type PythonProjectEnvironment,
} from "./pythonProject";
import contract from "../../fixtures/project-environment/python-project-v1.json";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => vi.resetAllMocks());

function report(): PythonProjectEnvironment {
  return {
    schemaVersion: 1,
    requestId: "python-project-1",
    snapshotRequestId: "snapshot-1",
    projectDirectory: "/work/project",
    observedAt: 1792512000000,
    declarations: [
      {
        source: "mise tools.python",
        path: "/work/project/mise.toml",
        requirement: "3.12",
        supported: true,
        reason: null,
      },
    ],
    requirementState: "supported",
    candidates: [
      {
        installationId: "python-mise-3.12.13",
        providerId: "mise",
        version: "3.12.13",
        locator: "/Users/contract/.local/share/mise/installs/python/3.12.13",
        requirementMatch: "satisfied",
        usable: true,
        reason: null,
      },
    ],
    inventoryComplete: true,
    selection: {
      kind: "mise",
      path: "/work/project/mise.toml",
      requirement: "3.12",
      reason: null,
    },
    issues: [],
    files: [
      {
        path: "/work/project/mise.toml",
        canonicalPath: "/work/project/mise.toml",
        exists: true,
        sha256: "a".repeat(64),
      },
    ],
  };
}

it("接受完整的 Python 项目报告并原样返回", () => {
  expect(validatePythonProject(report())).toEqual(report());
});

it("共享契约样本通过校验", () => {
  expect(validatePythonProject(contract)).toEqual(contract);
});

it("调用 python_project_environment 并校验返回值", async () => {
  vi.mocked(invoke).mockResolvedValue(report());
  const request = {
    requestId: "python-project-1",
    snapshotRequestId: "snapshot-1",
    path: "/work/project",
  };
  await expect(pythonProject(request)).resolves.toEqual(report());
  expect(invoke).toHaveBeenCalledExactlyOnceWith("python_project_environment", { request });
});

it("拒绝未来版本、未知状态、跨来源候选与超预算记录", () => {
  const mutate = (edit: (value: ReturnType<typeof report>) => void) => {
    const value = structuredClone(report());
    edit(value);
    return value;
  };
  for (const value of [
    { ...report(), schemaVersion: 2 },
    mutate((v) => {
      v.requirementState = "broken" as never;
    }),
    mutate((v) => {
      v.candidates[0].providerId = "cargo" as never;
    }),
    mutate((v) => {
      v.candidates[0].requirementMatch = "maybe" as never;
    }),
    mutate((v) => {
      v.candidates = Array.from({ length: 257 }, () => report().candidates[0]);
    }),
    mutate((v) => {
      v.selection.reason = "x".repeat(513);
    }),
    mutate((v) => {
      v.files[0].sha256 = 42 as never;
    }),
    mutateEmpty(),
  ]) {
    expect(() => validatePythonProject(value)).toThrow();
  }
});

function mutateEmpty() {
  const value = report();
  // @ts-expect-error 缺少候选字段，运行时必须拒绝
  value.candidates = [{}];
  return value;
}

it("Python 绑定列表通过校验并按后端原样返回", async () => {
  const list = [
    {
      bindingId: "python-binding-1",
      installationId: "python-mise-3.13.15",
      version: "3.13.15",
      locator: "/fixture/.local/share/mise/installs/python/3.13.15",
      configPath: "/work/project/mise.toml",
      scope: "project" as const,
      state: "bound",
    },
  ];
  expect(validatePythonBindings(list)).toEqual(list);
  vi.mocked(invoke).mockResolvedValue(list);
  await expect(pythonBindings()).resolves.toEqual(list);
  expect(invoke).toHaveBeenCalledWith("python_bindings");
});

it("拒绝未来版本、跨作用域和缺失字段的 Python 绑定", () => {
  for (const value of [
    {},
    [
      {
        ...{
          bindingId: "b",
          installationId: "i",
          version: "3.13.15",
          locator: "/l",
          configPath: "/c",
          scope: "registration",
          state: "bound",
        },
      },
    ],
    [{}],
    new Array(33).fill({}),
  ]) {
    expect(() => validatePythonBindings(value)).toThrow();
  }
});
