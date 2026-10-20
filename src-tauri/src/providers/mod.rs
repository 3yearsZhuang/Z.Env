//! macOS Node 的只读来源发现；每个来源独立返回覆盖、失败和实例证据。

mod directories;
mod managed;
pub mod python;

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::domain::{
    Health, InstallationObservation, ObservationEvidence, Ownership, Platform, ProviderError,
    ProviderId, ProviderPackage, ProviderReport, ProviderScan, ProviderStatus, ScanCoverage,
    ToolId,
};
pub use crate::process::probe::CancellationToken;
use crate::process::probe::{run_probe, ProbeError, ProbeLimits};

/// 来源独立限制目录、候选、元数据和探针次数；超时及输出另设单探针上限。
/// 四个来源最多同时运行四个探针，文件系统调用之间协作检查来源时间预算。
#[derive(Clone, Debug)]
pub struct ScanLimits {
    pub provider_timeout: Duration,
    pub probe_timeout: Duration,
    pub max_directory_entries: usize,
    pub max_candidates: usize,
    pub max_probes: usize,
    pub max_output_bytes: usize,
    pub max_metadata_bytes: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            provider_timeout: Duration::from_secs(8),
            probe_timeout: Duration::from_secs(3),
            max_directory_entries: 2048,
            max_candidates: 128,
            max_probes: 32,
            max_output_bytes: 32 * 1024,
            max_metadata_bytes: 128 * 1024,
        }
    }
}

/// 扫描环境在入口捕获；测试可传临时目录，执行期间不修改进程全局环境。
#[derive(Clone, Debug)]
pub struct NodeScanOptions {
    pub platform: Platform,
    pub home: PathBuf,
    pub mise_data_dir: PathBuf,
    pub nvm_dir: PathBuf,
    pub brew_cellars: Vec<PathBuf>,
    pub path_entries: Vec<PathBuf>,
    pub limits: ScanLimits,
}

impl NodeScanOptions {
    pub fn from_environment() -> io::Result<Self> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "无法定位绝对主目录"))?;
        let mise_data_dir = std::env::var_os("MISE_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".local/share"))
                    .join("mise")
            });
        let nvm_dir = std::env::var_os("NVM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".nvm"));
        let mut brew_cellars: Vec<_> = std::env::var_os("HOMEBREW_CELLAR")
            .map(PathBuf::from)
            .into_iter()
            .collect();
        for path in ["/opt/homebrew/Cellar", "/usr/local/Cellar"] {
            if !brew_cellars.contains(&PathBuf::from(path)) {
                brew_cellars.push(PathBuf::from(path));
            }
        }
        let path_entries = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        let platform = if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        };
        Ok(Self {
            platform,
            home,
            mise_data_dir,
            nvm_dir,
            brew_cellars,
            path_entries,
            limits: ScanLimits::default(),
        })
    }
}

pub(super) struct Candidate {
    pub provider_package: ProviderPackage,
    pub locator: PathBuf,
    pub executable: PathBuf,
    pub declared_version: Option<String>,
    pub ownership: Ownership,
    pub evidence: Vec<ObservationEvidence>,
    pub trusted: bool,
}

pub(super) struct ScanRun<'a> {
    pub options: &'a NodeScanOptions,
    cancellation: &'a CancellationToken,
    started: Instant,
    provider: ProviderId,
    tool: ToolId,
    available: bool,
    entries: usize,
    probes: usize,
    metadata_bytes: usize,
    observations: Vec<InstallationObservation>,
    failure: Option<(ProviderStatus, ProviderError)>,
    truncated: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(9_007_199_254_740_991) as u64
}

fn priority(status: ProviderStatus) -> u8 {
    match status {
        ProviderStatus::Cancelled => 7,
        ProviderStatus::Timeout => 6,
        ProviderStatus::PermissionDenied => 5,
        ProviderStatus::Error => 4,
        ProviderStatus::Unavailable => 3,
        ProviderStatus::Unsupported => 2,
        ProviderStatus::Ok => 0,
    }
}

impl<'a> ScanRun<'a> {
    fn new(
        provider: ProviderId,
        options: &'a NodeScanOptions,
        cancellation: &'a CancellationToken,
    ) -> Self {
        Self {
            options,
            cancellation,
            provider,
            tool: ToolId::from("node"),
            started: Instant::now(),
            available: false,
            entries: 0,
            probes: 0,
            metadata_bytes: 0,
            observations: Vec::new(),
            failure: None,
            truncated: false,
        }
    }

    pub(super) fn problem(&mut self, status: ProviderStatus, code: &str, message: &str) {
        if self
            .failure
            .as_ref()
            .is_none_or(|(previous, _)| priority(status) > priority(*previous))
        {
            self.failure = Some((
                status,
                ProviderError {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    retryable: !matches!(status, ProviderStatus::Unsupported),
                },
            ));
        }
    }

