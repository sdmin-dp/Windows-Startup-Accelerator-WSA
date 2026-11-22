//! Minimal, dependency-free Win32 FFI bindings.
//!
//! Only the functions needed by WSA are declared here. Everything is
//! `extern "system"` and links against the Windows system DLLs directly,
//! so the crate needs no `windows-sys`, no `winapi` and no build script.

#![cfg(windows)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::ffi::{c_char, c_void};
use std::mem;
use std::ptr;

// ---------------------------------------------------------------------------
// Foundation types
// ---------------------------------------------------------------------------

pub type BOOL = i32;
pub type DWORD = u32;
pub type LONG = i32;
pub type LSTATUS = i32;
pub type HANDLE = *mut c_void;
pub type HKEY = HANDLE;
pub type SC_HANDLE = HANDLE;
pub type SERVICE_STATUS_HANDLE = HANDLE;
pub type WCHAR = u16;
pub type LPWSTR = *mut WCHAR;
pub type LPCWSTR = *const WCHAR;

pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;

pub const ERROR_SUCCESS: DWORD = 0;
pub const ERROR_NO_MORE_ITEMS: DWORD = 259;
pub const ERROR_FILE_NOT_FOUND: DWORD = 2;
pub const ERROR_MORE_DATA: DWORD = 234;

pub const KEY_READ: DWORD = 0x0002_0019;
pub const KEY_SET_VALUE: DWORD = 0x0002;
pub const STANDARD_RIGHTS_READ: DWORD = 0x0002_0000;
pub const KEY_QUERY_VALUE: DWORD = 0x0001;
pub const KEY_ENUMERATE_SUB_KEYS: DWORD = 0x0008;
pub const KEY_NOTIFY: DWORD = 0x0010;

pub const TH32CS_SNAPPROCESS: DWORD = 0x0000_0002;

pub const SC_MANAGER_CONNECT: DWORD = 0x0001;
pub const SC_MANAGER_ENUMERATE_SERVICE: DWORD = 0x0004;
pub const SC_MANAGER_ALL_ACCESS: DWORD = 0x000F_003F;

pub const SERVICE_KERNEL_DRIVER: DWORD = 0x0000_0001;
pub const SERVICE_FILE_SYSTEM_DRIVER: DWORD = 0x0000_0002;
pub const SERVICE_AUTO_START: DWORD = 0x0000_0002;
pub const SERVICE_DELAYED_AUTO_START: DWORD = 0x0000_0003;
pub const SERVICE_CONFIG_DELAYED_AUTO_START_INFO: DWORD = 3;
pub const SERVICE_QUERY_CONFIG: DWORD = 0x0001;
pub const SERVICE_CHANGE_CONFIG: DWORD = 0x0002;
pub const SERVICE_DEMAND_START: DWORD = 0x0000_0003;
pub const SERVICE_DISABLED: DWORD = 0x0000_0004;
pub const SERVICE_TYPE_ALL: DWORD = 0x0000_00FF;
pub const SERVICE_ACTIVE: DWORD = 0x0000_0001;
pub const SERVICE_INACTIVE: DWORD = 0x0000_0002;
pub const SERVICE_STATE_ALL: DWORD = SERVICE_ACTIVE | SERVICE_INACTIVE;
pub const SC_STATUS_PROCESS_INFO: DWORD = 0;
pub const SERVICE_NO_CHANGE: DWORD = 0xFFFF_FFFF;
pub const ERROR_INSUFFICIENT_BUFFER: DWORD = 122;
pub const ERROR_ACCESS_DENIED: DWORD = 5;

pub const REG_SZ: DWORD = 1;
pub const REG_EXPAND_SZ: DWORD = 2;
pub const REG_MULTI_SZ: DWORD = 7;
pub const REG_DWORD: DWORD = 4;
pub const REG_QWORD: DWORD = 11;

pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001_usize as HKEY;
pub const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002_usize as HKEY;

pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
pub const PROCESS_SET_INFORMATION: DWORD = 0x0200;
pub const PROCESS_TERMINATE: DWORD = 0x0001;
pub const PROCESS_VM_READ: DWORD = 0x0010;

