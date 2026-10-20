//! Windows 原生场景；由本测试二进制派生 fixture，无下载或 PowerShell 依赖。
use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::{fs, io::Write, path::Path, process::Stdio};
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

fn limits() -> ProbeLimits {
    ProbeLimits {
        timeout: Duration::from_secs(8),
        max_output_bytes: 4096,
    }
}

fn fixture_command(mode: &str) -> Command {
    let fixture = format!("{}::fixture", module_path!().split_once("::").unwrap().1);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &fixture,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
            "--quiet",
        ])
        .env("ZENV_PROBE_FIXTURE", mode);
    command
}

fn wait_marker(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "fixture did not reach {path:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn watched_probe(
    mode: &str,
    budget: ProbeLimits,
    cancel: bool,
) -> (Result<ProbeOutput, ProbeError>, OwnedHandle) {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("child.pid");
    let ready = directory.path().join("observer.ready");
    let cancellation = CancellationToken::default();
    let trigger = cancellation.clone();
    let watch = marker.clone();
    let acknowledge = ready.clone();
    let observer = std::thread::spawn(move || {
        wait_marker(&watch);
        let pid: u32 = fs::read_to_string(&watch).unwrap().parse().unwrap();
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
        assert!(
            !handle.is_null(),
            "cannot observe fixture: {}",
            io::Error::last_os_error()
        );
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        // fixture 在确认前不退出或洪泛输出，使观测句柄确定指向本次子进程。
        fs::write(acknowledge, b"observed").unwrap();
        if cancel {
            trigger.cancel();
        }
        handle
    });
    let mut command = fixture_command(mode);
    command
        .env("ZENV_MARKER", &marker)
        .env("ZENV_OBSERVER_READY", &ready);
    let result = run_probe(&mut command, &budget, &cancellation);
    (result, observer.join().unwrap())
}

fn assert_stopped(handle: OwnedHandle) {
    let status = unsafe { WaitForSingleObject(handle.as_raw_handle(), 3000) };
    if status != WAIT_OBJECT_0 {
        // 先兜底清理再报告回收失败，避免有缺陷的实现污染后续原生测试。
        unsafe {
            TerminateProcess(handle.as_raw_handle(), 1);
            WaitForSingleObject(handle.as_raw_handle(), 3000);
        }
    }
    assert_eq!(status, WAIT_OBJECT_0, "probe left a live fixture process");
}

fn publish_pid(path: &Path) {
    let temporary = path.with_extension("pending");
    fs::write(&temporary, std::process::id().to_string()).unwrap();
    fs::rename(temporary, path).unwrap();
}

#[test]
fn explicit_arguments_environment_cwd_unicode_and_nonzero_status_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let argument = "空格 and \"quotes\" \\ tail\\";
    let expected = format!(
        "{argument}|literal value|{}|中文",
        directory.path().display()
    );
    let mut command = fixture_command("observe");
    command
        .args(["--skip", argument])
        .env("ZENV_LITERAL", "literal value")
        .current_dir(directory.path());
    let result = run_probe(&mut command, &limits(), &CancellationToken::default()).unwrap();
    assert_eq!(result.status.code(), Some(7));
    assert!(result.stdout.ends_with(expected.as_bytes()));
    assert_eq!(result.stderr, b"stderr");
}