    fn io_problem(&mut self, code: &str, error: &io::Error) {
        let status = if error.kind() == io::ErrorKind::PermissionDenied {
            ProviderStatus::PermissionDenied
        } else {
            ProviderStatus::Error
        };
        self.problem(status, code, &format!("读取本机工具证据失败：{error}"));
    }

    pub(super) fn check(&mut self) -> bool {
        if self.cancellation.is_cancelled() {
            self.problem(ProviderStatus::Cancelled, "scanCancelled", "扫描已取消");
            return false;
        }
        if self.started.elapsed() >= self.options.limits.provider_timeout {
            self.problem(
                ProviderStatus::Timeout,
                "scanTimeout",
                "来源扫描超出时间预算",
            );
            return false;
        }
        true
    }

    pub(super) fn mark_available(&mut self) {
        self.available = true;
    }

    pub(super) fn visit_entry(&mut self) -> bool {
        if !self.check() {
            return false;
        }
        if self.entries >= self.options.limits.max_directory_entries {
            self.truncated = true;
            return false;
        }
        self.entries += 1;
        true
    }

    pub(super) fn read_dir(&mut self, path: &Path) -> Option<Vec<PathBuf>> {
        if !self.check() {
            return None;
        }
        if !path.is_absolute() {
            self.problem(
                ProviderStatus::Error,
                "relativeProviderRoot",
                "来源目录必须是绝对路径",
            );
            return None;
        }
        let directory = match fs::read_dir(path) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                self.io_problem("directoryRead", &error);
                return None;
            }
        };
        let mut paths = Vec::new();
        for entry in directory {
            if !self.visit_entry() {
                break;
            }
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => self.io_problem("directoryEntry", &error),
            }
        }
        paths.sort();
        Some(paths)
    }

    pub(super) fn read_metadata(&mut self, path: &Path) -> Option<Vec<u8>> {
        if !self.check() {
            return None;
        }
        // 只读普通文件，避免收据路径被 FIFO、设备或符号链接替代后阻塞读取。
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                self.problem(
                    ProviderStatus::Error,
                    "invalidMetadataFile",
                    "元数据不是普通文件",
                );
                return None;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                self.io_problem("metadataFile", &error);
                return None;
            }
        }
        let remaining = self
            .options
            .limits
            .max_metadata_bytes
            .saturating_sub(self.metadata_bytes);
        if remaining == 0 {
            self.truncated = true;
            self.problem(
                ProviderStatus::Error,
                "metadataBudget",
                "来源元数据已耗尽字节预算",
            );
            return None;
        }
        let mut open = fs::OpenOptions::new();
        open.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = match open.open(path) {
            Ok(file) => file,
            Err(error) => {
                self.io_problem("metadataOpen", &error);
                return None;
            }
        };
        match file.metadata() {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                self.problem(
                    ProviderStatus::Error,
                    "metadataChanged",
                    "元数据读取前已变为非普通文件",
                );
                return None;
            }
            Err(error) => {
                self.io_problem("metadataChanged", &error);
                return None;
            }
        }
        let mut data = Vec::new();
        let read_result = Read::by_ref(&mut file)
            .take(remaining.saturating_add(1) as u64)
            .read_to_end(&mut data);
        self.metadata_bytes = self
            .metadata_bytes
            .saturating_add(data.len())
            .min(self.options.limits.max_metadata_bytes);
        match read_result {
            Ok(_) if data.len() <= remaining => Some(data),
            Ok(_) => {
                self.truncated = true;
                self.problem(
                    ProviderStatus::Error,
                    "metadataBudget",
                    "来源元数据超出字节预算",
                );
                None
            }
            Err(error) => {
                self.io_problem("metadataRead", &error);
                None
            }
        }
    }

    pub(super) fn observe(&mut self, mut candidate: Candidate) {
        if !self.check() {
            return;
        }
        if self.observations.len() >= self.options.limits.max_candidates {
            self.truncated = true;
            return;
        }
        let (Some(locator), Some(executable)) =
            (candidate.locator.to_str(), candidate.executable.to_str())
        else {
            self.problem(
                ProviderStatus::Error,
                "nonUnicodePath",
                "来源路径无法无损传输为 UTF-8",
            );
            return;
        };
        let mut observation = InstallationObservation {
            provider_package: candidate.provider_package,
            tool_id: self.tool.clone(),
            locator: locator.to_owned(),
            executable_path: Some(executable.to_owned()),
            canonical_executable_path: None,
            version: None,
            architecture: None,
            health: Health::Unverified,
            ownership: candidate.ownership,
            evidence: Vec::new(),
        };
        if let Some(declared) = candidate.declared_version.take() {
            candidate.evidence.push(ObservationEvidence {
                kind: "declaredVersion".into(),
                path: Some(locator.to_owned()),
                detail: Some(declared),
            });
        }
        observation.evidence = candidate.evidence;
        let metadata = match fs::metadata(&candidate.executable) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                observation.health = Health::Broken;
                self.add_evidence(
                    &mut observation,
                    "invalidExecutable",
                    "可执行路径不是普通文件",
                );
                self.observations.push(observation);
                return;
            }
            Err(error) => {
                observation.health = Health::Broken;
                if error.kind() != io::ErrorKind::NotFound {
                    self.io_problem("executableMetadata", &error);
                }
                self.add_evidence(
                    &mut observation,
                    "executableUnavailable",
                    "可执行文件缺失或不可读取",
                );
                self.observations.push(observation);
                return;
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                observation.health = Health::Broken;
                self.problem(
                    ProviderStatus::PermissionDenied,
                    "executablePermission",
                    "文件没有执行权限",
                );
                self.add_evidence(&mut observation, "notExecutable", "文件没有执行权限");
                self.observations.push(observation);
                return;
            }
        }
        #[cfg(not(unix))]
        let _ = metadata;
        match fs::canonicalize(&candidate.executable) {
            Ok(path) => match path.to_str() {
                Some(path) => observation.canonical_executable_path = Some(path.to_owned()),
                None => self.problem(
                    ProviderStatus::Error,
                    "nonUnicodeTarget",
                    "真实可执行路径不是 UTF-8",
                ),
            },
            Err(error) => {
                self.io_problem("canonicalExecutable", &error);
                observation.health = Health::Broken;
            }
        }
        if candidate.trusted && observation.canonical_executable_path.is_some() {
            if self.probes >= self.options.limits.max_probes {
                self.truncated = true;
            } else if self.check() {
                self.probes += 1;
                self.probe(&mut observation);
            }
        } else if !candidate.trusted {
            self.add_evidence(
                &mut observation,
                "probeNotRequested",
                "仅展示文件证据，未自动执行此来源程序",
            );
        }
        self.observations.push(observation);
    }

    fn add_evidence(&self, observation: &mut InstallationObservation, kind: &str, detail: &str) {
        observation.evidence.push(ObservationEvidence {
            kind: kind.to_owned(),
            path: observation.executable_path.clone(),
            detail: Some(detail.to_owned()),
        });
    }

    fn probe(&mut self, observation: &mut InstallationObservation) {
        if self.tool.0 == "python" {
            python::probe(self, observation);
            return;
        }
        let executable = observation.canonical_executable_path.as_ref().unwrap();
        let mut command = Command::new(executable);
        command
            .args(["--eval", "process.stdout.write(JSON.stringify({version:process.versions.node,architecture:process.arch}))"])
            .env_clear()
            .env("HOME", &self.options.home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .current_dir(&self.options.home);
        let limits = ProbeLimits {
            timeout: self.options.limits.probe_timeout.min(
                self.options
                    .limits
                    .provider_timeout
                    .saturating_sub(self.started.elapsed()),
            ),
            max_output_bytes: self.options.limits.max_output_bytes,
        };
        match run_probe(&mut command, &limits, self.cancellation) {
            Ok(output) if output.status.success() => match parse_probe(&output.stdout) {
                Some((version, architecture)) => {
                    observation.version = Some(version);
                    observation.architecture = Some(architecture);
                    observation.health = Health::Healthy;
                    self.add_evidence(
                        observation,
                        "nodeProbe",
                        &format!(
                            "受控 Node 探针验证成功；stdout {} 字节，stderr {} 字节",
                            output.stdout.len(),
                            output.stderr.len()
                        ),
                    );
                }
                None => {
                    observation.health = Health::Broken;
                    self.problem(
                        ProviderStatus::Error,
                        "invalidProbeOutput",
                        "Node 探针输出格式或版本无效",
                    );
                    self.add_evidence(
                        observation,
                        "invalidProbeOutput",
                        "无法确认 Node 版本与架构",
                    );
                }
            },
            Ok(output) => {
                observation.health = Health::Broken;
                self.problem(ProviderStatus::Error, "probeExit", "Node 探针退出失败");
                self.add_evidence(
                    observation,
                    "probeExit",
                    &format!(
                        "Node 探针退出失败；退出码 {:?}，stderr {} 字节",
                        output.status.code(),
                        output.stderr.len()
                    ),
                );
            }
            Err(error) => {
                let (status, code, detail) = match &error {
                    ProbeError::Timeout => {
                        (ProviderStatus::Timeout, "probeTimeout", "Node 探针超时")
                    }
                    ProbeError::Cancelled => (
                        ProviderStatus::Cancelled,
                        "probeCancelled",
                        "Node 探针已取消",
                    ),
                    ProbeError::OutputLimit => {
                        self.truncated = true;
                        (
                            ProviderStatus::Error,
                            "probeOutputLimit",
                            "Node 探针输出超出预算",
                        )
                    }
                    ProbeError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => (
                        ProviderStatus::PermissionDenied,
                        "probePermission",
                        "Node 探针权限不足",
                    ),
                    ProbeError::Io(_) => (ProviderStatus::Error, "probeIo", "Node 探针无法执行"),
                };
                // 取消和超时无法证明安装损坏，继续保留待验证实例。
                if !matches!(error, ProbeError::Timeout | ProbeError::Cancelled) {
                    observation.health = Health::Broken;
                }
                self.problem(status, code, detail);
                self.add_evidence(observation, code, detail);
            }
        }
    }

    fn finish(mut self) -> ProviderScan {
        self.check();
        if !self.available && self.failure.is_none() {
            self.problem(
                ProviderStatus::Unavailable,
                "sourceUnavailable",
                "未发现该来源的安装目录",
            );
        }
        let (status, error) = match self.failure {
            Some((status, error)) => (status, Some(error)),
            None => (ProviderStatus::Ok, None),
        };
        self.observations.sort();
        self.observations.dedup();
        ProviderScan {
            report: ProviderReport {
                provider_id: self.provider,
                status,
                observed_at: now_ms(),
                stale: false,
                coverage: ScanCoverage {
                    tool_ids: vec![self.tool.clone()],
                    complete: status == ProviderStatus::Ok && !self.truncated,
                    truncated: self.truncated,
                },
                error,
            },
            observations: self.observations,
        }
    }
}

