//! 不修改环境、不写任务日志的有界只读探针执行器。

use std::io;
#[cfg(any(unix, windows))]
use std::io::Read;
#[cfg(any(unix, windows))]
use std::process::{Child, Stdio};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[path = "probe_windows.rs"]
mod windows;
#[cfg(windows)]
use windows::{prepare_pipe, ProbePipe, RunningChild};

#[cfg(all(test, windows))]
#[path = "probe_windows_tests.rs"]
mod windows_tests;

#[derive(Clone, Default, Debug)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProbeLimits {
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

#[derive(Debug)]
pub struct ProbeOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("probe I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("probe timed out")]
    Timeout,
    #[error("probe cancelled")]
    Cancelled,
    #[error("probe exceeded the combined output limit")]
    OutputLimit,
}

/// 命令的环境和可执行文件路径完全由调用方设置；输出总量包含标准输出与错误输出。
pub fn run_probe(
    command: &mut Command,
    limits: &ProbeLimits,
    cancellation: &CancellationToken,
) -> Result<ProbeOutput, ProbeError> {
    let started = Instant::now();
    check_budget(started, limits, cancellation)?;
    #[cfg(any(unix, windows))]
    {
        run_platform_probe(command, limits, cancellation, started)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "bounded probes require Unix or Windows",
        )
        .into())
    }
}

#[cfg(any(unix, windows))]
fn run_platform_probe(
    command: &mut Command,
    limits: &ProbeLimits,
    cancellation: &CancellationToken,
    started: Instant,
) -> Result<ProbeOutput, ProbeError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    // 守卫涵盖管道初始化、读取、轮询及提前返回，任何路径都回收进程树。
    let mut running = RunningChild::spawn(command)?;
    let mut stdout = running.child.stdout.take().ok_or_else(missing_pipe)?;
    let mut stderr = running.child.stderr.take().ok_or_else(missing_pipe)?;
    prepare_pipe(&stdout)?;
    prepare_pipe(&stderr)?;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut stdout_closed = false;
    let mut stderr_closed = false;
    let mut status = None;
    let mut buffer = [0_u8; 8192];
    let mut output_bytes = 0;
    loop {
        check_budget(started, limits, cancellation)?;
        let mut progressed = false;
        // 每轮各读取一次，持续输出的一条流不能阻塞取消、超时或另一条流。
        for (stream, destination, closed) in [
            (
                &mut stdout as &mut dyn ProbePipe,
                &mut out,
                &mut stdout_closed,
            ),
            (
                &mut stderr as &mut dyn ProbePipe,
                &mut err,
                &mut stderr_closed,
            ),
        ] {
            if *closed {
                continue;
            }
            match stream.read_ready(&mut buffer)? {
                ReadState::Bytes(count) => {
                    if count > limits.max_output_bytes.saturating_sub(output_bytes) {
                        return Err(ProbeError::OutputLimit);
                    }
                    output_bytes += count;
                    destination.extend_from_slice(&buffer[..count]);
                    progressed = true;
                }
                ReadState::Closed => *closed = true,
                ReadState::Pending => {}
            }
        }
        if status.is_none() {
            status = running.child.try_wait()?;
        }
        if stdout_closed && stderr_closed {
            if let Some(status) = status {
                check_budget(started, limits, cancellation)?;
                return Ok(ProbeOutput {
                    status,
                    stdout: out,
                    stderr: err,
                });
            }
        }
        if !progressed {
            // 短周期轮询避免为每个探针生成可能永久阻塞的管道读取线程。
            std::thread::sleep(
                Duration::from_millis(5).min(limits.timeout.saturating_sub(started.elapsed())),
            );
        }
    }
}

#[cfg(any(unix, windows))]
fn missing_pipe() -> io::Error {
    io::Error::other("probe output pipe was not created")
}

fn check_budget(
    started: Instant,
    limits: &ProbeLimits,
    cancellation: &CancellationToken,
) -> Result<(), ProbeError> {
    if cancellation.is_cancelled() {
        return Err(ProbeError::Cancelled);
    }
    if started.elapsed() >= limits.timeout {
        return Err(ProbeError::Timeout);
    }
    Ok(())
}

#[cfg(unix)]
struct RunningChild {
    child: Child,
}

