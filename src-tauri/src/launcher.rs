//! 原生实例启动器：从已核验的清单实例直接启动，保留 argv、退出码与环境。
//! 不经过 shell、不依赖 PATH shim 或符号链接；Windows 与 Unix 共用受控探针的进程守卫。
pub use crate::process::probe::CancellationToken;
use crate::{
    domain::{Health, InventorySnapshot, ProviderId},
    process::probe::{run_probe, ProbeError, ProbeLimits},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const MAX_ARGV: usize = 256;
const MAX_ARG_BYTES: usize = 8 * 1024;
const MAX_ARGV_TOTAL: usize = 64 * 1024;
const MAX_ENV: usize = 64;
const MAX_ENV_KEY: usize = 128;
const MAX_ENV_VALUE: usize = 8 * 1024;
const MAX_PATH_ENTRIES: usize = 32;
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchEnvironment {
    pub home: String,
    pub path: Vec<String>,
    pub vars: Vec<(String, String)>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchRequest {
    pub installation_id: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub environment: LaunchEnvironment,
    pub timeout_ms: Option<u64>,
    pub max_output_bytes: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub installation_id: String,
    pub executable_path: String,
    pub canonical_executable_path: String,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("没有找到可启动的健康实例")]
    NotFound,
    #[error("实例证据不完整或已变化，拒绝启动")]
    Evidence,
    #[error("参数数量、长度或内容超出预算")]
    InvalidArgv,
    #[error("启动环境无效或超出预算")]
    InvalidEnvironment,
    #[error("启动失败：{0}")]
    Io(String),
    #[error("启动超时，已回收进程树")]
    Timeout,
    #[error("启动已取消，已回收进程树")]
    Cancelled,
    #[error("输出超出预算，已回收进程树")]
    OutputLimit,
}

fn invalid_environment() -> LaunchError {
    LaunchError::InvalidEnvironment
}

fn validate_argv(argv: &[String]) -> Result<(), LaunchError> {
    if argv.len() > MAX_ARGV {
        return Err(LaunchError::InvalidArgv);
    }
    let mut total = 0;
    for argument in argv {
        if argument.len() > MAX_ARG_BYTES || argument.contains('\0') {
            return Err(LaunchError::InvalidArgv);
        }
        total += argument.len();
    }
    if total > MAX_ARGV_TOTAL {
        return Err(LaunchError::InvalidArgv);
    }
    Ok(())
}

fn validate_environment(environment: &LaunchEnvironment) -> Result<PathBuf, LaunchError> {
    let home = PathBuf::from(&environment.home);
    if !home.is_absolute() || environment.home.contains('\0') {
        return Err(invalid_environment());
    }
    if environment.path.len() > MAX_PATH_ENTRIES {
        return Err(invalid_environment());
    }
    for entry in &environment.path {
        if entry.is_empty() || entry.contains('\0') || !Path::new(entry).is_absolute() {
            return Err(invalid_environment());
        }
    }
    if environment.vars.len() > MAX_ENV {
        return Err(invalid_environment());
    }
    for (key, value) in &environment.vars {
        if key.is_empty()
            || key.len() > MAX_ENV_KEY
            || value.len() > MAX_ENV_VALUE
            || key.contains('=')
            || value.contains('\0')
            || !key.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            })
        {
            return Err(invalid_environment());
        }
        if matches!(
            key.as_str(),
            "LD_PRELOAD"
                | "LD_LIBRARY_PATH"
                | "DYLD_INSERT_LIBRARIES"
                | "DYLD_LIBRARY_PATH"
                | "DYLD_FRAMEWORK_PATH"
        ) {
            return Err(invalid_environment());
        }
    }
    Ok(home)
}

fn limits(request: &LaunchRequest) -> Result<ProbeLimits, LaunchError> {
    let timeout = request.timeout_ms.unwrap_or(30_000);
    let max_output_bytes = request.max_output_bytes.unwrap_or(256 * 1024);
    if !(1..=300_000).contains(&timeout) || !(1..=MAX_OUTPUT).contains(&max_output_bytes) {
        return Err(LaunchError::InvalidEnvironment);
    }
    Ok(ProbeLimits {
        timeout: Duration::from_millis(timeout),
        max_output_bytes,
    })
}

