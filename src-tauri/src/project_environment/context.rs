//! 只执行新鲜、健康来源中已知的 Node；未知 PATH 首项不得跳过后伪造成功。
use super::{
    constraint::{Constraint, Version},
    model::*,
    numeric_request,
};
use crate::{
    domain::*,
    inventory::has_complete_coverage,
    process::probe::{run_probe, ProbeLimits},
    providers::{CancellationToken, NodeScanOptions},
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
const EXPRESSION: &str =
    "JSON.stringify({version:process.versions.node,execPath:process.execPath})";
pub(super) fn empty(context: &str, status: &str, reason: &str) -> ContextResult {
    ContextResult {
        context: context.into(),
        status: status.into(),
        version: None,
        executable_path: None,
        canonical_executable_path: None,
        installation_id: None,
        requirement_match: Match::Unknown,
        selection_match: Match::Unknown,
        reason: reason.into(),
    }
}
#[derive(Eq, PartialEq)]
struct FileStamp {
    canonical: PathBuf,
    length: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, u32),
}
fn stamp(path: &Path) -> Option<FileStamp> {
    let canonical = path.canonicalize().ok()?;
    let m = std::fs::metadata(&canonical).ok()?;
    if !m.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if m.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(FileStamp {
        canonical,
        length: m.len(),
        modified: m.modified().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (m.dev(), m.ino(), m.mode())
        },
    })
}
fn fresh(i: &Installation, snapshot: &InventorySnapshot) -> bool {
    i.tool_id.0 == "node"
        && i.health == Health::Healthy
        && !i.stale
        && i.provider_package.provider_id != ProviderId::Path
        && snapshot.providers.iter().any(|p| {
            p.provider_id == i.provider_package.provider_id
                && has_complete_coverage(p, &ToolId::from("node"))
        })
}
fn known<'a>(path: &Path, snapshot: &'a InventorySnapshot) -> Option<&'a Installation> {
    let canonical = path.canonicalize().ok()?;
    snapshot.installations.iter().find(|i| {
        fresh(i, snapshot)
            && i.canonical_executable_path
                .as_ref()
                .and_then(|p| Path::new(p).canonicalize().ok())
                == Some(canonical.clone())
    })
}
fn path_node(paths: &[PathBuf]) -> Result<Option<PathBuf>, &'static str> {
    for dir in paths {
        if !dir.is_absolute() {
            return Err("PATH 包含相对或空目录，当前无法可靠复现解析顺序");
        }
        let path = dir.join("node");
        match std::fs::metadata(&path) {
            Ok(m) if m.is_file() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if m.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                }
                return Ok(Some(path));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("PATH 首部存在无法检查的位置，实际解析未知"),
        }
    }
    Ok(None)
}
fn command(program: &Path, options: &NodeScanOptions, directory: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .current_dir(directory)
        .env("HOME", &options.home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_AUTO_INSTALL", "0"),
        ("MISE_EXEC_AUTO_INSTALL", "0"),
        ("MISE_NOT_FOUND_AUTO_INSTALL", "0"),
        ("MISE_NODE_COMPILE", "0"),
        ("MISE_COLOR", "0"),
    ] {
        command.env(key, value);
    }
    for (key, tail) in [
        ("MISE_DATA_DIR", ""),
        ("MISE_INSTALLS_DIR", "installs"),
        ("MISE_CACHE_DIR", "cache"),
        ("MISE_STATE_DIR", "state"),
        ("MISE_DOWNLOADS_DIR", "downloads"),
        ("MISE_SHIMS_DIR", "shims"),
        ("MISE_CONFIG_DIR", "zenv-command-config"),
    ] {
        command.env(key, options.mise_data_dir.join(tail));
    }
    command
}
fn run(
    mut command: Command,
    context: &str,
    snapshot: &InventorySnapshot,
    options: &NodeScanOptions,
    token: &CancellationToken,
    program: &Path,
    guard: FileStamp,
) -> ContextResult {
    let output = match run_probe(
        &mut command,
        &ProbeLimits {
            timeout: options.limits.probe_timeout.min(Duration::from_secs(3)),
            max_output_bytes: 64 * 1024,
        },
        token,
    ) {
        Ok(o) if o.status.success() => o,
        _ => return empty(context, "failed", "受限探针失败、超时或取消，不能确认环境"),
    };
    let value: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return empty(context, "failed", "探针输出格式无法验证"),
    };
    let version = value["version"]
        .as_str()
        .filter(|s| Version::parse(s).is_some())
        .map(|s| s.trim_start_matches('v').to_owned());
    let Some(path) = value["execPath"]
        .as_str()
        .filter(|p| Path::new(p).is_absolute() && p.len() <= 4096)
    else {
        return empty(context, "failed", "探针缺少可核对的真实文件路径");
    };
    let canonical = Path::new(path).canonicalize().ok();
    let source = canonical.as_ref().and_then(|p| known(p, snapshot));
    let identity_ok = stamp(program) == Some(guard)
        && source.is_some_and(|s| s.version == version)
        && version.is_some();
    ContextResult {
        context: context.into(),
        status: if identity_ok { "verified" } else { "changed" }.into(),
        version,
        executable_path: Some(path.into()),
        canonical_executable_path: canonical.map(|p| p.to_string_lossy().into()),
        installation_id: if identity_ok {
            source.map(|s| s.installation_id.clone())
        } else {
            None
        },
        requirement_match: Match::Unknown,
        selection_match: Match::Unknown,
        reason: if identity_ok {
            "已核对受控进程版本与已知来源文件；不代表用户终端生效"
        } else {
            "实际文件或版本与来源清单不同，请重新扫描；不能据此确认环境"
        }
        .into(),
    }
}
fn direct(
    paths: &[PathBuf],
    context: &str,
    snapshot: &InventorySnapshot,
    options: &NodeScanOptions,
    directory: &Path,
    token: &CancellationToken,
) -> ContextResult {
    let path = match path_node(paths) {
        Ok(Some(p)) => p,
        Ok(None) => return empty(context, "unavailable", "该 PATH 范围未发现可执行 Node"),
        Err(reason) => return empty(context, "unknown", reason),
    };
    if known(&path, snapshot).is_none() {
        let mut result = empty(
            context,
            "unverified",
            "首个 Node 文件没有新鲜的健康来源证据，未自动执行，也未跳过它选择后续版本",
        );
        result.executable_path = Some(path.to_string_lossy().into());
        return result;
    }
    let Some(before) = stamp(&path) else {
        return empty(context, "changed", "Node 文件在解析时变化");
    };
    let canonical = before.canonical.clone();
    let mut command = command(&canonical, options, directory);
    command.args(["-p", EXPRESSION]);
    run(command, context, snapshot, options, token, &path, before)
}
pub(super) fn application(
    snapshot: &InventorySnapshot,
    options: &NodeScanOptions,
    directory: &Path,
    token: &CancellationToken,
) -> ContextResult {
    if options.platform != Platform::Macos {
        return empty(
            "applicationPath",
            "unsupported",
            "当前受控 Node 上下文仅支持 macOS",
        );
    }
    direct(
        &options.path_entries,
        "applicationPath",
        snapshot,
        options,
        directory,
        token,
    )
}
pub(super) fn controlled(
    selection: &Selection,
    snapshot: &InventorySnapshot,
    options: &NodeScanOptions,
    directory: &Path,
    token: &CancellationToken,
) -> ContextResult {
    if options.platform != Platform::Macos {
        return empty(
            "controlledProject",
            "unsupported",
            "当前受控 Node 上下文仅支持 macOS",
        );
    }
    if selection.kind == "path" {
        let paths: Vec<_> = selection.path_entries.iter().map(PathBuf::from).collect();
        let mut result = direct(
            &paths,
            "controlledProject",
            snapshot,
            options,
            directory,
            token,
        );
        if result.status == "verified" {
            result.selection_match = Match::Satisfied;
        }
        return result;
    }
    if selection.kind != "mise" {
        return empty(
            "controlledProject",
            "unknown",
            selection
                .reason
                .as_deref()
                .unwrap_or("没有可重现的项目选择"),
        );
    }
    let Some(request) = selection.request.as_deref().filter(|s| numeric_request(s)) else {
        return empty(
            "controlledProject",
            "unknown",
            "配置选择含不支持的别名或范围",
        );
    };
    let range = Constraint::parse(request).unwrap();
    if !snapshot.installations.iter().any(|i| {
        fresh(i, snapshot)
            && i.provider_package.provider_id == ProviderId::Mise
            && i.provider_package.package_id == "node"
            && i.version
                .as_deref()
                .and_then(Version::parse)
                .is_some_and(|v| range.matches(v))
    }) {
        return empty(
            "controlledProject",
            "unavailable",
            "mise 尚无满足此配置选择的健康注册；可用的其它来源须先显式绑定，不自动安装",
        );
    }
    let mut dirs = options.path_entries.clone();
    dirs.extend([
        options.home.join(".local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    let Some((program, before)) = dirs
        .into_iter()
        .filter(|p| p.is_absolute())
        .map(|p| p.join("mise"))
        .find_map(|p| stamp(&p).map(|s| (p, s)))
    else {
        return empty(
            "controlledProject",
            "unavailable",
            "未找到可定位的 mise 管理器",
        );
    };
    let mut help = command(&before.canonical, options, directory);
    help.arg("--help");
    let help = run_probe(
        &mut help,
        &ProbeLimits {
            timeout: Duration::from_secs(3),
            max_output_bytes: 64 * 1024,
        },
        token,
    );
    if !help.is_ok_and(|o| {
        o.status.success()
            && ["--no-config", "--no-env", "--no-hooks"]
                .iter()
                .all(|f| String::from_utf8_lossy(&o.stdout).contains(f))
    }) {
        return empty(
            "controlledProject",
            "unsupported",
            "mise 未确认支持禁用配置、环境和钩子的受控调用",
        );
    }
    let mut cmd = command(&before.canonical, options, directory);
    cmd.args([
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        &format!("core:node@{request}"),
        "--",
        "node",
        "-p",
        EXPRESSION,
    ]);
    let mut result = run(
        cmd,
        "controlledProject",
        snapshot,
        options,
        token,
        &program,
        before,
    );
    result.selection_match = result
        .version
        .as_deref()
        .and_then(Version::parse)
        .map(|v| {
            if range.matches(v) {
                Match::Satisfied
            } else {
                Match::Mismatch
            }
        })
        .unwrap_or(Match::Unknown);
    if result.selection_match == Match::Mismatch {
        result.reason = "受控解析返回的版本不符合配置选择，不能确认选择已生效".into();
    }
    result
}
