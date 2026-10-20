// 统一进程执行器：退出码判断、UTF-8 流、有限日志、超时/取消时回收进程树。
use crate::{error::AppError, operations};
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[path = "process/probe.rs"]
pub mod probe;

const MAX_OUTPUT: usize = 2 * 1024 * 1024;

/// 旧入口保留 mise PATH；领域调用方可以明确保留其已配置的环境。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentPolicy {
    LegacyMisePath,
    /// 不覆盖命令的环境、PATH 或 cwd；调用方仍负责显式清理继承环境。
    AsConfigured,
}

pub fn output(command: &mut Command, timeout: Duration) -> Result<Output, AppError> {
    output_with_environment(command, timeout, EnvironmentPolicy::LegacyMisePath)
}

pub fn output_with_environment(
    command: &mut Command,
    timeout: Duration,
    environment: EnvironmentPolicy,
) -> Result<Output, AppError> {
    run_with_environment(
        command,
        timeout,
        operations::current_id().as_deref(),
        environment,
        |_| {},
    )
}

pub fn run(
    command: &mut Command,
    timeout: Duration,
    job: Option<&str>,
    on_line: impl FnMut(&str),
) -> Result<Output, AppError> {
    run_with_environment(
        command,
        timeout,
        job,
        EnvironmentPolicy::LegacyMisePath,
        on_line,
    )
}

