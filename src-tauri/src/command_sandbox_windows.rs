//! Windows AppContainer + Job Object isolation for `run_command`.

use std::ffi::c_void;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::Path;
use std::process::{ExitStatus, Output};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, BOOL, HANDLE, HLOCAL, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
    SE_FILE_OBJECT, SET_ACCESS, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
};
use windows::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows::Win32::Security::{
    FreeSid, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SUB_CONTAINERS_AND_OBJECTS_INHERIT, ACL,
    DACL_SECURITY_INFORMATION, PACL, PSID,
};
use windows::Win32::Storage::FileSystem::{FILE_ALL_ACCESS, FILE_GENERIC_READ};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject,
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, STARTUPINFOW,
};

use crate::command_sandbox::CommandSandboxPlan;
use crate::runtime_compat::SandboxProfileV1;

const PROFILE_NAME: &str = "Sunsetz.CmdSandbox";
const ACTIVE_PROCESS_LIMIT: u32 = 32;

struct WideString(Vec<u16>);

impl WideString {
    fn from_path(path: &Path) -> Self {
        use std::os::windows::ffi::OsStrExt;
        Self(
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect(),
        )
    }

    fn from_str(value: &str) -> Self {
        use std::os::windows::ffi::OsStrExt;
        Self(
            std::ffi::OsStr::new(value)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect(),
        )
    }

    fn pcwstr(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}

struct SidHandle(PSID);

impl Drop for SidHandle {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe {
                let _ = FreeSid(self.0);
            }
        }
    }
}

fn ensure_profile() -> Result<SidHandle, String> {
    let name = WideString::from_str(PROFILE_NAME);
    let display = WideString::from_str("Sunsetz command sandbox");
    unsafe {
        match CreateAppContainerProfile(name.pcwstr(), display.pcwstr(), display.pcwstr(), None) {
            Ok(sid) => return Ok(SidHandle(sid)),
            Err(error) if already_exists(error.code()) => {}
            Err(error) => {
                return Err(format!(
                    "SANDBOX_UNAVAILABLE: CreateAppContainerProfile failed ({error})"
                ));
            }
        }
        DeriveAppContainerSidFromAppContainerName(name.pcwstr())
            .map(SidHandle)
            .map_err(|error| {
                format!(
                    "SANDBOX_UNAVAILABLE: DeriveAppContainerSidFromAppContainerName failed ({error})"
                )
            })
    }
}

fn already_exists(code: HRESULT) -> bool {
    code.0 == 0x8007_00B7_u32 as i32 || code.0 == 183
}

struct AclRestore {
    path: WideString,
    old_dacl: PACL,
    descriptor: PSECURITY_DESCRIPTOR,
}

impl Drop for AclRestore {
    fn drop(&mut self) {
        unsafe {
            let _ = SetNamedSecurityInfoW(
                self.path.pcwstr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(self.old_dacl.0 as *const ACL),
                None,
            );
            if !self.descriptor.0.is_null() {
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0 as *mut c_void)));
            }
        }
    }
}

fn grant_path(sid: PSID, path: &Path, write: bool) -> Result<AclRestore, String> {
    let wide = WideString::from_path(path);
    let mut old_dacl = PACL::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        GetNamedSecurityInfoW(
            wide.pcwstr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut old_dacl),
            None,
            &mut descriptor,
        )
        .map_err(|error| format!("SANDBOX_UNAVAILABLE: GetNamedSecurityInfoW failed ({error})"))?;
    }
    let access = if write {
        FILE_ALL_ACCESS.0
    } else {
        FILE_GENERIC_READ.0
    };
    let mut trustee = TRUSTEE_W::default();
    trustee.TrusteeForm = TRUSTEE_IS_SID;
    trustee.TrusteeType = TRUSTEE_IS_WELL_KNOWN_GROUP;
    trustee.ptstrName = PWSTR(sid.0 as *mut u16);
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access,
        grfAccessMode: SET_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: trustee,
    };
    let mut new_dacl = PACL::default();
    unsafe {
        SetEntriesInAclW(Some(&[entry]), Some(old_dacl), &mut new_dacl).map_err(|error| {
            format!("SANDBOX_UNAVAILABLE: SetEntriesInAclW failed ({error})")
        })?;
        SetNamedSecurityInfoW(
            wide.pcwstr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(new_dacl.0 as *const ACL),
            None,
        )
        .map_err(|error| format!("SANDBOX_UNAVAILABLE: SetNamedSecurityInfoW failed ({error})"))?;
        let _ = LocalFree(Some(HLOCAL(new_dacl.0 as *mut c_void)));
    }
    Ok(AclRestore {
        path: wide,
        old_dacl,
        descriptor,
    })
}