#[cfg(unix)]
impl RunningChild {
    fn spawn(command: &mut Command) -> io::Result<Self> {
        Ok(Self {
            child: command.spawn()?,
        })
    }
}

#[cfg(unix)]
impl Drop for RunningChild {
    fn drop(&mut self) {
        unsafe {
            // 即使直接子进程已经退出，仍清理继承管道的同组后代。
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(any(unix, windows))]
enum ReadState {
    Bytes(usize),
    Pending,
    Closed,
}

#[cfg(unix)]
trait ProbePipe {
    fn read_ready(&mut self, buffer: &mut [u8]) -> io::Result<ReadState>;
}

#[cfg(unix)]
fn prepare_pipe(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // 管道仅归本执行器所有，非阻塞标记不会影响调用方的其他文件。
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
impl<T: Read> ProbePipe for T {
    fn read_ready(&mut self, buffer: &mut [u8]) -> io::Result<ReadState> {
        match self.read(buffer) {
            Ok(0) => Ok(ReadState::Closed),
            Ok(count) => Ok(ReadState::Bytes(count)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(ReadState::Pending),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(ReadState::Pending),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_prevents_spawn_even_for_missing_commands() {
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            run_probe(
                &mut Command::new("zenv-probe-missing-command"),
                &ProbeLimits {
                    timeout: Duration::from_secs(1),
                    max_output_bytes: 1024,
                },
                &cancellation,
            ),
            Err(ProbeError::Cancelled)
        ));
    }

    #[test]
    fn zero_timeout_prevents_spawn() {
        assert!(matches!(
            run_probe(
                &mut Command::new("zenv-probe-missing-command"),
                &ProbeLimits {
                    timeout: Duration::ZERO,
                    max_output_bytes: 1024,
                },
                &CancellationToken::default(),
            ),
            Err(ProbeError::Timeout)
        ));
    }

    #[cfg(not(any(unix, windows)))]
    #[test]
    fn unsupported_platform_does_not_spawn() {
        assert!(matches!(
            run_probe(
                &mut Command::new("zenv-probe-missing-command"),
                &ProbeLimits {
                    timeout: Duration::from_secs(1),
                    max_output_bytes: 1024,
                },
                &CancellationToken::default(),
            ),
            Err(ProbeError::Io(error)) if error.kind() == io::ErrorKind::Unsupported
        ));
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::path::Path;

        struct Script {
            directory: tempfile::TempDir,
        }

        impl Script {
            fn new(source: &str) -> Self {
                let directory = tempfile::tempdir().unwrap();
                std::fs::write(directory.path().join("probe.sh"), source).unwrap();
                Self { directory }
            }

            fn command(&self) -> Command {
                let mut command = Command::new("/bin/sh");
                command
                    .arg(self.directory.path().join("probe.sh"))
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin");
                command
            }
        }

        fn limits() -> ProbeLimits {
            ProbeLimits {
                timeout: Duration::from_secs(2),
                max_output_bytes: 4096,
            }
        }

        #[test]
        fn explicit_environment_and_nonzero_status_are_preserved() {
            let script = Script::new(
                "printf '%s|%s' \"$PATH\" \"$ZEN_PROBE_VALUE\"; printf '中文' >&2; exit 7",
            );
            let result = run_probe(
                script
                    .command()
                    .env("PATH", "/probe/explicit-only")
                    .env("ZEN_PROBE_VALUE", "literal value"),
                &limits(),
                &CancellationToken::default(),
            )
            .unwrap();
            assert_eq!(result.status.code(), Some(7));
            assert_eq!(result.stdout, b"/probe/explicit-only|literal value");
            assert_eq!(result.stderr, "中文".as_bytes());
            let success = Script::new("exit 0");
            let result = run_probe(
                &mut success.command(),
                &limits(),
                &CancellationToken::default(),
            )
            .unwrap();
            assert!(result.status.success());
            assert!(result.stdout.is_empty());
            assert!(result.stderr.is_empty());
        }

        #[test]
        fn output_limit_combines_both_streams_and_bounds_infinite_output() {
            for source in [
                "printf 'abcd'; printf 'efgh' >&2",
                "while :; do printf 'abcdefgh'; done",
            ] {
                let script = Script::new(source);
                let started = Instant::now();
                assert!(matches!(
                    run_probe(
                        &mut script.command(),
                        &ProbeLimits {
                            max_output_bytes: 7,
                            ..limits()
                        },
                        &CancellationToken::default(),
                    ),
                    Err(ProbeError::OutputLimit)
                ));
                assert!(started.elapsed() < Duration::from_secs(1));
            }
        }

        #[test]
        fn exact_output_limit_and_zero_output_limit_allow_success() {
            for (source, max_output_bytes) in [("printf 'ab'; printf 'cd' >&2", 4), ("exit 0", 0)] {
                let script = Script::new(source);
                assert!(run_probe(
                    &mut script.command(),
                    &ProbeLimits {
                        max_output_bytes,
                        ..limits()
                    },
                    &CancellationToken::default(),
                )
                .unwrap()
                .status
                .success());
            }
        }

        #[test]
        fn timeout_reaps_children_holding_pipes_after_parent_exits() {
            let script = Script::new("sleep 30 & printf '%s' \"$!\" > \"$PID_FILE\"; exit 0");
            let pid_file = script.directory.path().join("child.pid");
            let started = Instant::now();
            assert!(matches!(
                run_probe(
                    script.command().env("PID_FILE", &pid_file),
                    &ProbeLimits {
                        timeout: Duration::from_millis(150),
                        ..limits()
                    },
                    &CancellationToken::default(),
                ),
                Err(ProbeError::Timeout)
            ));
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_child_stopped(&pid_file);
        }

        #[test]
        fn successful_parent_also_reaps_descendants_that_closed_output() {
            let script = Script::new(
                "sleep 30 </dev/null >/dev/null 2>&1 & printf '%s' \"$!\" > \"$PID_FILE\"; exit 0",
            );
            let pid_file = script.directory.path().join("child.pid");
            let result = run_probe(
                script.command().env("PID_FILE", &pid_file),
                &limits(),
                &CancellationToken::default(),
            )
            .unwrap();
            assert!(result.status.success());
            assert!(result.stdout.is_empty());
            assert!(result.stderr.is_empty());
            assert_child_stopped(&pid_file);
        }

        #[test]
        fn cancellation_during_execution_reaps_the_process_group() {
            let script = Script::new("sleep 30 & printf '%s' \"$!\" > \"$PID_FILE\"; wait");
            let pid_file = script.directory.path().join("child.pid");
            let cancellation = CancellationToken::default();
            let cancel = cancellation.clone();
            let marker = pid_file.clone();
            let trigger = std::thread::spawn(move || {
                let started = Instant::now();
                while !marker.exists() && started.elapsed() < Duration::from_secs(1) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                cancel.cancel();
            });
            let started = Instant::now();
            assert!(matches!(
                run_probe(
                    script.command().env("PID_FILE", &pid_file),
                    &limits(),
                    &cancellation,
                ),
                Err(ProbeError::Cancelled)
            ));
            trigger.join().unwrap();
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_child_stopped(&pid_file);
        }

        #[test]
        fn output_overflow_reaps_the_process_group() {
            let script = Script::new(
                "sleep 30 & printf '%s' \"$!\" > \"$PID_FILE\"; while :; do printf 'abcdefgh'; done",
            );
            let pid_file = script.directory.path().join("child.pid");
            assert!(matches!(
                run_probe(
                    script.command().env("PID_FILE", &pid_file),
                    &ProbeLimits {
                        max_output_bytes: 7,
                        ..limits()
                    },
                    &CancellationToken::default(),
                ),
                Err(ProbeError::OutputLimit)
            ));
            assert_child_stopped(&pid_file);
        }

        fn assert_child_stopped(pid_file: &Path) {
            let pid: i32 = std::fs::read_to_string(pid_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let started = Instant::now();
            loop {
                // signal 0 只检查存活，不执行 setuid 的 /bin/ps：沙箱和加固环境常拒绝后者。
                // 僵尸进程在被 init/launchd 回收前仍会返回成功，因此给它一段回收时间。
                if unsafe { libc::kill(pid, 0) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::ESRCH) {
                        break;
                    }
                }
                if started.elapsed() > Duration::from_secs(2) {
                    // 失败测试也清理后代，避免让测试套件遗留后台进程。
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                    panic!("probe descendant is still running: pid {pid}");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
