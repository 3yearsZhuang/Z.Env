import { invoke } from "@tauri-apps/api/core";
import type { ProviderPackage } from "./inventory";
import { inventoryRead } from "./inventoryRead";
export type RequirementMatch = "satisfied" | "mismatch" | "unknown";
export interface EnvironmentContext {
  context: string;
  status: string;
  version: string | null;
  executablePath: string | null;
  canonicalExecutablePath: string | null;
  installationId: string | null;
  requirementMatch: RequirementMatch;
  selectionMatch: RequirementMatch;
  reason: string;
}
export interface ProjectCandidate {
  installationId: string;
  providerPackage: ProviderPackage;
  version: string | null;
  locator: string;
  executablePath: string | null;
  requirementMatch: RequirementMatch;
  usable: boolean;
  reason: string | null;
}
export interface ProjectEnvironment {
  schemaVersion: 1;
  requestId: string;
  snapshotRequestId: string;
  projectDirectory: string;
  configurationPath: string;
  observedAt: number;
  declarations: {
    source: string;
    path: string;
    requirement: string | null;
    supported: boolean;
    reason: string | null;
  }[];
  requirementState: "unspecified" | "supported" | "conflict" | "unknown";
  candidates: ProjectCandidate[];
  inventoryComplete: boolean;
  selection: {
    kind: string;
    scope: string;
    path: string | null;
    request: string | null;
    pathEntries: string[];
    reason: string | null;
  };
  controlled: EnvironmentContext;
  applicationPath: EnvironmentContext;
  terminal: EnvironmentContext;
  issues: { code: string; message: string }[];
  files: { path: string; canonicalPath: string; exists: boolean; sha256: string | null }[];
}
export function projectEnvironment(request: {
  requestId: string;
  snapshotRequestId: string;
  path: string;
  probe: boolean;
}): Promise<ProjectEnvironment> {
  return inventoryRead(() => invoke("project_environment", { request }));
}