fn create_inheritable_pipe() -> Result<(HANDLE, HANDLE), String> {
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: BOOL(1),
    };
    unsafe {
        CreatePipe(&mut read, &mut write, Some(&attrs), 0)
            .map_err(|error| format!("SANDBOX_UNAVAILABLE: CreatePipe failed ({error})"))?;
    }
    Ok((read, write))
}

fn command_line(plan: &CommandSandboxPlan) -> WideString {
    let mut line = String::from("cmd.exe");
    for arg in &plan.args {
        line.push(' ');
        let value = arg.to_string_lossy();
        if value.contains(' ') {
            line.push('"');
            line.push_str(&value.replace('"', "\\\""));
            line.push('"');
        } else {
            line.push_str(&value);
        }
    }
    WideString::from_str(&line)
}

pub fn run_windows_isolated_sync(plan: &CommandSandboxPlan) -> Result<Output, String> {
    let (stdout, stderr, code, _) =
        run_windows_isolated_inner(plan, None, Duration::from_secs(60))?;
    Ok(Output {
        status: exit_status(code),
        stdout,
        stderr,
    })
}

fn exit_status(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(code as u32)
}

pub async fn run_windows_isolated(
    plan: CommandSandboxPlan,
    stop: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<(Vec<u8>, Vec<u8>, i32, bool), String> {
    tokio::task::spawn_blocking(move || run_windows_isolated_inner(&plan, Some(stop), timeout))
        .await
        .map_err(|error| format!("run_command: {error}"))?
}

fn run_windows_isolated_inner(
    plan: &CommandSandboxPlan,
    stop: Option<Arc<AtomicBool>>,
    timeout: Duration,
) -> Result<(Vec<u8>, Vec<u8>, i32, bool), String> {
    let sid = ensure_profile()?;
    let write_project = plan.profile == SandboxProfileV1::WorkspaceWrite;
    let _project_acl = grant_path(sid.0, &plan.project_root, write_project)?;
    let temp = std::env::temp_dir().join(format!("sunsetz-sandbox-{}", std::process::id()));
    std::fs::create_dir_all(&temp).map_err(|error| format!("sandbox temp: {error}"))?;
    let _temp_acl = grant_path(sid.0, &temp, true)?;

    let job = unsafe {
        CreateJobObjectW(None, PCWSTR::null()).map_err(|error| {
            format!("SANDBOX_UNAVAILABLE: CreateJobObjectW failed ({error})")
        })?
    };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    limits.BasicLimitInformation.ActiveProcessLimit = ACTIVE_PROCESS_LIMIT;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|error| {
            format!("SANDBOX_UNAVAILABLE: SetInformationJobObject failed ({error})")
        })?;
    }

    let (stdout_r, stdout_w) = create_inheritable_pipe()?;
    let (stderr_r, stderr_w) = create_inheritable_pipe()?;

    let mut capabilities = windows::Win32::Security::SECURITY_CAPABILITIES {
        AppContainerSid: sid.0,
        Capabilities: ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };

    let mut attr_size = 0_usize;
    unsafe {
        let _ = InitializeProcThreadAttributeList(
            LPPROC_THREAD_ATTRIBUTE_LIST(ptr::null_mut()),
            1,
            0,
            &mut attr_size,
        );
    }
    let mut attr_buf = vec![0_u8; attr_size.max(1)];
    let attr_list = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buf.as_mut_ptr() as *mut c_void);
    unsafe {
        InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size).map_err(|error| {
            format!("SANDBOX_UNAVAILABLE: InitializeProcThreadAttributeList failed ({error})")
        })?;
        UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            Some((&mut capabilities as *mut _).cast::<c_void>()),
            std::mem::size_of::<windows::Win32::Security::SECURITY_CAPABILITIES>(),
            None,
            None,
        )
        .map_err(|error| {
            format!("SANDBOX_UNAVAILABLE: UpdateProcThreadAttribute failed ({error})")
        })?;
    }

    let mut startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
            dwFlags: STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW,
            hStdOutput: stdout_w,
            hStdError: stderr_w,
            ..Default::default()
        },
        lpAttributeList: attr_list,
    };
    let mut process_info = PROCESS_INFORMATION::default();
    let mut cmd = command_line(plan);
    let cwd = WideString::from_path(&plan.current_dir);
    let created = unsafe {
        CreateProcessW(
            None,
            PWSTR(cmd.0.as_mut_ptr()),
            None,
            None,
            true,
            CREATE_NO_WINDOW
                | CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT,
            None,
            cwd.pcwstr(),
            &startup.StartupInfo,
            &mut process_info,
        )
    };
    unsafe {
        let _ = CloseHandle(stdout_w);
        let _ = CloseHandle(stderr_w);
        DeleteProcThreadAttributeList(attr_list);
    }
    created.map_err(|error| format!("SANDBOX_UNAVAILABLE: CreateProcessW failed ({error})"))?;

    unsafe {
        if let Err(error) = AssignProcessToJobObject(job, process_info.hProcess) {
            let _ = TerminateJobObject(job, 1);
            let _ = CloseHandle(process_info.hThread);
            let _ = CloseHandle(process_info.hProcess);
            return Err(format!(
                "SANDBOX_UNAVAILABLE: AssignProcessToJobObject failed ({error})"
            ));
        }
        ResumeThread(process_info.hThread);
        let _ = CloseHandle(process_info.hThread);
    }

    let mut stdout_file = unsafe { std::fs::File::from_raw_handle(stdout_r.0 as RawHandle) };
    let mut stderr_file = unsafe { std::fs::File::from_raw_handle(stderr_r.0 as RawHandle) };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    let mut cancelled = false;
    loop {
        if stop
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
        {
            cancelled = true;
            unsafe {
                let _ = TerminateJobObject(job, 1);
            }
            break;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            unsafe {
                let _ = TerminateJobObject(job, 1);
            }
            let _ = std::io::Read::read_to_end(&mut stdout_file, &mut stdout);
            let _ = std::io::Read::read_to_end(&mut stderr_file, &mut stderr);
            unsafe {
                let _ = CloseHandle(process_info.hProcess);
                let _ = CloseHandle(job);
            }
            return Err(format!(
                "command timed out after {}s",
                timeout.as_secs().max(1)
            ));
        }
        let wait = remaining.min(Duration::from_millis(50));
        let rc = unsafe { WaitForSingleObject(process_info.hProcess, wait.as_millis() as u32) };
        if rc == WAIT_OBJECT_0 {
            break;
        }
        if rc != WAIT_TIMEOUT {
            break;
        }
    }
    let _ = std::io::Read::read_to_end(&mut stdout_file, &mut stdout);
    let _ = std::io::Read::read_to_end(&mut stderr_file, &mut stderr);
    let mut code = 1_u32;
    unsafe {
        let _ = GetExitCodeProcess(process_info.hProcess, &mut code);
        let _ = CloseHandle(process_info.hProcess);
        let _ = CloseHandle(job);
    }
    Ok((stdout, stderr, code as i32, cancelled))
}
