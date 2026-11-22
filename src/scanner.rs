//! Startup entry discovery across all major Windows auto-start locations.

#![cfg(windows)]

use crate::model::{Source, StartupEntry};
use crate::registry::{RegKey, ScHandle};
use crate::winapi as w;
use std::collections::HashSet;
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::ptr;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";

/// Scan every supported auto-start location.
pub fn scan_all() -> Vec<StartupEntry> {
    let mut entries = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    push_all(&mut entries, &mut seen, scan_registry(w::HKEY_CURRENT_USER, RUN_KEY, Source::HkcuRun));
    push_all(&mut entries, &mut seen, scan_registry(w::HKEY_LOCAL_MACHINE, RUN_KEY, Source::HklmRun));
    push_all(&mut entries, &mut seen, scan_registry(w::HKEY_CURRENT_USER, RUN_ONCE_KEY, Source::RunOnce));
    push_all(&mut entries, &mut seen, scan_registry(w::HKEY_LOCAL_MACHINE, RUN_ONCE_KEY, Source::RunOnce));
    push_all(&mut entries, &mut seen, scan_startup_folder());
    push_all(&mut entries, &mut seen, scan_services());

    // Mark which ones are currently running.
    let running = running_process_names();
    for e in &mut entries {
        let exe = e.exe_name().to_lowercase();
        e.running = running.iter().any(|p| p.eq_ignore_ascii_case(&exe));
    }

    entries
}

fn push_all(dst: &mut Vec<StartupEntry>, seen: &mut HashSet<String>, src: Vec<StartupEntry>) {
    for e in src {
        if seen.insert(e.id.clone()) {
            dst.push(e);
        }
    }
}

fn scan_registry(root: w::HKEY, subkey: &str, source: Source) -> Vec<StartupEntry> {
    let key = match RegKey::open(root, subkey) {
        Some(k) => k,
        None => return Vec::new(),
    };
    let key_path = format!(
        "{}:{}",
        if root == w::HKEY_CURRENT_USER { "HKCU" } else { "HKLM" },
        subkey
    );
    key.value_names()
        .into_iter()
        .filter_map(|name| {
            let cmd = key.read_string(&name)?;
            Some(StartupEntry::new(name, cmd, source, key_path.clone()))
        })
        .collect()
}

