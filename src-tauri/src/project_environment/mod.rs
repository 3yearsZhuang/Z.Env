//! 项目 Node 需求、候选与受控上下文；不加载项目钩子，不自动安装或修复。
mod constraint;
mod context;
mod files;
pub mod model;
#[cfg(all(test, unix))]
mod tests;
use crate::{
    domain::*,
    inventory::has_complete_coverage,
    providers::{CancellationToken, NodeScanOptions},
};
use constraint::{Constraint, Version};
use files::Reader;
pub use model::*;
use std::path::Path;

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(9_007_199_254_740_991) as u64
}
fn issue(issues: &mut Vec<Issue>, code: &str, message: &str) {
    if !issues.iter().any(|i| i.code == code) {
        issues.push(Issue {
            code: code.into(),
            message: message.into(),
        });
    }
}
#[derive(Default)]
struct Loaded {
    declarations: Vec<Declaration>,
    ranges: Vec<Constraint>,
    unknown: bool,
    issues: Vec<Issue>,
}
impl Loaded {
    fn declaration(
        &mut self,
        source: &str,
        path: &Path,
        requirement: Option<&str>,
        reason: Option<&str>,
    ) {
        if self.declarations.len() >= 64 {
            self.unknown = true;
            issue(
                &mut self.issues,
                "declarationBudget",
                "Node 声明超过 64 条，剩余要求未知",
            );
            return;
        }
        let range = requirement.and_then(Constraint::parse);
        let supported = range.is_some() && reason.is_none();
        if supported {
            self.ranges.push(range.unwrap());
        } else {
            self.unknown = true;
        }
        self.declarations.push(Declaration {
            source: source.into(),
            path: path.to_string_lossy().into(),
            requirement: requirement.map(|s| s.chars().take(256).collect()),
            supported,
            reason: if supported {
                None
            } else {
                Some(reason.unwrap_or("版本别名或范围语法尚未支持").into())
            },
        });
    }
    fn state(&self) -> RequirementState {
        if self
            .ranges
            .iter()
            .cloned()
            .reduce(|a, b| a.intersect(&b))
            .is_some_and(|r| r.empty())
        {
            RequirementState::Conflict
        } else if self.unknown {
            RequirementState::Unknown
        } else if self.declarations.is_empty() {
            RequirementState::Unspecified
        } else {
            RequirementState::Supported
        }
    }
    fn matches(&self, version: Option<&str>) -> Match {
        let Some(v) = version.and_then(Version::parse) else {
            return Match::Unknown;
        };
        if self.ranges.iter().any(|r| !r.matches(v)) {
            Match::Mismatch
        } else if self.unknown || self.declarations.is_empty() {
            Match::Unknown
        } else {
            Match::Satisfied
        }
    }
}
fn read(reader: &mut Reader, path: &Path, loaded: &mut Loaded) -> Option<String> {
    match reader.read(path) {
        Ok(s) => s,
        Err(_) => {
            loaded.declaration(
                "文件读取",
                path,
                None,
                Some("文件不可完整读取，无法确认要求"),
            );
            None
        }
    }
}
fn no_selection() -> Selection {
    Selection {
        kind: "none".into(),
        scope: "project".into(),
        path: None,
        request: None,
        path_entries: vec![],
        reason: Some("尚无已支持的 mise Node 选择或项目 PATH 选择".into()),
    }
}
fn config_selection(
    text: &str,
    path: &Path,
    scope: &str,
    loaded: &mut Loaded,
    requirements: bool,
) -> Selection {
    let value = match toml::from_str::<toml::Value>(text) {
        Ok(v) => v,
        Err(_) => {
            if requirements {
                loaded.declaration("mise", path, None, Some("TOML 无法解析"));
            }
            let mut s = no_selection();
            s.kind = "unknown".into();
            s.path = Some(path.to_string_lossy().into());
            s.reason = Some("配置无法解析，保持实际解析未知".into());
            return s;
        }
    };
    let tools = value.get("tools").and_then(toml::Value::as_table);
    let nodes: Vec<_> = tools
        .into_iter()
        .flat_map(|t| t.iter())
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "node" | "core:node" | "nodejs" | "asdf:nodejs"
            )
        })
        .collect();
    let mut selected = no_selection();
    selected.scope = scope.into();
    selected.path = Some(path.to_string_lossy().into());
    for (key, value) in &nodes {
        let request = value.as_str();
        if requirements {
            loaded.declaration(
                &format!("mise tools.{key}"),
                path,
                request,
                if request.is_none() {
                    Some("复杂 Node 工具配置暂不推断")
                } else {
                    None
                },
            );
        }
    }
    if let Some((key, value)) = nodes.first() {
        selected.request = value.as_str().map(str::to_owned);
        selected.kind = if nodes.len() == 1
            && matches!(key.as_str(), "node" | "core:node")
            && value.as_str().is_some_and(numeric_request)
        {
            "mise"
        } else {
            "unknown"
        }
        .into();
        selected.reason = if selected.kind == "mise" {
            None
        } else {
            Some("多后端、别名或复杂 Node 选择尚不能确定解析规则".into())
        };
    }
    if let Some(paths) = value
        .get("env")
        .and_then(|v| v.get("_"))
        .and_then(|v| v.get("path"))
    {
        let entries = paths.as_array().filter(|a| a.len() <= 64).and_then(|a| {
            a.iter()
                .map(|v| {
                    v.as_str()
                        .filter(|s| {
                            s.len() <= 4096
                                && Path::new(s).is_absolute()
                                && !s.contains(['{', '$', '\n', '\r'])
                        })
                        .map(str::to_owned)
                })
                .collect::<Option<Vec<_>>>()
        });
        if nodes.is_empty() && entries.as_ref().is_some_and(|p| !p.is_empty()) {
            selected.kind = "path".into();
            selected.path_entries = entries.unwrap();
            selected.reason = None;
        } else {
            selected.kind = "unknown".into();
            selected.reason =
                Some("项目 PATH 与工具选择并存，或含动态/相对路径，暂不推断优先级".into());
        }
    }
    if ["include", "extends", "settings"]
        .iter()
        .any(|key| value.get(key).is_some())
    {
        selected.kind = "unknown".into();
        selected.reason = Some("配置包含继承、导入或额外设置，当前保持解析未知".into());
        issue(
            &mut loaded.issues,
            "configurationIncludes",
            "配置包含尚未支持的继承、导入或额外设置",
        );
    }
    selected
}
pub(super) fn numeric_request(raw: &str) -> bool {
    raw.len() <= 32
        && !raw.is_empty()
        && raw.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && Constraint::parse(raw).is_some()
}
fn load(directory: &Path, global: &Path, reader: &mut Reader) -> (Loaded, Selection) {
    let mut loaded = Loaded::default();
    let mut selections = vec![];
    for name in ["mise.toml", ".mise.toml"] {
        let path = directory.join(name);
        if let Some(text) = read(reader, &path, &mut loaded) {
            selections.push(config_selection(&text, &path, "project", &mut loaded, true));
        }
    }
    for name in [".nvmrc", ".node-version"] {
        let path = directory.join(name);
        if let Some(text) = read(reader, &path, &mut loaded) {
            loaded.declaration(name, &path, Some(text.trim()), None);
        }
    }
    let legacy = directory.join(".tool-versions");
    if let Some(text) = read(reader, &legacy, &mut loaded) {
        for line in text.lines() {
            let words: Vec<_> = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            if words
                .first()
                .is_some_and(|w| matches!(*w, "node" | "nodejs"))
            {
                loaded.declaration(
                    ".tool-versions",
                    &legacy,
                    words.get(1).copied(),
                    if words.len() != 2 {
                        Some("多版本或缺失的 Node 声明尚未支持")
                    } else {
                        None
                    },
                );
                if words.len() == 2 {
                    selections.push(Selection {
                        kind: if numeric_request(words[1]) {
                            "mise"
                        } else {
                            "unknown"
                        }
                        .into(),
                        scope: "project".into(),
                        path: Some(legacy.to_string_lossy().into()),
                        request: Some(words[1].into()),
                        path_entries: vec![],
                        reason: None,
                    });
                }
            }
        }
    }
    let package = directory.join("package.json");
    if let Some(text) = read(reader, &package, &mut loaded) {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => {
                if let Some(requirement) = value.get("engines").and_then(|v| v.get("node")) {
                    loaded.declaration(
                        "package.json engines.node",
                        &package,
                        requirement.as_str(),
                        None,
                    );
                }
            }
            Err(_) => loaded.declaration("package.json", &package, None, Some("JSON 无法解析")),
        }
    }
    let mut selection = if selections.len() > 1 {
        issue(
            &mut loaded.issues,
            "multipleProjectConfigurations",
            "存在多个项目配置来源，当前不假定覆盖顺序",
        );
        let mut s = no_selection();
        s.kind = "unknown".into();
        s.reason = Some("存在多个项目配置来源".into());
        s
    } else {
        selections.pop().unwrap_or_else(no_selection)
    };
    if selection.kind == "none" {
        if let Some(text) = read(reader, global, &mut loaded) {
            selection = config_selection(&text, global, "global", &mut loaded, false);
        }
    }
    let mut ancestor = directory.parent();
    let mut depth = 0;
    while let Some(parent) = ancestor {
        if depth >= 16 {
            issue(
                &mut loaded.issues,
                "ancestorBudget",
                "祖先配置检查达到上限，继承状态未知",
            );
            selection.kind = "unknown".into();
            break;
        }
        depth += 1;
        for name in ["mise.toml", ".mise.toml", ".tool-versions"] {
            let path = parent.join(name);
            if Path::new(&selection.path.clone().unwrap_or_default()) == path {
                continue;
            }
            if read(reader, &path, &mut loaded).is_some() {
                issue(
                    &mut loaded.issues,
                    "ancestorConfiguration",
                    "检测到祖先配置；复杂继承尚未解析",
                );
                selection.kind = "unknown".into();
                selection.reason = Some("存在祖先配置，请先明确项目覆盖关系".into());
            }
        }
        ancestor = parent.parent();
    }
    // 平台、local 与环境覆盖不能被单一 mise.toml 的静态投影掩盖。
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => Some(entries),
        Err(_) => {
            issue(
                &mut loaded.issues,
                "directoryUnavailable",
                "无法检查项目配置变体，保持解析未知",
            );
            selection.kind = "unknown".into();
            selection.reason = Some("项目目录不可完整读取".into());
            None
        }
    };
    if let Some(entries) = entries {
        let mut count = 0;
        for entry in entries {
            count += 1;
            if count > 256 {
                issue(
                    &mut loaded.issues,
                    "directoryBudget",
                    "配置变体检查达到目录预算",
                );
                selection.kind = "unknown".into();
                break;
            }
            if entry.is_err() {
                issue(
                    &mut loaded.issues,
                    "directoryEntryUnavailable",
                    "部分目录成员不可读取，配置变体未知",
                );
                selection.kind = "unknown".into();
                selection.reason = Some("项目目录成员不可完整读取".into());
            }
            if let Ok(entry) = entry {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !matches!(name.as_str(), "mise.toml" | ".mise.toml")
                    && (((name.starts_with("mise.") || name.starts_with(".mise."))
                        && name.ends_with(".toml"))
                        || matches!(name.as_str(), "mise.lock" | ".mise.lock"))
                {
                    let path = entry.path();
                    let _ = read(reader, &path, &mut loaded);
                    issue(
                        &mut loaded.issues,
                        "configurationVariant",
                        "存在 local、平台配置或 lockfile，当前不推断隐式优先级",
                    );
                    selection.kind = "unknown".into();
                    selection.reason = Some("存在额外配置变体或锁文件".into());
                }
            }
        }
    }
    (loaded, selection)
}
pub fn inspect(
    request: &EnvironmentRequest,
    snapshot: &InventorySnapshot,
    options: &NodeScanOptions,
    global: &Path,
    cancellation: &CancellationToken,
) -> Result<ProjectEnvironment, String> {
    let path = Path::new(&request.path);
    if !path.is_absolute()
        || request.path.len() > 4096
        || request.snapshot_request_id != snapshot.request_id
    {
        return Err("项目或清单身份无效，请重新读取".into());
    }
    if cancellation.is_cancelled() {
        return Err("项目核对已取消".into());
    }
    let target = crate::project::target(&request.path).map_err(|_| "无法定位项目配置目标")?;
    let directory = target.parent().ok_or("项目目录无效")?.to_path_buf();
    let directory_before = files::directory_stamp(&directory).map_err(|_| "无法核对项目目录")?;
    let mut reader = Reader::default();
    let (mut loaded, selection) = load(&directory, global, &mut reader);
    let requirement_state = loaded.state();
    let candidates = snapshot
        .installations
        .iter()
        .filter(|i| i.tool_id.0 == "node" && i.provider_package.provider_id != ProviderId::Path)
        .map(|i| {
            let usable = options.platform == Platform::Macos
                && i.provider_package.platform == options.platform
                && !i.stale
                && i.health == Health::Healthy
                && snapshot.providers.iter().any(|p| {
                    p.provider_id == i.provider_package.provider_id
                        && has_complete_coverage(p, &ToolId::from("node"))
                });
            Candidate {
                installation_id: i.installation_id.clone(),
                provider_package: i.provider_package.clone(),
                version: i.version.clone(),
                locator: i.locator.clone(),
                executable_path: i.executable_path.clone(),
                requirement_match: loaded.matches(i.version.as_deref()),
                usable,
                reason: (!usable).then(|| "来源过期、不完整或实例尚未健康验证".into()),
            }
        })
        .collect();
    let mut controlled = context::empty("controlledProject", "notRequested", "尚未运行受控验证");
    let mut application_path =
        context::empty("applicationPath", "notRequested", "尚未核对应用继承 PATH");
    if request.probe {
        if !reader.unchanged() {
            return Err("项目声明在读取期间变化，请重新核对".into());
        }
        application_path = context::application(snapshot, options, &directory, cancellation);
        if requirement_state == RequirementState::Conflict {
            controlled = context::empty(
                "controlledProject",
                "unknown",
                "声明冲突未解决，不能确认项目选择",
            );
        } else {
            controlled =
                context::controlled(&selection, snapshot, options, &directory, cancellation);
        }
        for result in [&mut controlled, &mut application_path] {
            result.requirement_match = loaded.matches(result.version.as_deref());
        }
    }
    if !reader.unchanged() || files::directory_stamp(&directory).ok() != Some(directory_before) {
        return Err("项目声明、配置路径或目录成员在核对期间变化，请重新读取".into());
    }
    if cancellation.is_cancelled() {
        return Err("项目核对已取消".into());
    }
    if requirement_state == RequirementState::Conflict {
        issue(
            &mut loaded.issues,
            "declarationConflict",
            "项目中的 Node 声明没有共同的稳定版本范围，请先解决冲突",
        );
    }
    if [&controlled, &application_path]
        .iter()
        .any(|c| c.requirement_match == Match::Mismatch)
    {
        issue(
            &mut loaded.issues,
            "runtimeMismatch",
            "已观测 Node 版本不满足项目声明；安装或配置存在不能证明环境就绪",
        );
    }
    Ok(ProjectEnvironment {
        schema_version: 1,
        request_id: request.request_id.clone(),
        snapshot_request_id: snapshot.request_id.clone(),
        project_directory: directory.to_string_lossy().into(),
        configuration_path: target.to_string_lossy().into(),
        observed_at: now(),
        declarations: loaded.declarations,
        requirement_state,
        candidates,
        inventory_complete: [
            ProviderId::Mise,
            ProviderId::Brew,
            ProviderId::Nvm,
            ProviderId::Path,
        ]
        .into_iter()
        .all(|id| {
            snapshot
                .providers
                .iter()
                .any(|p| p.provider_id == id && has_complete_coverage(p, &ToolId::from("node")))
        }),
        selection,
        controlled,
        application_path,
        terminal: context::empty(
            "userTerminal",
            "unknown",
            "未读取用户终端或 IDE 的实际环境，需在对应上下文另行验证",
        ),
        issues: loaded.issues,
        files: reader.evidence(),
    })
}