fn parse_probe(output: &[u8]) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_slice(output).ok()?;
    let version = value
        .get("version")?
        .as_str()?
        .strip_prefix('v')
        .unwrap_or(value.get("version")?.as_str()?);
    let base = version.split(['-', '+']).next()?;
    let parts: Vec<_> = base.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return None;
    }
    let architecture = match value.get("architecture")?.as_str()? {
        "x64" | "x86_64" => "x86_64",
        "ia32" | "x86" => "x86",
        "arm64" => "arm64",
        "arm" => "arm",
        _ => return None,
    };
    Some((version.to_owned(), architecture.to_owned()))
}

/// 单来源可独立重试；非 macOS 明确未支持，不推测平台能力。
pub fn scan_node_provider(
    provider: ProviderId,
    options: &NodeScanOptions,
    cancellation: &CancellationToken,
) -> ProviderScan {
    let mut run = ScanRun::new(provider, options, cancellation);
    if !run.check() {
        return run.finish();
    }
    if options.platform != Platform::Macos || !cfg!(unix) {
        run.problem(
            ProviderStatus::Unsupported,
            "platformUnsupported",
            "当前只提供 macOS Node 来源扫描",
        );
    } else if !options.home.is_absolute() {
        run.problem(
            ProviderStatus::Error,
            "relativeHome",
            "探针主目录必须是绝对路径",
        );
    } else if provider == ProviderId::Mise && !options.mise_data_dir.is_absolute() {
        run.problem(
            ProviderStatus::Error,
            "relativeProviderRoot",
            "mise 数据目录必须是绝对路径",
        );
    } else {
        match provider {
            ProviderId::Mise => managed::collect_mise(&mut run),
            ProviderId::Brew => managed::collect_brew(&mut run),
            ProviderId::Nvm => directories::collect_nvm(&mut run),
            ProviderId::Path => directories::collect_path(&mut run),
            _ => run.problem(
                ProviderStatus::Unsupported,
                "providerUnsupported",
                "此来源不提供 Node 扫描",
            ),
        }
    }
    run.finish()
}

/// 四个独立任务有界并发；一个来源失败不会阻止其它来源发布观测。
pub fn scan_node_sources(
    options: &NodeScanOptions,
    cancellation: &CancellationToken,
) -> Vec<ProviderScan> {
    std::thread::scope(|scope| {
        let workers: Vec<_> = [
            ProviderId::Mise,
            ProviderId::Brew,
            ProviderId::Nvm,
            ProviderId::Path,
        ]
        .into_iter()
        .map(|provider| {
            (
                provider,
                scope.spawn(move || scan_node_provider(provider, options, cancellation)),
            )
        })
        .collect();
        workers
            .into_iter()
            .map(|(provider, worker)| {
                worker.join().unwrap_or_else(|_| {
                    let mut run = ScanRun::new(provider, options, cancellation);
                    run.problem(ProviderStatus::Error, "sourcePanicked", "来源扫描异常中断");
                    run.finish()
                })
            })
            .collect()
    })
}

#[cfg(test)]
mod tests;
