use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use windows_sys::Win32::Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

pub(super) struct Job(OwnedHandle);

impl Job {
    fn new() -> io::Result<Self> {
        // An unnamed, non-inheritable job is owned solely by this timeout scope.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    pub(super) fn terminate(&self, diagnostics: &mut Vec<String>) {
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            diagnostics.push(format!(
                "failed to terminate child job: {}",
                io::Error::last_os_error()
            ));
        } else {
            diagnostics.push("sent kill to child process job".to_string());
        }
    }
}

pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Job)> {
    let job = Job::new()?;
    let flags = CREATE_SUSPENDED
        | if crate::process_spawn::is_interactive_terminal() {
            0
        } else {
            crate::process_spawn::CREATE_NO_WINDOW
        };
    let mut child = command.creation_flags(flags).spawn()?;
    // Assign before any child code runs, so transport helpers cannot escape
    // the job between CreateProcess and AssignProcessToJobObject.
    let assigned =
        unsafe { AssignProcessToJobObject(job.0.as_raw_handle(), child.as_raw_handle()) };
    let result = if assigned == 0 {
        Err(io::Error::last_os_error())
    } else {
        resume_initial_thread(child.id())
    };
    if let Err(error) = result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok((child, job))
}

fn resume_initial_thread(process_id: u32) -> io::Result<()> {
    // Rust 1.93 does not expose Child's primary thread handle on stable. A
    // suspended fresh process has one initial thread; fail closed if injected
    // threads make that identity ambiguous instead of resuming the wrong one.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    if unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut thread_id = None;
    loop {
        if entry.th32OwnerProcessID == process_id && thread_id.replace(entry.th32ThreadID).is_some()
        {
            return Err(io::Error::other("suspended child has multiple threads"));
        }
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error);
            }
            break;
        }
    }
    let thread_id =
        thread_id.ok_or_else(|| io::Error::other("suspended child thread not found"))?;
    let raw = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
    match unsafe { ResumeThread(thread.as_raw_handle()) } {
        1 => Ok(()),
        u32::MAX => Err(io::Error::last_os_error()),
        count => Err(io::Error::other(format!(
            "unexpected child suspension count: {count}"
        ))),
    }
}
