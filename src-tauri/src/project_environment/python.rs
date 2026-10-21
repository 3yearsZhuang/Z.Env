//! Python 项目声明、候选匹配与当前选择；只读，不写配置、不执行解释器。
use super::files::Reader;
use super::model::*;
use super::now;
use crate::domain::*;
use std::path::{Path, PathBuf};

const MAX_DECLARATIONS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Version(u32, u32, u32);

impl Version {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() || raw.len() > 32 {
            return None;
        }
        let parts: Vec<_> = raw.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let mut numbers = [0_u32; 3];
        for (index, part) in parts.iter().enumerate() {
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            numbers[index] = part.parse().ok()?;
        }
        Some(Self(numbers[0], numbers[1], numbers[2]))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Prefix {
    Major(u32),
    Minor(u32, u32),
}

impl Prefix {
    fn parse(raw: &str) -> Option<Self> {
        let parts: Vec<_> = raw.split('.').collect();
        if parts.is_empty() || parts.len() > 2 {
            return None;
        }
        let mut numbers = [0_u32; 2];
        for (index, part) in parts.iter().enumerate() {
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            numbers[index] = part.parse().ok()?;
        }
        match parts.len() {
            1 => Some(Self::Major(numbers[0])),
            2 => Some(Self::Minor(numbers[0], numbers[1])),
            _ => None,
        }
    }
    fn matches(self, version: Version) -> bool {
        match self {
            Self::Major(major) => version.0 == major,
            Self::Minor(major, minor) => version.0 == major && version.1 == minor,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exclusion {
    Exact(Version),
    Prefix(Prefix),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Requirement {
    lower: Option<(Version, bool)>,
    upper: Option<(Version, bool)>,
    exact: Option<Version>,
    excludes: Vec<Exclusion>,
    conflict: bool,
}

impl Requirement {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() || raw.len() > 256 {
            return None;
        }
        let mut requirement = Self::default();
        for raw_clause in raw.split(',') {
            let clause = raw_clause.trim();
            if clause.is_empty() {
                return None;
            }
            if clause == "*" {
                continue;
            }
            if clause.starts_with("===") || clause.contains("||") || clause.contains(' ') {
                return None;
            }
            let (operator, argument) = if let Some(rest) = clause.strip_prefix("~=") {
                ("~=", rest)
            } else if let Some(rest) = clause.strip_prefix("==") {
                ("==", rest)
            } else if let Some(rest) = clause.strip_prefix("!=") {
                ("!=", rest)
            } else if let Some(rest) = clause.strip_prefix(">=") {
                (">=", rest)
            } else if let Some(rest) = clause.strip_prefix("<=") {
                ("<=", rest)
            } else if let Some(rest) = clause.strip_prefix('>') {
                (">", rest)
            } else if let Some(rest) = clause.strip_prefix('<') {
                ("<", rest)
            } else {
                ("", clause)
            };
            if argument.is_empty() {
                return None;
            }
            match operator {
                "~=" => {
                    let parts: Vec<_> = argument.split('.').collect();
                    if !(2..=3).contains(&parts.len()) {
                        return None;
                    }
                    let lower = Version::parse(argument)?;
                    let upper = if parts.len() == 2 {
                        Version(lower.0.checked_add(1)?, 0, 0)
                    } else {
                        Version(lower.0, lower.1.checked_add(1)?, 0)
                    };
                    requirement.lower(lower, true);
                    requirement.upper(upper, false);
                }
                ">=" => requirement.lower(Version::parse(argument)?, true),
                ">" => requirement.lower(Version::parse(argument)?, false),
                "<=" => requirement.upper(Version::parse(argument)?, true),
                "<" => requirement.upper(Version::parse(argument)?, false),
                "==" | "!=" => {
                    let exclusion = operator == "!=";
                    if argument == "*" {
                        continue;
                    }
                    if let Some(prefix) = argument.strip_suffix(".*") {
                        let prefix = Prefix::parse(prefix)?;
                        if exclusion {
                            requirement.excludes.push(Exclusion::Prefix(prefix));
                        } else {
                            match prefix {
                                Prefix::Major(major) => {
                                    requirement.lower(Version(major, 0, 0), true);
                                    requirement.upper(Version(major.checked_add(1)?, 0, 0), false);
                                }
                                Prefix::Minor(major, minor) => {
                                    requirement.lower(Version(major, minor, 0), true);
                                    requirement
                                        .upper(Version(major, minor.checked_add(1)?, 0), false);
                                }
                            }
                        }
                    } else {
                        let version = Version::parse(argument)?;
                        if exclusion {
                            requirement.excludes.push(Exclusion::Exact(version));
                        } else {
                            requirement.exact(version);
                        }
                    }
                }
                "" => {
                    let parts: Vec<_> = argument.split('.').collect();
                    if !(1..=3).contains(&parts.len()) {
                        return None;
                    }
                    if parts.len() == 3 {
                        requirement.exact(Version::parse(argument)?);
                    } else {
                        let prefix = Prefix::parse(argument)?;
                        match prefix {
                            Prefix::Major(major) => {
                                requirement.lower(Version(major, 0, 0), true);
                                requirement.upper(Version(major.checked_add(1)?, 0, 0), false);
                            }
                            Prefix::Minor(major, minor) => {
                                requirement.lower(Version(major, minor, 0), true);
                                requirement.upper(Version(major, minor.checked_add(1)?, 0), false);
                            }
                        }
                    }
                }
                _ => return None,
            }
        }
        requirement.normalize();
        Some(requirement)
    }
    fn lower(&mut self, version: Version, inclusive: bool) {
        match self.lower {
            None => self.lower = Some((version, inclusive)),
            Some((current, _)) => {
                if version > current || (version == current && !inclusive) {
                    self.lower = Some((version, inclusive));
                }
            }
        }
    }
    fn upper(&mut self, version: Version, inclusive: bool) {
        match self.upper {
            None => self.upper = Some((version, inclusive)),
            Some((current, _)) => {
                if version < current || (version == current && !inclusive) {
                    self.upper = Some((version, inclusive));
                }
            }
        }
    }
    fn exact(&mut self, version: Version) {
        match self.exact {
            Some(current) if current != version => self.conflict = true,
            Some(_) => {}
            None => self.exact = Some(version),
        }
    }
    fn normalize(&mut self) {
        if let Some(exact) = self.exact {
            if self.lower.is_some_and(|(version, inclusive)| {
                exact < version || (exact == version && !inclusive)
            }) || self.upper.is_some_and(|(version, inclusive)| {
                exact > version || (exact == version && !inclusive)
            }) {
                self.conflict = true;
            }
            self.lower = Some((exact, true));
            self.upper = Some((exact, true));
        }
        if let (Some((low, low_inclusive)), Some((high, high_inclusive))) = (self.lower, self.upper)
        {
            if low > high || (low == high && !(low_inclusive && high_inclusive)) {
                self.conflict = true;
            }
        }
        if self.conflict {
            return;
        }
        if let (Some((low, true)), Some((high, true))) = (self.lower, self.upper) {
            if low == high
                && self
                    .excludes
                    .iter()
                    .any(|exclusion| exclusion_matches(*exclusion, low))
            {
                self.conflict = true;
            }
        }
    }
    fn merge(&mut self, other: &Self) {
        self.conflict |= other.conflict;
        if let Some(exact) = other.exact {
            self.exact(exact);
        }
        if let Some((version, inclusive)) = other.lower {
            self.lower(version, inclusive);
        }
        if let Some((version, inclusive)) = other.upper {
            self.upper(version, inclusive);
        }
        self.excludes.extend(other.excludes.iter().copied());
        self.normalize();
    }
    fn matches(&self, version: Version) -> bool {
        if self.conflict {
            return false;
        }
        let lower = self
            .lower
            .is_none_or(|(bound, inclusive)| version > bound || (inclusive && version == bound));
        let upper = self
            .upper
            .is_none_or(|(bound, inclusive)| version < bound || (inclusive && version == bound));
        lower
            && upper
            && !self
                .excludes
                .iter()
                .any(|exclusion| exclusion_matches(*exclusion, version))
    }
}

fn exclusion_matches(exclusion: Exclusion, version: Version) -> bool {
    match exclusion {
        Exclusion::Exact(target) => version == target,
        Exclusion::Prefix(prefix) => prefix.matches(version),
    }
}

#[derive(Default)]
struct Loaded {
    declarations: Vec<Declaration>,
    requirements: Vec<Requirement>,
    unknown: bool,
    issues: Vec<Issue>,
    selections: Vec<PythonSelection>,
}

impl Loaded {
    fn declaration(
        &mut self,
        source: &str,
        path: &Path,
        requirement: Option<&str>,
        reason: Option<&'static str>,
    ) {
        if self.declarations.len() >= MAX_DECLARATIONS {
            self.unknown = true;
            issue(
                &mut self.issues,
                "declarationBudget",
                "Python 声明超过 64 条",
            );
            return;
        }
        let parsed = requirement
            .filter(|_| reason.is_none())
            .and_then(Requirement::parse);
        if let Some(parsed) = &parsed {
            self.requirements.push(parsed.clone());
        } else {
            self.unknown = true;
        }
        self.declarations.push(Declaration {
            source: source.into(),
            path: path.to_string_lossy().into(),
            requirement: requirement.map(|value| value.chars().take(256).collect()),
            supported: parsed.is_some(),
            reason: if parsed.is_some() {
                None
            } else {
                Some(reason.unwrap_or("Python 版本范围语法尚未支持").into())
            },
        });
    }
    fn state(&self) -> RequirementState {
        let mut combined = Requirement::default();
        for requirement in &self.requirements {
            combined.merge(requirement);
        }
        if !self.requirements.is_empty() && combined.conflict {
            RequirementState::Conflict
        } else if self.unknown {
            RequirementState::Unknown
        } else if self.declarations.is_empty() {
            RequirementState::Unspecified
        } else {
            RequirementState::Supported
        }
    }
    fn combined(&self) -> Requirement {
        let mut combined = Requirement::default();
        for requirement in &self.requirements {
            combined.merge(requirement);
        }
        combined
    }
}

fn issue(issues: &mut Vec<Issue>, code: &str, message: &str) {
    if !issues.iter().any(|existing| existing.code == code) {
        issues.push(Issue {
            code: code.into(),
            message: message.into(),
        });
    }
}

fn read(reader: &mut Reader, path: &Path, loaded: &mut Loaded) -> Option<String> {
    match reader.read(path) {
        Ok(text) => text,
        Err(_) => {
            loaded.declaration(
                "文件读取",
                path,
                None,
                Some("文件不可完整读取，无法确认 Python 要求"),
            );
            None
        }
    }
}

fn numeric_request(raw: &str) -> bool {
    Requirement::parse(raw).is_some()
}

fn mise_config(text: &str, path: &Path, loaded: &mut Loaded) {
    let value: toml::Value = match toml::from_str(text) {
        Ok(value) => value,
        Err(_) => {
            loaded.declaration("mise", path, None, Some("TOML 无法解析"));
            return;
        }
    };
    let tools = value.get("tools").and_then(toml::Value::as_table);
    for key in ["python", "core:python"] {
        let Some(value) = tools.and_then(|tools| tools.get(key)) else {
            continue;
        };
        match value.as_str().filter(|request| numeric_request(request)) {
            Some(request) => {
                loaded.declaration(&format!("mise tools.{key}"), path, Some(request), None);
                loaded.selections.push(PythonSelection {
                    kind: "mise".into(),
                    path: Some(path.to_string_lossy().into()),
                    requirement: Some(request.chars().take(256).collect()),
                    reason: None,
                });
            }
            None => {
                loaded.declaration(
                    &format!("mise tools.{key}"),
                    path,
                    value.as_str(),
                    Some("复杂 Python 工具配置暂不推断"),
                );
                loaded.selections.push(PythonSelection {
                    kind: "unknown".into(),
                    path: Some(path.to_string_lossy().into()),
                    requirement: None,
                    reason: Some("复杂或不受支持的 Python 工具配置".into()),
                });
            }
        }
    }
    if ["include", "extends", "settings"]
        .iter()
        .any(|key| value.get(*key).is_some())
    {
        issue(
            &mut loaded.issues,
            "configurationIncludes",
            "配置包含尚未支持的继承、导入或额外设置",
        );
    }
}

fn python_version_file(text: &str, path: &Path, loaded: &mut Loaded) {
    let versions: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if versions.len() == 1 {
        loaded.declaration(".python-version", path, Some(versions[0]), None);
        return;
    }
    loaded.declaration(
        ".python-version",
        path,
        None,
        Some("多版本或缺失的 .python-version 尚未支持"),
    );
}

fn pyproject(text: &str, path: &Path, loaded: &mut Loaded) {
    let value: toml::Value = match toml::from_str(text) {
        Ok(value) => value,
        Err(_) => {
            loaded.declaration("pyproject", path, None, Some("TOML 无法解析"));
            return;
        }
    };
    let Some(requirement) = value
        .get("project")
        .and_then(|project| project.get("requires-python"))
    else {
        return;
    };
    match requirement.as_str() {
        Some(requirement) => {
            loaded.declaration("pyproject requires-python", path, Some(requirement), None)
        }
        None => loaded.declaration(
            "pyproject requires-python",
            path,
            None,
            Some("requires-python 不是字符串，暂不推断"),
        ),
    }
}

fn tool_versions(text: &str, path: &Path, loaded: &mut Loaded) {
    for line in text.lines() {
        let words: Vec<_> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if words.first().is_some_and(|word| *word == "python") {
            if words.len() == 2 && numeric_request(words[1]) {
                loaded.declaration(".tool-versions", path, Some(words[1]), None);
            } else {
                loaded.declaration(
                    ".tool-versions",
                    path,
                    words.get(1).copied(),
                    Some("多版本或不受支持的 Python 声明"),
                );
            }
        }
    }
}

fn load(directory: &Path, reader: &mut Reader) -> Loaded {
    let mut loaded = Loaded::default();
    for name in ["mise.toml", ".mise.toml"] {
        let path = directory.join(name);
        if let Some(text) = read(reader, &path, &mut loaded) {
            mise_config(&text, &path, &mut loaded);
        }
    }
    let path = directory.join(".python-version");
    if let Some(text) = read(reader, &path, &mut loaded) {
        python_version_file(&text, &path, &mut loaded);
        loaded.selections.push(PythonSelection {
            kind: "python-version".into(),
            path: Some(path.to_string_lossy().into()),
            requirement: None,
            reason: Some("读取 .python-version 作为声明；写入目标仍以 mise 配置为准".into()),
        });
    }
    let path = directory.join("pyproject.toml");
    if let Some(text) = read(reader, &path, &mut loaded) {
        pyproject(&text, &path, &mut loaded);
    }
    let path = directory.join(".tool-versions");
    if let Some(text) = read(reader, &path, &mut loaded) {
        tool_versions(&text, &path, &mut loaded);
    }
    loaded
}

fn resolve_directory(path: &str) -> Result<PathBuf, &'static str> {
    let raw = Path::new(path);
    if !raw.is_absolute() {
        return Err("项目路径必须是绝对路径");
    }
    let canonical = raw.canonicalize().map_err(|_| "项目路径不可读取")?;
    if canonical.is_dir() {
        Ok(canonical)
    } else if canonical.is_file() {
        canonical
            .parent()
            .map(Path::to_path_buf)
            .ok_or("项目路径没有父目录")
    } else {
        Err("项目路径不是目录或配置文件")
    }
}

fn selection(loaded: &Loaded) -> PythonSelection {
    match loaded.selections.as_slice() {
        [] => PythonSelection {
            kind: "none".into(),
            path: None,
            requirement: None,
            reason: Some("没有发现 mise Python 选择或 .python-version".into()),
        },
        [only] => only.clone(),
        _ => PythonSelection {
            kind: "unknown".into(),
            path: loaded.selections.first().and_then(|item| item.path.clone()),
            requirement: None,
            reason: Some("多个项目配置同时声明 Python，未推断优先级".into()),
        },
    }
}

fn candidates(
    snapshot: &InventorySnapshot,
    state: &RequirementState,
    combined: &Requirement,
) -> Vec<PythonCandidate> {
    let mut values: Vec<_> = snapshot
        .installations
        .iter()
        .filter(|installation| installation.tool_id == ToolId::from("python"))
        .map(|installation| {
            let version = installation.version.as_deref().and_then(Version::parse);
            let requirement_match = match (state, version) {
                (RequirementState::Supported, Some(version)) if combined.matches(version) => {
                    Match::Satisfied
                }
                (RequirementState::Supported, Some(_)) => Match::Mismatch,
                _ => Match::Unknown,
            };
            let usable = installation.health == Health::Healthy
                && !installation.stale
                && installation.provider_package.provider_id != ProviderId::Path
                && version.is_some();
            PythonCandidate {
                installation_id: installation.installation_id.clone(),
                provider_id: installation.provider_package.provider_id,
                version: installation.version.clone(),
                locator: installation.locator.clone(),
                requirement_match,
                usable,
                reason: if usable {
                    None
                } else if installation.provider_package.provider_id == ProviderId::Path {
                    Some("PATH 记录只作观测，不能作为项目绑定目标".into())
                } else if installation.stale {
                    Some("来源数据已过期".into())
                } else if installation.health != Health::Healthy {
                    Some("实例未通过隔离探针".into())
                } else {
                    Some("缺少精确版本".into())
                },
            }
        })
        .collect();
    values.sort_by(|left, right| {
        (&right.version, &left.provider_id, &left.locator).cmp(&(
            &left.version,
            &right.provider_id,
            &right.locator,
        ))
    });
    values
}

pub fn inspect_python(
    snapshot: &InventorySnapshot,
    request: &PythonProjectRequest,
) -> Result<PythonProjectEnvironment, &'static str> {
    let directory = resolve_directory(&request.path)?;
    let mut reader = Reader::default();
    let loaded = load(&directory, &mut reader);
    let state = loaded.state();
    let combined = loaded.combined();
    let inventory_complete = snapshot.providers.iter().all(|provider| {
        provider.coverage.tool_ids.contains(&ToolId::from("python"))
            && !provider.stale
            && provider.error.is_none()
            && provider.coverage.complete
            && !provider.coverage.truncated
    });
    let selection = selection(&loaded);
    let mut issues = loaded.issues.clone();
    if !inventory_complete {
        issue(
            &mut issues,
            "inventoryIncomplete",
            "Python 来源未完整读取，候选与匹配不能作为完整结论",
        );
    }
    if state == RequirementState::Conflict {
        issue(
            &mut issues,
            "requirementConflict",
            "项目 Python 声明互相冲突，无法确定可满足的版本",
        );
    }
    if state == RequirementState::Unknown {
        issue(
            &mut issues,
            "requirementUnknown",
            "项目包含尚未支持的 Python 声明，保持未知",
        );
    }
    Ok(PythonProjectEnvironment {
        schema_version: 1,
        request_id: request.request_id.clone(),
        snapshot_request_id: request.snapshot_request_id.clone(),
        project_directory: directory.to_string_lossy().into(),
        observed_at: now(),
        declarations: loaded.declarations,
        requirement_state: state.clone(),
        candidates: candidates(snapshot, &state, &combined),
        inventory_complete,
        selection,
        issues,
        files: reader.evidence(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Health, Installation, Ownership, ProviderPackage, ProviderReport, ScanCoverage,
    };
    use std::{fs, path::Path};

    fn version(raw: &str) -> Version {
        Version::parse(raw).unwrap()
    }

    #[test]
    fn pep440_subset_parses_exact_prefix_ranges_and_exclusions() {
        for (raw, ok) in [
            ("3.12.13", true),
            ("3.12", true),
            ("3", true),
            (">=3.10", true),
            (">=3.10,<3.13", true),
            ("~=3.12", true),
            ("~=3.12.1", true),
            ("==3.12.*", true),
            ("!=3.12.13", true),
            (">=3.10,!=3.11.0", true),
            ("*", true),
            ("^3.12", false),
            ("3.12 || 3.13", false),
            (">=3.10 <3.13", false),
            ("latest", false),
            ("3.12.x", false),
            ("", false),
        ] {
            assert_eq!(Requirement::parse(raw).is_some(), ok, "{raw}");
        }
    }

    #[test]
    fn requirement_matching_and_conflicts_are_explicit() {
        let exact = Requirement::parse("3.12.13").unwrap();
        assert!(exact.matches(version("3.12.13")));
        assert!(!exact.matches(version("3.12.12")));
        let minor = Requirement::parse("3.12").unwrap();
        assert!(minor.matches(version("3.12.99")));
        assert!(!minor.matches(version("3.13.0")));
        let range = Requirement::parse(">=3.10,<3.13,!=3.12.0").unwrap();
        assert!(range.matches(version("3.11.9")));
        assert!(!range.matches(version("3.12.0")));
        assert!(!range.matches(version("3.13.0")));
        let compatible = Requirement::parse("~=3.12.1").unwrap();
        assert!(compatible.matches(version("3.12.9")));
        assert!(!compatible.matches(version("3.13.0")));
        let mut conflict = Requirement::parse(">=3.13").unwrap();
        conflict.merge(&Requirement::parse("<3.12").unwrap());
        assert!(conflict.conflict);
        let mut excluded = Requirement::parse("3.12.13").unwrap();
        excluded.merge(&Requirement::parse("!=3.12.13").unwrap());
        assert!(excluded.conflict);
        let mut wildcard = Requirement::parse("==3.12.*").unwrap();
        wildcard.merge(&Requirement::parse(">=3.12.5").unwrap());
        assert!(wildcard.matches(version("3.12.5")));
        assert!(!wildcard.matches(version("3.11.9")));
    }

    fn write(directory: &Path, name: &str, text: &str) {
        fs::write(directory.join(name), text).unwrap();
    }

    fn package(provider: ProviderId) -> ProviderPackage {
        ProviderPackage {
            provider_id: provider,
            package_id: "python".into(),
            kind: PackageKind::Runtime,
            platform: Platform::Macos,
        }
    }

    fn installation(id: &str, provider: ProviderId, version: &str, health: Health) -> Installation {
        Installation {
            installation_id: InstallationId::from(id),
            provider_package: package(provider),
            tool_id: ToolId::from("python"),
            locator: format!("/tmp/{id}"),
            executable_path: Some(format!("/tmp/{id}/bin/python3")),
            canonical_executable_path: Some(format!("/tmp/{id}/bin/python3")),
            version: Some(version.into()),
            architecture: Some("arm64".into()),
            health,
            ownership: Ownership::Observed,
            evidence: vec![],
            stale: false,
        }
    }

    fn snapshot(installations: Vec<Installation>, complete: bool) -> InventorySnapshot {
        InventorySnapshot {
            schema_version: 1,
            request_id: "snapshot".into(),
            observed_at: 0,
            tools: vec![],
            providers: vec![ProviderReport {
                provider_id: ProviderId::Mise,
                status: ProviderStatus::Ok,
                observed_at: 0,
                stale: false,
                coverage: ScanCoverage {
                    tool_ids: vec![ToolId::from("python")],
                    complete,
                    truncated: false,
                },
                error: None,
            }],
            installations,
            aliases: vec![],
        }
    }

    fn request(path: &Path) -> PythonProjectRequest {
        PythonProjectRequest {
            request_id: "request".into(),
            snapshot_request_id: "snapshot".into(),
            path: path.to_string_lossy().into(),
        }
    }

    #[test]
    fn declarations_from_mise_python_version_and_pyproject_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        write(directory.path(), "mise.toml", "[tools]\npython = '3.12'\n");
        write(directory.path(), ".python-version", "3.12.13\n");
        write(
            directory.path(),
            "pyproject.toml",
            "[project]\nrequires-python = '>=3.10,<3.13'\n",
        );
        let report = inspect_python(&snapshot(vec![], true), &request(directory.path())).unwrap();
        assert_eq!(report.requirement_state, RequirementState::Supported);
        assert_eq!(report.declarations.len(), 3);
        assert!(report.declarations.iter().all(|item| item.supported));
        assert_eq!(report.selection.kind, "unknown");
        assert!(report.inventory_complete);
    }

    #[test]
    fn conflicting_and_unsupported_declarations_stay_unknown() {
        let directory = tempfile::tempdir().unwrap();
        write(
            directory.path(),
            "mise.toml",
            "[tools]\npython = '>=3.13'\n",
        );
        write(directory.path(), ".python-version", "3.12.13\n");
        let report = inspect_python(&snapshot(vec![], true), &request(directory.path())).unwrap();
        assert_eq!(report.requirement_state, RequirementState::Conflict);
        write(
            directory.path(),
            "mise.toml",
            "[tools]\npython = { version = '3.12' }\n",
        );
        let report = inspect_python(&snapshot(vec![], true), &request(directory.path())).unwrap();
        assert_eq!(report.requirement_state, RequirementState::Unknown);
    }

    #[test]
    fn candidates_match_versions_and_skip_unusable_instances() {
        let directory = tempfile::tempdir().unwrap();
        write(directory.path(), "mise.toml", "[tools]\npython = '3.12'\n");
        let snapshot = snapshot(
            vec![
                installation("healthy", ProviderId::Mise, "3.12.13", Health::Healthy),
                installation("wrong", ProviderId::Mise, "3.13.1", Health::Healthy),
                installation("broken", ProviderId::Mise, "3.12.12", Health::Broken),
                installation("path", ProviderId::Path, "3.12.13", Health::Healthy),
            ],
            true,
        );
        let report = inspect_python(&snapshot, &request(directory.path())).unwrap();
        assert!(report.inventory_complete);
        let healthy = report
            .candidates
            .iter()
            .find(|candidate| candidate.installation_id.0 == "healthy")
            .unwrap();
        assert_eq!(healthy.requirement_match, Match::Satisfied);
        assert!(healthy.usable);
        let wrong = report
            .candidates
            .iter()
            .find(|candidate| candidate.installation_id.0 == "wrong")
            .unwrap();
        assert_eq!(wrong.requirement_match, Match::Mismatch);
        let broken = report
            .candidates
            .iter()
            .find(|candidate| candidate.installation_id.0 == "broken")
            .unwrap();
        assert!(!broken.usable);
        let path = report
            .candidates
            .iter()
            .find(|candidate| candidate.installation_id.0 == "path")
            .unwrap();
        assert!(!path.usable);
        assert!(path.reason.as_deref().unwrap().contains("PATH"));
    }

    #[test]
    fn shared_python_project_contract_roundtrips_without_machine_state() {
        let raw = include_str!("../../../fixtures/project-environment/python-project-v1.json");
        let report: PythonProjectEnvironment = serde_json::from_str(raw).unwrap();
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(
            value,
            serde_json::from_str::<serde_json::Value>(raw).unwrap()
        );
        let text = value.to_string();
        assert!(!text.contains("PRIVATE"));
        assert!(!text.contains("/Users/"));
        assert!(report.inventory_complete);
        assert_eq!(report.requirement_state, RequirementState::Supported);
    }

    #[test]
    fn relative_paths_and_missing_directories_are_rejected() {
        assert!(inspect_python(
            &snapshot(vec![], true),
            &PythonProjectRequest {
                request_id: "r".into(),
                snapshot_request_id: "s".into(),
                path: "relative/project".into(),
            }
        )
        .is_err());
        assert!(inspect_python(
            &snapshot(vec![], true),
            &PythonProjectRequest {
                request_id: "r".into(),
                snapshot_request_id: "s".into(),
                path: "/definitely/missing/zenv-project".into(),
            }
        )
        .is_err());
    }
}