fn executable(
    snapshot: &InventorySnapshot,
    installation_id: &str,
) -> Result<(PathBuf, PathBuf), LaunchError> {
    let item = snapshot
        .installations
        .iter()
        .find(|item| {
            item.installation_id.0 == installation_id
                && item.provider_package.provider_id != ProviderId::Path
                && !item.stale
                && item.health == Health::Healthy
        })
        .ok_or(LaunchError::NotFound)?;
    let path = item
        .executable_path
        .as_deref()
        .ok_or(LaunchError::Evidence)?;
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(LaunchError::Evidence);
    }
    let canonical = path.canonicalize().map_err(|_| LaunchError::Evidence)?;
    if !canonical.is_file() {
        return Err(LaunchError::Evidence);
    }
    if let Some(expected) = item.canonical_executable_path.as_deref() {
        let expected = Path::new(expected)
            .canonicalize()
            .map_err(|_| LaunchError::Evidence)?;
        if expected != canonical {
            return Err(LaunchError::Evidence);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if canonical
            .metadata()
            .map_err(|_| LaunchError::Evidence)?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(LaunchError::Evidence);
        }
    }
    Ok((path, canonical))
}

fn command(
    executable: &Path,
    request: &LaunchRequest,
    home: &Path,
) -> Result<Command, LaunchError> {
    let cwd = Path::new(&request.cwd);
    if !cwd.is_absolute() {
        return Err(LaunchError::InvalidEnvironment);
    }
    let cwd = cwd
        .canonicalize()
        .map_err(|_| LaunchError::InvalidEnvironment)?;
    if !cwd.is_dir() {
        return Err(LaunchError::InvalidEnvironment);
    }
    let mut command = Command::new(executable);
    command
        .args(&request.argv)
        .current_dir(&cwd)
        .env_clear()
        .env("HOME", home);
    #[cfg(windows)]
    command.env("USERPROFILE", home);
    let mut entries: Vec<PathBuf> = request.environment.path.iter().map(PathBuf::from).collect();
    if let Some(parent) = executable.parent() {
        if !entries.iter().any(|entry| entry == parent) {
            entries.insert(0, parent.to_path_buf());
        }
    }
    if entries.is_empty() {
        #[cfg(unix)]
        entries.extend(["/usr/bin", "/bin"].map(PathBuf::from));
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            entries.push(PathBuf::from(root).join("System32"));
        }
    }
    let path = std::env::join_paths(entries).map_err(|_| LaunchError::InvalidEnvironment)?;
    command.env("PATH", path);
    for (key, value) in &request.environment.vars {
        command.env(key, value);
    }
    Ok(command)
}

fn code(status: &std::process::ExitStatus) -> Option<i32> {
    status.code()
}

#[cfg(unix)]
fn signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

