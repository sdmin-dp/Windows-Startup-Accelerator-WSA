//! The acceleration engine: builds a staggered launch plan and applies it.

#![cfg(windows)]
#![allow(dead_code)]

use crate::model::{Action, StartupEntry};
use crate::winapi as w;
use std::collections::HashMap;
use std::ptr;
use std::time::{Duration, Instant};

/// Persisted per-entry override chosen by the user in the TUI.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    /// entry id -> desired action override
    pub overrides: HashMap<String, Action>,
}

impl Profile {
    pub fn action_for(&self, entry: &StartupEntry) -> Action {
        self.overrides
            .get(&entry.id)
            .copied()
            .unwrap_or(entry.action)
    }
}

/// One scheduled launch.
#[derive(Debug, Clone)]
pub struct LaunchStep {
    pub entry: StartupEntry,
    pub delay_ms: u32,
    pub action: Action,
}

/// Build a deterministic, low-contention launch plan.
///
/// Entries are sorted by impact (heaviest last) and their delays are spaced so
/// that at most `concurrency` processes are admitted per wave. Protected or
/// kept entries are left to Windows; entries disabled by the user are dropped.
pub fn build_plan(entries: &[StartupEntry], profile: &Profile, concurrency: usize) -> Vec<LaunchStep> {
    let mut candidates: Vec<&StartupEntry> = entries
        .iter()
        .filter(|e| {
            matches!(
                profile.action_for(e),
                Action::Delay | Action::LowPriority
            )
        })
        .collect();

    // Heaviest first so the CPU has time to settle before the next wave.
    candidates.sort_by(|a, b| b.impact.cmp(&a.impact).then_with(|| a.name.cmp(&b.name)));

    let concurrency = concurrency.max(1);
    let mut plan = Vec::with_capacity(candidates.len());
    let mut wave_end = 0u32;

    for (i, e) in candidates.into_iter().enumerate() {
        let action = profile.action_for(e);
        let base = e.suggested_delay_ms.max(200);
        // Each wave is assigned a single start time; entries inside a wave are
        // tight together so the OS scheduler can overlap them efficiently.
        let delay = if i % concurrency == 0 {
            wave_end = wave_end.max(base);
            wave_end
        } else {
            wave_end
        };
        plan.push(LaunchStep {
            entry: e.clone(),
            delay_ms: delay,
            action,
        });
    }
    plan
}

/// Result of applying the plan.
#[derive(Debug, Default, Clone, Copy)]
pub struct ApplyResult {
    pub launched: usize,
    pub failed: usize,
    pub total_delay_ms: u64,
}

/// Execute a plan synchronously (used by `--apply` daemon mode).
pub fn apply_plan(plan: &[LaunchStep], dry_run: bool) -> ApplyResult {
    let mut result = ApplyResult::default();
    let start = Instant::now();
    let mut last_delay = 0u32;

    for step in plan {
        let wait = step.delay_ms.saturating_sub(last_delay);
        last_delay = step.delay_ms;
        if !dry_run && wait > 0 {
            std::thread::sleep(Duration::from_millis(wait as u64));
        }
        if dry_run {
            continue;
        }
        match launch(&step.entry, step.action) {
            Ok(()) => result.launched += 1,
            Err(()) => result.failed += 1,
        }
    }
    result.total_delay_ms = start.elapsed().as_millis() as u64;
    result
}