#[test]
fn both_streams_share_exact_budget_and_zero_budget_accepts_empty_success() {
    // 用固定输出的 cmd 验证字节预算；测试二进制由 libtest 包一层前缀，跨平台字节数不稳定。
    let reference = run_probe(
        &mut budget_command(),
        &limits(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(reference.stdout.ends_with(b"abcd\r\n"));
    assert_eq!(reference.stderr, b"efgh\r\n");
    let exact = reference.stdout.len() + reference.stderr.len();
    let result = run_probe(
        &mut budget_command(),
        &ProbeLimits {
            max_output_bytes: exact,
            ..limits()
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(result.status.success());
    assert!(matches!(
        run_probe(
            &mut budget_command(),
            &ProbeLimits {
                max_output_bytes: exact - 1,
                ..limits()
            },
            &CancellationToken::default()
        ),
        Err(ProbeError::OutputLimit)
    ));
    let mut empty = empty_command();
    let result = run_probe(
        &mut empty,
        &ProbeLimits {
            max_output_bytes: 0,
            ..limits()
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty() && result.stderr.is_empty());
}

fn empty_command() -> Command {
    let mut command =
        Command::new(Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe"));
    // 固定测试命令，不拼接任何用户参数，不加载 cmd AutoRun。
    command.args(["/d", "/c", "exit 0"]);
    command
}

fn budget_command() -> Command {
    let mut command =
        Command::new(Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe"));
    // 固定两行输出（stdout + stderr），不依赖 libtest 前缀或控制台宽度。
    command.args(["/d", "/c", "echo abcd& echo efgh 1>&2"]);
    command
}

#[test]
fn continuous_output_is_bounded_and_process_is_reaped() {
    let (result, child) = watched_probe("flood", limits(), false);
    assert_stopped(child);
    assert!(matches!(result, Err(ProbeError::OutputLimit)));
}

#[test]
fn cancellation_reaps_descendants_before_returning() {
    let (result, child) = watched_probe("parent-wait", limits(), true);
    assert_stopped(child);
    assert!(matches!(result, Err(ProbeError::Cancelled)));
}

#[test]
fn timeout_reaps_descendants_holding_pipes_after_parent_exits() {
    let (result, child) = watched_probe(
        "parent-exit",
        ProbeLimits {
            timeout: Duration::from_secs(3),
            ..limits()
        },
        false,
    );
    assert_stopped(child);
    assert!(matches!(result, Err(ProbeError::Timeout)));
}

#[test]
#[ignore = "Windows 原生 Job/管道继承场景待复核；见 docs/cross-platform-foundation.md"]
fn successful_parent_also_reaps_descendants_that_closed_output() {
    let (result, child) = watched_probe("parent-detached-output", limits(), false);
    assert_stopped(child);
    assert!(result.unwrap().status.success());
}

#[test]
fn command_reuse_does_not_inherit_a_suspended_creation_flag() {
    let mut command = fixture_command("empty");
    assert!(
        run_probe(&mut command, &limits(), &CancellationToken::default())
            .unwrap()
            .status
            .success()
    );
    let mut child = command.spawn().unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("reused command remained suspended");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "subprocess fixture; invoked only by native probe tests"]
fn fixture() {
    let Ok(mode) = std::env::var("ZENV_PROBE_FIXTURE") else {
        return;
    };
    let marker = std::env::var_os("ZENV_MARKER");
    match mode.as_str() {
        "observe" => {
            let text = format!(
                "{}|{}|{}|中文",
                std::env::args().next_back().unwrap(),
                std::env::var("ZENV_LITERAL").unwrap(),
                std::env::current_dir().unwrap().display()
            );
            io::stdout().write_all(text.as_bytes()).unwrap();
            io::stderr().write_all(b"stderr").unwrap();
            std::process::exit(7);
        }
        "small" => {
            io::stdout().write_all(b"abcd").unwrap();
            io::stderr().write_all(b"efgh").unwrap();
        }
        "sleep" => {
            publish_pid(Path::new(&marker.unwrap()));
            wait_marker(Path::new(&std::env::var_os("ZENV_OBSERVER_READY").unwrap()));
            std::thread::sleep(Duration::from_secs(60));
        }
        "flood" => {
            publish_pid(Path::new(&marker.unwrap()));
            wait_marker(Path::new(&std::env::var_os("ZENV_OBSERVER_READY").unwrap()));
            loop {
                io::stdout().write_all(b"abcdefgh").unwrap();
                io::stderr().write_all(b"abcdefgh").unwrap();
            }
        }
        "parent-wait" | "parent-exit" | "parent-detached-output" => {
            let marker = marker.unwrap();
            let mut child = fixture_command("sleep");
            child.env("ZENV_MARKER", &marker);
            if mode == "parent-detached-output" {
                child.stdout(Stdio::null()).stderr(Stdio::null());
            }
            // 父退出场景必须留下运行中的后代，由被测探针的 Job 守卫回收。
            #[allow(
                clippy::zombie_processes,
                reason = "intentional orphan fixture for Job cleanup"
            )]
            let mut child = child.spawn().unwrap();
            wait_marker(Path::new(&marker));
            wait_marker(Path::new(&std::env::var_os("ZENV_OBSERVER_READY").unwrap()));
            if mode == "parent-wait" {
                let _ = child.wait();
            } else {
                // 故意在后代仍存活时退出，验证父进程退出后的 Job 回收。
                std::process::exit(0);
            }
        }
        "empty" => {}
        _ => panic!("unknown fixture mode"),
    }
    // 不输出 libtest 成功后缀，探针只需区分真实载荷和已知 harness 前缀。
    std::process::exit(0);
}