pub fn launch(
    snapshot: &InventorySnapshot,
    request: &LaunchRequest,
    cancellation: &CancellationToken,
) -> Result<LaunchResult, LaunchError> {
    validate_argv(&request.argv)?;
    let home = validate_environment(&request.environment)?;
    let limits = limits(request)?;
    let (path, canonical) = executable(snapshot, &request.installation_id)?;
    let mut command = command(&path, request, &home)?;
    let started = Instant::now();
    let output = run_probe(&mut command, &limits, cancellation).map_err(|error| match error {
        ProbeError::Timeout => LaunchError::Timeout,
        ProbeError::Cancelled => LaunchError::Cancelled,
        ProbeError::OutputLimit => LaunchError::OutputLimit,
        ProbeError::Io(error) => LaunchError::Io(error.to_string()),
    })?;
    Ok(LaunchResult {
        installation_id: request.installation_id.clone(),
        executable_path: path.to_string_lossy().into(),
        canonical_executable_path: canonical.to_string_lossy().into(),
        code: code(&output.status),
        signal: signal(&output.status),
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::{
        Installation, InstallationId, Ownership, PackageKind, Platform, ProviderPackage, ToolId,
    };
    use std::{fs, os::unix::fs::PermissionsExt};

    fn installation(executable: &Path) -> Installation {
        Installation {
            installation_id: InstallationId::from("instance"),
            provider_package: ProviderPackage {
                provider_id: ProviderId::Mise,
                package_id: "node".into(),
                kind: PackageKind::Runtime,
                platform: Platform::Macos,
            },
            tool_id: ToolId::from("node"),
            locator: executable.parent().unwrap().to_string_lossy().into(),
            executable_path: Some(executable.to_string_lossy().into()),
            canonical_executable_path: Some(
                executable.canonicalize().unwrap().to_string_lossy().into(),
            ),
            version: Some("22.1.0".into()),
            architecture: Some("arm64".into()),
            health: Health::Healthy,
            ownership: Ownership::Observed,
            evidence: vec![],
            stale: false,
        }
    }

    fn snapshot_for(executable: &Path) -> InventorySnapshot {
        InventorySnapshot {
            schema_version: 1,
            request_id: "snapshot".into(),
            observed_at: 0,
            tools: vec![],
            providers: vec![],
            installations: vec![installation(executable)],
            aliases: vec![],
        }
    }

    fn script(directory: &Path, text: &str) -> PathBuf {
        let path = directory.join("tool");
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn environment(home: &Path) -> LaunchEnvironment {
        LaunchEnvironment {
            home: home.to_string_lossy().into(),
            path: vec![],
            vars: vec![("TRAIT".into(), "ok".into())],
        }
    }

    fn launch_request(directory: &Path) -> LaunchRequest {
        LaunchRequest {
            installation_id: "instance".into(),
            argv: vec!["arg value".into()],
            cwd: directory.to_string_lossy().into(),
            environment: environment(directory),
            timeout_ms: Some(5_000),
            max_output_bytes: Some(64 * 1024),
        }
    }

    #[test]
    fn launches_real_executable_with_argv_cwd_environment_and_exit_code() {
        let directory = tempfile::tempdir().unwrap();
        let path = script(
            directory.path(),
            "#!/bin/sh\nprintf 'cwd=%s\\n' \"$PWD\"\nprintf 'arg=%s\\n' \"$1\"\nprintf 'home=%s\\n' \"$HOME\"\nprintf 'trait=%s\\n' \"$TRAIT\"\nprintf 'path=%s\\n' \"$PATH\"\nexit 7\n",
        );
        let snapshot = snapshot_for(&path);
        let result = launch(
            &snapshot,
            &launch_request(directory.path()),
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(result.code, Some(7));
        assert!(!result.success);
        assert!(result.stdout.contains("arg=arg value"));
        assert!(result.stdout.contains(&format!(
            "cwd={}",
            directory.path().canonicalize().unwrap().display()
        )));
        assert!(result
            .stdout
            .contains(&format!("home={}", directory.path().display())));
        assert!(result.stdout.contains("trait=ok"));
        assert!(result
            .stdout
            .contains(&format!("path={}", directory.path().display())));
        assert_eq!(result.installation_id, "instance");
        assert!(result.canonical_executable_path.ends_with("/tool"));
    }

    #[test]
    fn rejects_unknown_relative_cwd_and_invalid_argv_or_environment() {
        let directory = tempfile::tempdir().unwrap();
        let path = script(directory.path(), "#!/bin/sh\nexit 0\n");
        let snapshot = snapshot_for(&path);
        let mut request = launch_request(directory.path());
        request.installation_id = "missing".into();
        assert!(matches!(
            launch(&snapshot, &request, &CancellationToken::default()),
            Err(LaunchError::NotFound)
        ));
        let mut request = launch_request(directory.path());
        request.cwd = "relative".into();
        assert!(matches!(
            launch(&snapshot, &request, &CancellationToken::default()),
            Err(LaunchError::InvalidEnvironment)
        ));
        let mut request = launch_request(directory.path());
        request.argv = vec!["bad\0arg".into()];
        assert!(matches!(
            launch(&snapshot, &request, &CancellationToken::default()),
            Err(LaunchError::InvalidArgv)
        ));
        let mut request = launch_request(directory.path());
        request.environment.vars = vec![("LD_PRELOAD".into(), "/tmp/x".into())];
        assert!(matches!(
            launch(&snapshot, &request, &CancellationToken::default()),
            Err(LaunchError::InvalidEnvironment)
        ));
    }

    #[test]
    fn timeout_and_output_limit_reap_the_process_tree() {
        let directory = tempfile::tempdir().unwrap();
        let path = script(directory.path(), "#!/bin/sh\n/bin/sleep 30\n");
        let snapshot = snapshot_for(&path);
        let mut request = launch_request(directory.path());
        request.timeout_ms = Some(100);
        assert!(matches!(
            launch(&snapshot, &request, &CancellationToken::default()),
            Err(LaunchError::Timeout)
        ));
        let path = script(
            directory.path(),
            "#!/bin/sh\nwhile :; do printf 'abcdefgh'; done\n",
        );
        let snapshot = snapshot_for(&path);
        let mut request = launch_request(directory.path());
        request.max_output_bytes = Some(16);
        assert!(matches!(
            launch(&snapshot, &request, &CancellationToken::default()),
            Err(LaunchError::OutputLimit)
        ));
    }

    #[test]
    fn non_executable_and_tampered_target_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tool");
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let snapshot = snapshot_for(&path);
        assert!(matches!(
            launch(
                &snapshot,
                &launch_request(directory.path()),
                &CancellationToken::default()
            ),
            Err(LaunchError::Evidence)
        ));
        let path = script(directory.path(), "#!/bin/sh\nexit 0\n");
        let mut snapshot = snapshot_for(&path);
        snapshot.installations[0].canonical_executable_path = Some("/definitely/elsewhere".into());
        assert!(matches!(
            launch(
                &snapshot,
                &launch_request(directory.path()),
                &CancellationToken::default()
            ),
            Err(LaunchError::Evidence)
        ));
    }
}