/// Launch one entry with the requested priority treatment.
pub fn launch(entry: &StartupEntry, action: Action) -> Result<(), ()> {
    let cmd = w::to_wide(&entry.command);
    let mut startup: w::STARTUPINFOW = w::STARTUPINFOW::default();
    startup.cb = std::mem::size_of::<w::STARTUPINFOW>() as w::DWORD;
    startup.dwFlags = w::STARTF_USESHOWWINDOW;
    startup.wShowWindow = w::SW_SHOWMINIMIZED;

    let mut info: w::PROCESS_INFORMATION = w::PROCESS_INFORMATION::default();
    let priority = if action == Action::LowPriority {
        w::BELOW_NORMAL_PRIORITY_CLASS
    } else {
        w::NORMAL_PRIORITY_CLASS
    };

    let ok = unsafe {
        w::CreateProcessW(
            ptr::null(),
            cmd.as_ptr() as w::LPWSTR,
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            priority | w::CREATE_NO_WINDOW,
            ptr::null_mut(),
            ptr::null(),
            &mut startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(());
    }

    if action == Action::LowPriority {
        // Reduce affinity to the first half of cores to leave headroom for the
        // foreground workload during the first seconds after logon.
        unsafe {
            let mut process_mask: usize = 0;
            let mut system_mask: usize = 0;
            if w::GetProcessAffinityMask(info.hProcess, &mut process_mask, &mut system_mask) != 0
                && system_mask > 1
            {
                let half = system_mask >> 1;
                let reduced = if half == 0 { 1 } else { half };
                w::SetProcessAffinityMask(info.hProcess, reduced);
            }
            w::SetPriorityClass(info.hProcess, w::BELOW_NORMAL_PRIORITY_CLASS);
            w::CloseHandle(info.hThread);
            w::CloseHandle(info.hProcess);
        }
    } else {
        unsafe {
            w::CloseHandle(info.hThread);
            w::CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

/// Disable an entry at its source (registry value / service start type).
///
/// Returns a human-readable description of what was done, or an error string.
pub fn disable_entry(entry: &StartupEntry) -> Result<String, String> {
    use crate::model::Source;
    use crate::registry::{RegKey, ScHandle};
    use crate::winapi as w;

    match entry.source {
        Source::HkcuRun | Source::HklmRun | Source::RunOnce => {
            // Registry-backed: rename the value to ".wsa_disabled" suffix so it
            // can be restored, rather than deleting it outright.
            let root = if entry.location.starts_with("HKCU") {
                w::HKEY_CURRENT_USER
            } else {
                w::HKEY_LOCAL_MACHINE
            };
            let subkey = entry.location.split_once(':').map(|(_, k)| k).unwrap_or(&entry.location);
            let key = RegKey::open_with_write(root, subkey)
                .ok_or_else(|| "无法打开注册表键（可能需要管理员权限）".to_string())?;
            let disabled_name = format!("{}.wsa_disabled", entry.name);
            let value = key
                .read_string(&entry.name)
                .ok_or_else(|| "无法读取原值".to_string())?;
            if !key.set_string(&disabled_name, &value) {
                return Err("写入禁用标记失败".to_string());
            }
            if !key.delete_value(&entry.name) {
                return Err("删除原启动值失败".to_string());
            }
            Ok(format!("已禁用注册表项 {}", entry.name))
        }
        Source::Service => {
            let scm = ScHandle::open_scm().ok_or_else(|| "无法打开服务管理器".to_string())?;
            let svc = scm
                .open_service(&entry.location, w::SERVICE_CHANGE_CONFIG)
                .ok_or_else(|| "无法打开服务（可能需要管理员权限）".to_string())?;
            if !svc.set_start_type(w::SERVICE_DISABLED) {
                return Err("修改服务启动类型失败".to_string());
            }
            Ok(format!("已禁用服务 {}", entry.name))
        }
        Source::StartupFolder | Source::ScheduledTask => Err(
            "此类型需手动禁用（请到启动文件夹/任务计划程序处理）".to_string(),
        ),
    }
}

/// Real-time system pulse used by the TUI header.
#[derive(Debug, Clone, Copy)]
pub struct Pulse {
    pub memory_load_pct: u32,
    pub avail_phys_mb: u64,
    pub uptime_minutes: u64,
    pub process_count: u32,
}

pub fn pulse() -> Pulse {
    let mut mem: w::MEMORYSTATUSEX = w::MEMORYSTATUSEX::default();
    unsafe { w::GlobalMemoryStatusEx(&mut mem) };
    Pulse {
        memory_load_pct: mem.dwMemoryLoad,
        avail_phys_mb: mem.ullAvailPhys / (1024 * 1024),
        uptime_minutes: unsafe { w::GetTickCount64() } / 60_000,
        process_count: crate::scanner::running_process_names().len() as u32,
    }
}
