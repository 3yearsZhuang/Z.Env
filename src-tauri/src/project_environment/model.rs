//! 项目声明、候选、选择与受控运行的独立传输证据。
use crate::domain::{InstallationId, ProviderPackage};
use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentRequest {
    pub request_id: String,
    pub snapshot_request_id: String,
    pub path: String,
    pub probe: bool,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RequirementState {
    Unspecified,
    Supported,
    Conflict,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Match {
    Satisfied,
    Mismatch,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Declaration {
    pub source: String,
    pub path: String,
    pub requirement: Option<String>,
    pub supported: bool,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub installation_id: InstallationId,
    pub provider_package: ProviderPackage,
    pub version: Option<String>,
    pub locator: String,
    pub executable_path: Option<String>,
    pub requirement_match: Match,
    pub usable: bool,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub kind: String,
    pub scope: String,
    pub path: Option<String>,
    pub request: Option<String>,
    pub path_entries: Vec<String>,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextResult {
    pub context: String,
    pub status: String,
    pub version: Option<String>,
    pub executable_path: Option<String>,
    pub canonical_executable_path: Option<String>,
    pub installation_id: Option<InstallationId>,
    pub requirement_match: Match,
    pub selection_match: Match,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEvidence {
    pub path: String,
    pub canonical_path: String,
    pub exists: bool,
    pub sha256: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEnvironment {
    pub schema_version: u32,
    pub request_id: String,
    pub snapshot_request_id: String,
    pub project_directory: String,
    pub configuration_path: String,
    pub observed_at: u64,
    pub declarations: Vec<Declaration>,
    pub requirement_state: RequirementState,
    pub candidates: Vec<Candidate>,
    pub inventory_complete: bool,
    pub selection: Selection,
    pub controlled: ContextResult,
    pub application_path: ContextResult,
    pub terminal: ContextResult,
    pub issues: Vec<Issue>,
    pub files: Vec<FileEvidence>,
}