pub fn run_with_environment(
    command: &mut Command,
    timeout: Duration,
    job: Option<&str>,
    environment: EnvironmentPolicy,
    mut on_line: impl FnMut(&str),
) -> Result<Output, AppError> {
    if job.is_some_and(operations::cancelled) {
        return Err(AppError::Other("任务已取消".into()));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if environment == EnvironmentPolicy::LegacyMisePath {
        command.env("PATH", crate::mise::build_path());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound
            && std::path::Path::new(command.get_program())
                .file_name()
                .is_some_and(|name| name == "mise" || name == "mise.exe")
        {
            AppError::MiseNotInstalled
        } else {
            error.into()
        }
    })?;
    let (tx, rx) = mpsc::sync_channel(128);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    for (mut stream, is_error) in [
        (Box::new(stdout) as Box<dyn Read + Send>, false),
        (Box::new(stderr) as Box<dyn Read + Send>, true),
    ] {
        let sender = tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0; 8192];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if sender.send((is_error, buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    drop(tx);
    let start = Instant::now();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut pending = [Vec::new(), Vec::new()];
    let mut status = None;
    let mut exited = None;
    loop {
        if job.is_some_and(operations::cancelled) || start.elapsed() > timeout {
            kill_tree(&mut child);
            return Err(AppError::Other(
                if job.is_some_and(operations::cancelled) {
                    "任务已取消"
                } else {
                    "命令执行超时，已终止子进程"
                }
                .into(),
            ));
        }
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok((is_error, bytes)) => {
                let dest = if is_error { &mut err } else { &mut out };
                dest.extend(bytes.iter().take(MAX_OUTPUT.saturating_sub(dest.len())));
                let line = &mut pending[usize::from(is_error)];
                for byte in bytes {
                    if (byte == b'\n' || byte == b'\r' || line.len() >= 16384) && !line.is_empty() {
                        let text = String::from_utf8_lossy(line);
                        if let Some(id) = job {
                            operations::log(id, text.as_ref());
                        }
                        on_line(&text);
                        line.clear();
                    }
                    if byte != b'\n' && byte != b'\r' {
                        line.push(byte);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) if status.is_some() => break,
            _ => {}
        }
        if status.is_none() {
            status = child.try_wait()?;
            if status.is_some() {
                exited = Some(Instant::now());
            }
        }
        // 脱离的后台进程可能继承管道；不无限等待其关闭。
        if exited.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            break;
        }
    }
    for line in pending {
        if !line.is_empty() {
            let text = String::from_utf8_lossy(&line);
            if let Some(id) = job {
                operations::log(id, text.as_ref());
            }
            on_line(&text);
        }
    }
    Ok(Output {
        status: status.unwrap_or(child.wait()?),
        stdout: out,
        stderr: err,
    })
}

fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // 只终止本执行器创建的进程组，避免遗留下载器或提权子进程。
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn configured_environment_preserves_explicit_path_cwd_and_literal_values() {
        let parent_path = std::env::var_os("PATH");
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().canonicalize().unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf '%s|%s|%s' \"$PATH\" \"$ZENV_LITERAL\" \"$PWD\"",
            ])
            .env_clear()
            .env("PATH", "/controlled path/only")
            .env("ZENV_LITERAL", "literal $(unchanged) value")
            .current_dir(&cwd);
        let result = output_with_environment(
            &mut command,
            Duration::from_secs(2),
            EnvironmentPolicy::AsConfigured,
        )
        .unwrap();
        assert!(result.status.success());
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!(
                "/controlled path/only|literal $(unchanged) value|{}",
                cwd.display()
            )
        );
        assert_eq!(std::env::var_os("PATH"), parent_path);
    }

    #[test]
    fn configured_environment_does_not_reintroduce_removed_path_or_parent_values() {
        let result = output_with_environment(
            Command::new("/usr/bin/env").env_clear(),
            Duration::from_secs(2),
            EnvironmentPolicy::AsConfigured,
        )
        .unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty());
    }

    #[test]
    fn legacy_environment_keeps_compatibility_without_changing_global_path() {
        let parent_path = std::env::var_os("PATH");
        let expected = crate::mise::build_path();
        let result = output(
            Command::new("/bin/sh")
                .args(["-c", "printf '%s' \"$PATH\""])
                .env_clear()
                .env("PATH", "/caller/legacy-value"),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(result.status.success());
        assert_eq!(String::from_utf8(result.stdout).unwrap(), expected);
        assert_eq!(std::env::var_os("PATH"), parent_path);
    }

    #[test]
    fn configured_streaming_preserves_failures_and_timeout_handling() {
        let mut lines = Vec::new();
        let result = run_with_environment(
            Command::new("/bin/sh")
                .args(["-c", "printf '中文\\n'; printf 'problem\\n' >&2; exit 7"])
                .env_clear()
                .env("PATH", "/configured/only"),
            Duration::from_secs(2),
            None,
            EnvironmentPolicy::AsConfigured,
            |line| lines.push(line.to_string()),
        )
        .unwrap();
        assert_eq!(result.status.code(), Some(7));
        assert_eq!(result.stdout, "中文\n".as_bytes());
        assert_eq!(result.stderr, b"problem\n");
        assert!(lines.contains(&"中文".into()));
        assert!(lines.contains(&"problem".into()));
        let started = Instant::now();
        let result = run_with_environment(
            Command::new("/bin/sh")
                .args(["-c", "/bin/sleep 20 & wait"])
                .env_clear()
                .env("PATH", "/configured/only"),
            Duration::from_millis(80),
            None,
            EnvironmentPolicy::AsConfigured,
            |_| {},
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn empty_success_unicode_nonzero_and_timeout_are_distinct() {
        assert!(output(
            Command::new("sh").args(["-c", "exit 0"]),
            Duration::from_secs(1)
        )
        .unwrap()
        .status
        .success());
        let mut lines = Vec::new();
        let out = run(
            Command::new("sh").args(["-c", "printf '中文\\n'; exit 7"]),
            Duration::from_secs(1),
            None,
            |s| lines.push(s.to_string()),
        )
        .unwrap();
        assert_eq!(out.status.code(), Some(7));
        assert_eq!(lines, vec!["中文"]);
        let start = Instant::now();
        assert!(output(
            Command::new("sh").args(["-c", "sleep 20 & wait"]),
            Duration::from_millis(80)
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
