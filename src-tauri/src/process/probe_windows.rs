//! Windows 探针：挂起启动、先纳入 Job 再恢复，管道读取沿用共享预算循环。
use super::{Child, Command, Read, ReadState};
use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::PeekNamedPipe;
use windows_sys::Win32::System::Threading::{
    GetProcessIdOfThread, OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

pub(super) struct RunningChild {
    pub(super) child: Child,
    job: Option<OwnedHandle>,
}

impl RunningChild {
    pub(super) fn spawn(command: &mut Command) -> io::Result<Self> {
        // job 不可继承，只有当前守卫拥有；关闭最后一个句柄时终止全部后代。
        let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        checked(unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        })?;
        // 不允许先运行后再附加 Job 的窗口，避免子进程在被管理前派生逃逸后代。
        // std 的 spawn_with_attributes 仍为 nightly API，使用稳定的挂起启动路径。
        let spawned = command
            .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)
            .spawn();
        // 不把挂起标记留在调用方可复用的 builder 上，spawn 失败时也必须恢复。
        command.creation_flags(0);
        let child = spawned?;
        let running = Self {
            child,
            job: Some(job),
        };
        checked(unsafe {
            AssignProcessToJobObject(
                running.job.as_ref().unwrap().as_raw_handle(),
                running.child.as_raw_handle(),
            )
        })?;
        resume_initial_thread(running.child.id())?;
        Ok(running)
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        // 直接进程退出后，也清理继承管道的后代；附加失败时 kill 清理挂起的进程。
        drop(self.job.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // 所有调用均返回独立拥有的有效内核句柄，不转移调用方的句柄。
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn checked(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn resume_initial_thread(pid: u32) -> io::Result<()> {
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut next = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    let mut thread_id = None;
    while next != 0 {
        if entry.th32OwnerProcessID == pid && thread_id.replace(entry.th32ThreadID).is_some() {
            return Err(io::Error::other(
                "suspended probe has multiple initial threads",
            ));
        }
        next = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error);
    }
    let thread_id = thread_id.ok_or_else(|| io::Error::other("probe initial thread is missing"))?;
    let thread = owned(unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            0,
            thread_id,
        )
    })?;
    // 快照之后子进程可能被外部终止，TID 随后重用；恢复前核对已打开句柄的归属。
    if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
        return Err(io::Error::other("probe initial thread ownership changed"));
    }
    match unsafe { ResumeThread(thread.as_raw_handle()) } {
        1 => Ok(()),
        u32::MAX => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other(
            "probe initial thread has unexpected suspend count",
        )),
    }
}

// std 创建的管道只由本线程读取。先 peek 后只读已到达字节，避免阻塞取消或另一条流。
pub(super) fn prepare_pipe(_pipe: &impl AsRawHandle) -> io::Result<()> {
    Ok(())
}

pub(super) trait ProbePipe {
    fn read_ready(&mut self, buffer: &mut [u8]) -> io::Result<ReadState>;
}

impl<T: Read + AsRawHandle> ProbePipe for T {
    fn read_ready(&mut self, buffer: &mut [u8]) -> io::Result<ReadState> {
        let mut available = 0_u32;
        let result = unsafe {
            PeekNamedPipe(
                self.as_raw_handle(),
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                Ok(ReadState::Closed)
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Ok(ReadState::Pending);
        }
        let count = buffer.len().min(available as usize);
        match self.read(&mut buffer[..count])? {
            0 => Ok(ReadState::Closed),
            count => Ok(ReadState::Bytes(count)),
        }
    }
}