fn scan_startup_folder() -> Vec<StartupEntry> {
    let mut dirs = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(Path::new(&appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }
    if let Ok(program_data) = std::env::var("PROGRAMDATA") {
        dirs.push(Path::new(&program_data).join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }

    let mut out = Vec::new();
    for dir in dirs {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for item in entries.flatten() {
            let path = item.path();
            let name = item.file_name().to_string_lossy().into_owned();
            if name.ends_with(".lnk") || name.ends_with(".exe") || name.ends_with(".bat") {
                let cmd = resolve_shortcut_target(&path).unwrap_or_else(|| path.to_string_lossy().into_owned());
                let loc = dir.to_string_lossy().into_owned();
                out.push(StartupEntry::new(
                    name.trim_end_matches(".lnk").to_string(),
                    cmd,
                    Source::StartupFolder,
                    loc,
                ));
            }
        }
    }
    out
}

/// Best-effort resolution of a .lnk target without the Shell COM interfaces.
/// Reads the raw Shell Link binary and extracts the LocalBasePath string.
fn resolve_shortcut_target(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;
    // LinkInfo structure begins at offset 0x4C (76) in a typical ShellLink.
    // Search for a UTF-16 or ANSI string containing a drive letter as a cheap
    // heuristic; this is intentionally best-effort.
    if data.len() < 0x4C {
        return None;
    }
    // Try wide strings first.
    let wide: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    for (i, w) in wide.windows(4).enumerate() {
        if w[0] >= b'C' as u16 && w[0] <= b'Z' as u16 && w[1] == b':' as u16
            && w[2] == b'\\' as u16 && w[3] != 0 && w[3] >= 0x20 {
            let mut end = i;
            while end < wide.len() && wide[end] != 0 && wide[end] >= 0x20 {
                end += 1;
            }
            // Require a plausible path with at least one subdirectory segment.
            let candidate = w::from_wide(&wide[i..end]);
            if candidate.matches('\\').count() >= 2 && candidate.len() > 6 {
                return Some(candidate);
            }
        }
    }
    // Try ANSI strings.
    for i in 0..data.len().saturating_sub(3) {
        if data[i].is_ascii_uppercase() && data[i + 1] == b':' && data[i + 2] == b'\\' {
            let mut end = i;
            while end < data.len() && data[end] >= 0x20 {
                end += 1;
            }
            if end > i {
                return Some(String::from_utf8_lossy(&data[i..end]).into_owned());
            }
        }
    }
    None
}

fn scan_services() -> Vec<StartupEntry> {
    let scm = match ScHandle::open_scm() {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut needed: w::DWORD = 0;
    let mut returned: w::DWORD = 0;
    unsafe {
        w::EnumServicesStatusExW(
            scm.0,
            w::SC_STATUS_PROCESS_INFO,
            w::SERVICE_TYPE_ALL,
            w::SERVICE_STATE_ALL,
            ptr::null_mut(),
            0,
            &mut needed,
            &mut returned,
            ptr::null_mut(),
            ptr::null(),
        );
    }
    if needed == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; needed as usize + 4096];
    let ok = unsafe {
        w::EnumServicesStatusExW(
            scm.0,
            w::SC_STATUS_PROCESS_INFO,
            w::SERVICE_TYPE_ALL,
            w::SERVICE_STATE_ALL,
            buf.as_mut_ptr(),
            buf.len() as w::DWORD,
            &mut needed,
            &mut returned,
            ptr::null_mut(),
            ptr::null(),
        )
    };
    if ok == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let item_size = std::mem::size_of::<w::ENUM_SERVICE_STATUS_PROCESSW>();
    for i in 0..returned as usize {
        let item = unsafe {
            ptr::read_unaligned(buf.as_ptr().add(i * item_size) as *const w::ENUM_SERVICE_STATUS_PROCESSW)
        };
        let display = unsafe { w::from_wide_ptr(item.lpDisplayName) };
        let service_name = unsafe { w::from_wide_ptr(item.lpServiceName) };

        let config = scm
            .open_service(&service_name, w::SERVICE_QUERY_CONFIG)
            .and_then(|s| s.query_config());
        // Only auto-start services belong in the startup list.
        if config.map(|c| c.dwStartType != w::SERVICE_AUTO_START).unwrap_or(true) {
            continue;
        }
        let command = config
            .and_then(|cfg| {
                if cfg.lpBinaryPathName.is_null() {
                    None
                } else {
                    Some(unsafe { w::from_wide_ptr(cfg.lpBinaryPathName) })
                }
            })
            .unwrap_or_default();

        // Drop pure OS services so the TUI focuses on third-party startup
        // impact. Windows-owned binaries live under System32 / drivers.
        let lower = command.to_lowercase();
        let windows_owned = lower.contains("\\windows\\system32\\")
            || lower.contains("\\windows\\syswow64\\")
            || lower.contains("\\systemroot\\system32\\")
            || lower.starts_with("system32\\");
        if windows_owned {
            continue;
        }

        out.push(StartupEntry::new(
            if display.is_empty() { service_name.clone() } else { display },
            command,
            Source::Service,
            service_name,
        ));
    }
    out
}

/// Return a set of lowercase executable names currently in the process list.
pub fn running_process_names() -> HashSet<String> {
    let mut set = HashSet::new();
    let snap = unsafe { w::CreateToolhelp32Snapshot(w::TH32CS_SNAPPROCESS, 0) };
    if snap.is_null() || snap == usize::MAX as *mut c_void {
        return set;
    }
    let mut entry: w::PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<w::PROCESSENTRY32W>() as w::DWORD;
    let mut ok = unsafe { w::Process32FirstW(snap, &mut entry) };
    while ok != 0 {
        set.insert(w::from_wide(&entry.szExeFile).to_lowercase());
        ok = unsafe { w::Process32NextW(snap, &mut entry) };
    }
    unsafe { w::CloseHandle(snap) };
    set
}