pub const THREAD_PRIORITY_LOWEST: i32 = -2;
pub const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
pub const THREAD_PRIORITY_NORMAL: i32 = 0;
pub const THREAD_PRIORITY_ABOVE_NORMAL: i32 = 1;
pub const THREAD_PRIORITY_HIGHEST: i32 = 2;
pub const THREAD_PRIORITY_IDLE: i32 = -15;

pub const STARTF_USESHOWWINDOW: DWORD = 0x0001;
pub const SW_SHOWMINIMIZED: u16 = 2;
pub const SW_HIDE: u16 = 0;
pub const CREATE_NO_WINDOW: DWORD = 0x0800_0000;
pub const DETACHED_PROCESS: DWORD = 0x0000_0008;
pub const NORMAL_PRIORITY_CLASS: DWORD = 0x0000_0020;
pub const BELOW_NORMAL_PRIORITY_CLASS: DWORD = 0x0000_4000;
pub const IDLE_PRIORITY_CLASS: DWORD = 0x0000_0040;

pub const MAX_PATH: usize = 260;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FILETIME {
    pub dwLowDateTime: DWORD,
    pub dwHighDateTime: DWORD,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PROCESSENTRY32W {
    pub dwSize: DWORD,
    pub cntUsage: DWORD,
    pub th32ProcessID: DWORD,
    pub th32DefaultHeapID: usize,
    pub th32ModuleID: DWORD,
    pub cntThreads: DWORD,
    pub th32ParentProcessID: DWORD,
    pub pcPriClassBase: i32,
    pub dwFlags: DWORD,
    pub szExeFile: [WCHAR; 260],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct STARTUPINFOW {
    pub cb: DWORD,
    pub lpReserved: LPWSTR,
    pub lpDesktop: LPWSTR,
    pub lpTitle: LPWSTR,
    pub dwX: DWORD,
    pub dwY: DWORD,
    pub dwXSize: DWORD,
    pub dwYSize: DWORD,
    pub dwXCountChars: DWORD,
    pub dwYCountChars: DWORD,
    pub dwFillAttribute: DWORD,
    pub dwFlags: DWORD,
    pub wShowWindow: u16,
    pub cbReserved2: u16,
    pub lpReserved2: *mut u8,
    pub hStdInput: HANDLE,
    pub hStdOutput: HANDLE,
    pub hStdError: HANDLE,
}

impl Default for STARTUPINFOW {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PROCESS_INFORMATION {
    pub hProcess: HANDLE,
    pub hThread: HANDLE,
    pub dwProcessId: DWORD,
    pub dwThreadId: DWORD,
}

impl Default for PROCESS_INFORMATION {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MEMORYSTATUSEX {
    pub dwLength: DWORD,
    pub dwMemoryLoad: DWORD,
    pub ullTotalPhys: u64,
    pub ullAvailPhys: u64,
    pub ullTotalPageFile: u64,
    pub ullAvailPageFile: u64,
    pub ullTotalVirtual: u64,
    pub ullAvailVirtual: u64,
    pub ullAvailExtendedVirtual: u64,
}

impl Default for MEMORYSTATUSEX {
    fn default() -> Self {
        Self {
            dwLength: mem::size_of::<Self>() as DWORD,
            ..unsafe { mem::zeroed() }
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SERVICE_STATUS_PROCESS {
    pub dwServiceType: DWORD,
    pub dwCurrentState: DWORD,
    pub dwControlsAccepted: DWORD,
    pub dwWin32ExitCode: DWORD,
    pub dwServiceSpecificExitCode: DWORD,
    pub dwCheckPoint: DWORD,
    pub dwWaitHint: DWORD,
    pub dwProcessId: DWORD,
    pub dwServiceFlags: DWORD,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ENUM_SERVICE_STATUS_PROCESSW {
    pub lpServiceName: LPWSTR,
    pub lpDisplayName: LPWSTR,
    pub ServiceStatusProcess: SERVICE_STATUS_PROCESS,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct QUERY_SERVICE_CONFIGW {
    pub dwServiceType: DWORD,
    pub dwStartType: DWORD,
    pub dwErrorControl: DWORD,
    pub lpBinaryPathName: LPWSTR,
    pub lpLoadOrderGroup: LPWSTR,
    pub dwTagId: DWORD,
    pub lpDependencies: LPWSTR,
    pub lpServiceStartName: LPWSTR,
    pub lpDisplayName: LPWSTR,
}

// ---------------------------------------------------------------------------
// External functions
// ---------------------------------------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    pub fn GetLastError() -> DWORD;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn MultiByteToWideChar(
        cp: u32,
        flags: DWORD,
        text: *const c_char,
        len: i32,
        wide: LPWSTR,
        wide_len: i32,
    ) -> i32;
    pub fn WideCharToMultiByte(
        cp: u32,
        flags: DWORD,
        wide: LPCWSTR,
        len: i32,
        text: *mut c_char,
        text_len: i32,
        default: *const c_char,
        used: *mut BOOL,
    ) -> i32;
    pub fn GetModuleFileNameW(module: HANDLE, buffer: LPWSTR, size: DWORD) -> DWORD;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn GetCurrentProcessId() -> DWORD;

    pub fn RegOpenKeyExW(
        key: HKEY,
        subkey: LPCWSTR,
        options: DWORD,
        desired: DWORD,
        result: *mut HKEY,
    ) -> LSTATUS;
    pub fn RegCloseKey(key: HKEY) -> LSTATUS;
    pub fn RegQueryValueExW(
        key: HKEY,
        name: LPCWSTR,
        reserved: *mut DWORD,
        value_type: *mut DWORD,
        data: *mut u8,
        data_len: *mut DWORD,
    ) -> LSTATUS;
    pub fn RegEnumValueW(
        key: HKEY,
        index: DWORD,
        name: LPWSTR,
        name_len: *mut DWORD,
        reserved: *mut DWORD,
        value_type: *mut DWORD,
        data: *mut u8,
        data_len: *mut DWORD,
    ) -> LSTATUS;
    pub fn RegEnumKeyExW(
        key: HKEY,
        index: DWORD,
        name: LPWSTR,
        name_len: *mut DWORD,
        reserved: *mut DWORD,
        class_name: LPWSTR,
        class_len: *mut DWORD,
        last_write: *mut FILETIME,
    ) -> LSTATUS;
    pub fn RegSetValueExW(
        key: HKEY,
        name: LPCWSTR,
        reserved: DWORD,
        value_type: DWORD,
        data: *const u8,
        data_len: DWORD,
    ) -> LSTATUS;
    pub fn RegDeleteValueW(key: HKEY, name: LPCWSTR) -> LSTATUS;

    pub fn CreateToolhelp32Snapshot(flags: DWORD, process_id: DWORD) -> HANDLE;
    pub fn Process32FirstW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> BOOL;
    pub fn Process32NextW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> BOOL;
    pub fn OpenProcess(desired: DWORD, inherit: BOOL, pid: DWORD) -> HANDLE;
    pub fn SetPriorityClass(process: HANDLE, priority: DWORD) -> BOOL;
    pub fn SetProcessAffinityMask(process: HANDLE, mask: usize) -> BOOL;
    pub fn GetProcessAffinityMask(
        process: HANDLE,
        process_mask: *mut usize,
        system_mask: *mut usize,
    ) -> BOOL;
    pub fn TerminateProcess(process: HANDLE, exit_code: u32) -> BOOL;
    pub fn GetProcessTimes(
        process: HANDLE,
        creation: *mut FILETIME,
        exit: *mut FILETIME,
        kernel: *mut FILETIME,
        user: *mut FILETIME,
    ) -> BOOL;

    pub fn CreateProcessW(
        app: LPCWSTR,
        cmd: LPWSTR,
        process_attrs: *mut c_void,
        thread_attrs: *mut c_void,
        inherit: BOOL,
        flags: DWORD,
        env: *mut c_void,
        cwd: LPCWSTR,
        startup: *mut STARTUPINFOW,
        info: *mut PROCESS_INFORMATION,
    ) -> BOOL;
    pub fn WaitForSingleObject(handle: HANDLE, milliseconds: DWORD) -> DWORD;
    pub fn GetExitCodeProcess(process: HANDLE, code: *mut DWORD) -> BOOL;
    pub fn Sleep(milliseconds: DWORD);

    pub fn GlobalMemoryStatusEx(status: *mut MEMORYSTATUSEX) -> BOOL;
    pub fn GetTickCount64() -> u64;
    pub fn GetSystemTimeAsFileTime(time: *mut FILETIME);
    pub fn GetSystemTime(time: *mut SYSTEMTIME);
}

#[link(name = "advapi32")]
extern "system" {
    pub fn OpenSCManagerW(machine: LPCWSTR, database: LPCWSTR, access: DWORD) -> SC_HANDLE;
    pub fn OpenServiceW(
        manager: SC_HANDLE,
        name: LPCWSTR,
        access: DWORD,
    ) -> SC_HANDLE;
    pub fn EnumServicesStatusExW(
        manager: SC_HANDLE,
        info_level: DWORD,
        service_type: DWORD,
        state: DWORD,
        buffer: *mut u8,
        buffer_size: DWORD,
        needed: *mut DWORD,
        returned: *mut DWORD,
        resume: *mut *mut u8,
        group: LPCWSTR,
    ) -> BOOL;
    pub fn QueryServiceConfigW(
        service: SC_HANDLE,
        config: *mut u8,
        size: DWORD,
        needed: *mut DWORD,
    ) -> BOOL;
    pub fn CloseServiceHandle(handle: SC_HANDLE) -> BOOL;
    pub fn ChangeServiceConfigW(
        service: SC_HANDLE,
        service_type: DWORD,
        start_type: DWORD,
        error_control: DWORD,
        binary_path: LPCWSTR,
        load_order_group: LPCWSTR,
        tag_id: *mut DWORD,
        dependencies: LPCWSTR,
        start_name: LPCWSTR,
        display_name: LPCWSTR,
    ) -> BOOL;
    pub fn ChangeServiceConfig2W(
        service: SC_HANDLE,
        info_level: DWORD,
        info: *mut c_void,
    ) -> BOOL;
    pub fn QueryServiceConfig2W(
        service: SC_HANDLE,
        info_level: DWORD,
        buffer: *mut u8,
        size: DWORD,
        needed: *mut DWORD,
    ) -> BOOL;
}

#[link(name = "shell32")]
extern "system" {
    pub fn ShellExecuteW(
        hwnd: HANDLE,
        operation: LPCWSTR,
        file: LPCWSTR,
        params: LPCWSTR,
        dir: LPCWSTR,
        show: i32,
    ) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    pub fn GetSystemMetrics(index: i32) -> i32;
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

const CP_UTF8: u32 = 65001;

/// Convert a Rust string to a NUL-terminated UTF-16 buffer.
pub fn to_wide(s: &str) -> Vec<WCHAR> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return vec![0];
    }
    let needed = unsafe {
        MultiByteToWideChar(CP_UTF8, 0, bytes.as_ptr() as *const c_char, bytes.len() as i32, ptr::null_mut(), 0)
    };
    if needed <= 0 {
        return vec![0];
    }
    let mut buf = vec![0u16; needed as usize + 1];
    let written = unsafe {
        MultiByteToWideChar(
            CP_UTF8,
            0,
            bytes.as_ptr() as *const c_char,
            bytes.len() as i32,
            buf.as_mut_ptr(),
            needed,
        )
    };
    buf.truncate(written.max(0) as usize + 1);
    buf[written as usize] = 0;
    buf
}

/// Convert a NUL-terminated UTF-16 buffer back to a Rust string (lossy).
pub fn from_wide(slice: &[WCHAR]) -> String {
    let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..len])
}

/// Convert a raw UTF-16 pointer (assumed NUL-terminated) to a Rust string.
pub unsafe fn from_wide_ptr(ptr: LPCWSTR) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 && len < 32768 {
        len += 1;
    }
    from_wide(std::slice::from_raw_parts(ptr, len))
}
