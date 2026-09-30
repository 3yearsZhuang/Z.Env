// 项目发现：扫描常用根目录，识别含 .git 或技术栈指纹的项目目录，
// 对照本机 mise 已装运行时给出缺失清单，衔接项目配置页的既有编辑/安装流程。
// 扫描有目录预算与结果上限，跳过隐藏目录与依赖/构建产物目录，不做任何写操作。
use serde::Serialize;

/// 已访问目录预算（异常庞大的目录树不至于拖死扫描）。
const DIR_BUDGET: usize = 4000;
/// 结果上限。
const MAX_PROJECTS: usize = 200;
/// 常用根目录下的递归深度。
const ROOT_DEPTH: usize = 3;

/// 指纹文件 → 建议的 mise 工具名（None 表示该指纹只用于识别项目，工具名在文件内容里）。
const FINGERPRINTS: &[(&str, Option<&str>)] = &[
    ("package.json", Some("node")),
    (".nvmrc", Some("node")),
    ("pyproject.toml", Some("python")),
    ("requirements.txt", Some("python")),
    (".python-version", Some("python")),
    ("go.mod", Some("go")),
    ("Gemfile", Some("ruby")),
    ("composer.json", Some("php")),
    ("pom.xml", Some("java")),
    ("build.gradle", Some("java")),
    ("build.gradle.kts", Some("java")),
    (".tool-versions", None),
];

/// 递归时跳过的重目录名（依赖与构建产物，内部不会是独立项目）。
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
    "Pods",
    "DerivedData",
    ".next",
    ".cache",
];

/// 发现的项目（对外结构）。missing_tools = 推断工具中本机未安装的。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredProject {
    pub name: String,
    pub path: String,
    pub has_mise_toml: bool,
    /// 指纹与 mise.toml [tools] 推断出的工具名（去重排序）。
    pub tools: Vec<String>,
    pub missing_tools: Vec<String>,
}

/// 扫描中收集的原始项目信息。
#[derive(Debug, Clone, PartialEq)]
struct RawProject {
    path: std::path::PathBuf,
    has_mise: bool,
    tools: Vec<String>,
}

/// 扫描入口：常用根目录（存在者，深 3 层）+ 家目录本身（1 层），对照已装工具产出缺失清单。
pub fn run() -> Vec<DiscoveredProject> {
    let installed: std::collections::HashSet<String> = crate::mise::list_tools()
        .map(|ts| ts.into_iter().map(|t| t.name).collect())
        .unwrap_or_default();
    scan_with_roots(default_roots(), &installed)
}

/// 以给定根目录执行扫描（run 的可测内核）。
fn scan_with_roots(
    roots: Vec<(std::path::PathBuf, usize)>,
    installed: &std::collections::HashSet<String>,
) -> Vec<DiscoveredProject> {
    let mut found: Vec<RawProject> = Vec::new();
    let mut budget = DIR_BUDGET;
    for (root, depth) in roots {
        scan_dir(&root, depth, &mut budget, &mut found);
        if budget == 0 || found.len() >= MAX_PROJECTS {
            break;
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);
    found
        .into_iter()
        .take(MAX_PROJECTS)
        .map(|raw| to_discovered(raw, installed))
        .collect()
}

/// 常用扫描根：家目录本身 1 层（兜住直接放 ~ 下的项目），存在才纳入。
fn default_roots() -> Vec<(std::path::PathBuf, usize)> {
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
    else {
        return Vec::new();
    };
    let mut roots = vec![(home.clone(), 1)];
    for d in [
        "Documents",
        "Desktop",
        "Projects",
        "Code",
        "code",
        "Dev",
        "dev",
        "work",
        "repos",
    ] {
        let p = home.join(d);
        if p.is_dir() {
            roots.push((p, ROOT_DEPTH));
        }
    }
    roots
}

/// 递归扫描：识别项目（.git 目录 / 指纹文件 / mise.toml），已识别项目不再深入。
fn scan_dir(dir: &std::path::Path, depth: usize, budget: &mut usize, out: &mut Vec<RawProject>) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut sub_dirs = Vec::new();
    let mut project = RawProject {
        path: dir.to_path_buf(),
        has_mise: false,
        tools: Vec::new(),
    };
    let mut is_project = false;
    for e in entries.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            if name == ".git" {
                is_project = true;
            } else if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            } else {
                sub_dirs.push(e.path());
            }
        } else if ft.is_file() {
            if name == "mise.toml" {
                project.has_mise = true;
                is_project = true;
            } else if let Some((_, tool)) = FINGERPRINTS.iter().find(|(f, _)| *f == name) {
                is_project = true;
                if let Some(t) = tool {
                    project.tools.push(t.to_string());
                }
                if name == ".tool-versions" {
                    if let Ok(content) = std::fs::read_to_string(e.path()) {
                        project.tools.extend(tools_from_tool_versions(&content));
                    }
                }
            }
        }
    }
    if is_project {
        out.push(project);
        return;
    }
    if depth > 0 {
        for d in sub_dirs {
            scan_dir(&d, depth - 1, budget, out);
            if *budget == 0 {
                return;
            }
        }
    }
}

/// 汇总单个项目：工具清单 = 指纹推断 ∪ mise.toml [tools] 声明；缺失 = 清单中未安装的。
fn to_discovered(
    raw: RawProject,
    installed: &std::collections::HashSet<String>,
) -> DiscoveredProject {
    let mut tools = raw.tools;
    if raw.has_mise {
        if let Ok(content) = std::fs::read_to_string(raw.path.join("mise.toml")) {
            tools.extend(tools_from_mise_toml(&content));
        }
    }
    tools.sort();
    tools.dedup();
    let missing_tools = tools
        .iter()
        .filter(|t| !installed.contains(*t))
        .cloned()
        .collect();
    DiscoveredProject {
        name: raw
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: raw.path.display().to_string(),
        has_mise_toml: raw.has_mise,
        tools,
        missing_tools,
    }
}

/// 解析 mise.toml 的 [tools] 段声明的工具名（点分键属高级用法，跳过）。
fn tools_from_mise_toml(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = section_bounds(&lines, "[tools]") else {
        return Vec::new();
    };
    (start + 1..end)
        .filter_map(|i| {
            let t = lines[i].trim();
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            let eq = t.find('=')?;
            let mut k = t[..eq].trim();
            // TOML 允许带引号的键："go" = "1.22"
            if let Some(rest) = k.strip_prefix('"') {
                k = rest.strip_suffix('"').unwrap_or(rest);
            }
            if k.is_empty() || k.contains('.') {
                return None;
            }
            Some(k.to_string())
        })
        .collect()
}

/// 解析 .tool-versions（asdf 风格）每行首个词为工具名。
fn tools_from_tool_versions(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            t.split_whitespace().next().map(String::from)
        })
        .collect()
}

/// 顶层段 `[header]` 的行区间 [start, end)（思路同 env_center，面向任意段头）。
fn section_bounds(lines: &[&str], header: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| l.trim() == header)?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.trim_start().starts_with('[') {
            end = i;
            break;
        }
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_from_mise_toml_reads_tools_section_only() {
        let cfg = "# top comment\nmin_version = \"2024\"\n\n[tools]\n# node = \"22\" 已注释\nnode = \"22\"\n\"go\" = \"1.22\"\n_.path = [\"x\"]\n\n[env]\nFOO = \"1\"\n";
        let tools = tools_from_mise_toml(cfg);
        assert_eq!(tools, vec!["node", "go"]);
    }

    #[test]
    fn tools_from_tool_versions_parses_names() {
        let content = "# comment\nnode 22.0.0\ndefault 1.28\n\npython 3.12\n";
        assert_eq!(
            tools_from_tool_versions(content),
            vec!["node", "default", "python"]
        );
    }

    #[test]
    fn scan_finds_projects_and_skips_noise() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // 项目 A：.git + package.json
        std::fs::create_dir_all(root.join("a-proj/.git")).unwrap();
        std::fs::write(root.join("a-proj/package.json"), "{}").unwrap();
        // 应被跳过：node_modules 内的项目、隐藏目录项目
        std::fs::create_dir_all(root.join("node_modules/skip-proj/.git")).unwrap();
        std::fs::create_dir_all(root.join(".hidden-proj/.git")).unwrap();
        // 无标记的普通目录
        std::fs::create_dir_all(root.join("plain")).unwrap();
        // 深层项目：sub/deep/mise.toml（[tools] go）
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::write(root.join("sub/deep/mise.toml"), "[tools]\ngo = \"1.22\"\n").unwrap();

        let mut found = Vec::new();
        let mut budget = DIR_BUDGET;
        scan_dir(root, ROOT_DEPTH, &mut budget, &mut found);
        let paths: Vec<String> = found
            .iter()
            .map(|p| p.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"a-proj".to_string()));
        assert!(paths.contains(&"deep".to_string()));
        assert!(!paths.contains(&"skip-proj".to_string()));
        assert!(!paths.contains(&"hidden-proj".to_string()));
        assert!(!paths.contains(&"plain".to_string()));

        let deep = found.iter().find(|p| p.has_mise).unwrap();
        assert_eq!(deep.path.file_name().unwrap(), "deep");
        // mise.toml 的 [tools] 声明在 to_discovered 阶段解析并参与缺失对比
        let empty = std::collections::HashSet::new();
        let d = to_discovered(deep.clone(), &empty);
        assert!(d.tools.contains(&"go".to_string()));
        assert_eq!(d.missing_tools, vec!["go"]);
        let a = found.iter().find(|p| p.path.ends_with("a-proj")).unwrap();
        assert!(a.tools.contains(&"node".to_string()));
    }

    #[test]
    fn scan_respects_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("a/b/c/proj/.git")).unwrap();
        let mut found = Vec::new();
        let mut budget = 2;
        scan_dir(root, ROOT_DEPTH, &mut budget, &mut found);
        assert!(found.is_empty(), "预算耗尽应停止扫描");
    }

    #[test]
    fn discovered_missing_tools_compare_installed() {
        let raw = RawProject {
            path: std::path::PathBuf::from("/tmp/x"),
            has_mise: false,
            tools: vec!["node".into(), "go".into()],
        };
        let mut installed = std::collections::HashSet::new();
        installed.insert("node".to_string());
        let d = to_discovered(raw, &installed);
        assert_eq!(d.missing_tools, vec!["go"]);
        assert_eq!(d.name, "x");
    }
}
