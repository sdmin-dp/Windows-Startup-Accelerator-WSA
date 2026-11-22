//! Settings persistence, backup management, and the logon daemon.

#![cfg(windows)]

use crate::optmodel::Backup;
use std::fs;
use std::path::PathBuf;

fn wsa_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("wsa")
}

fn settings_path() -> PathBuf { wsa_dir().join("settings.tsv") }
fn backup_path() -> PathBuf { wsa_dir().join("backups.tsv") }
fn bootlog_path() -> PathBuf { wsa_dir().join("boot-times.tsv") }

// ---------------------------------------------------------------------------
// Settings: opt id -> enabled (0/1)
// ---------------------------------------------------------------------------

pub fn load_enabled() -> std::collections::HashMap<String, bool> {
    let mut map = std::collections::HashMap::new();
    if let Ok(text) = fs::read_to_string(settings_path()) {
        for line in text.lines() {
            let mut p = line.splitn(2, '\t');
            if let (Some(id), Some(v)) = (p.next(), p.next()) {
                map.insert(id.to_string(), v == "1");
            }
        }
    }
    map
}

pub fn save_enabled(map: &std::collections::HashMap<String, bool>) -> std::io::Result<()> {
    fs::create_dir_all(wsa_dir())?;
    let mut out = String::new();
    for (id, on) in map {
        out.push_str(&format!("{}\t{}\n", id, if *on { 1 } else { 0 }));
    }
    fs::write(settings_path(), out)
}

// ---------------------------------------------------------------------------
// Backups (append-only log of applied changes)
// ---------------------------------------------------------------------------

pub fn load_backups() -> Vec<Backup> {
    let mut out = Vec::new();
    let text = match fs::read_to_string(backup_path()) { Ok(t) => t, Err(_) => return out };
    for line in text.lines() {
        // Format: opt_id \t serialized-target \t serialized-original
        let mut p = line.splitn(3, '\t');
        let (Some(id), Some(target), Some(orig)) = (p.next(), p.next(), p.next()) else { continue };
        if let Some(t) = parse_target(target) {
            out.push(Backup { opt_id: id.to_string(), target: t, original: orig.to_string() });
        }
    }
    out
}

pub fn append_backups(new: &[Backup]) -> std::io::Result<()> {
    if new.is_empty() { return Ok(()); }
    fs::create_dir_all(wsa_dir())?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(backup_path())?;
    for b in new {
        use std::io::Write;
        writeln!(file, "{}\t{}\t{}", b.opt_id, serialize_target(&b.target), b.original)?;
    }
    Ok(())
}

pub fn clear_backups() -> std::io::Result<()> {
    if backup_path().exists() { fs::remove_file(backup_path())?; }
    Ok(())
}

fn serialize_target(t: &crate::optmodel::OptTarget) -> String {
    use crate::optmodel::OptTarget::*;
    match t {
        Service { name } => format!("svc\t{}", name),
        Registry { root, key, value, optimal, default } =>
            format!("reg\t{}\t{}\t{}\t{}\t{}", root, key, value, optimal, default),
        StaggeredLaunch => "feature\tstagger".to_string(),
        Measurement => "feature\tmeasurement".to_string(),
        Custom { detail } => format!("custom\t{}", detail),
    }
}

fn parse_target(s: &str) -> Option<crate::optmodel::OptTarget> {
    use crate::optmodel::OptTarget::*;
    let mut p = s.split('\t');
    match p.next()? {
        "svc" => Some(Service { name: p.next()?.to_string() }),
        "reg" => Some(Registry {
            root: match p.next()? { "HKLM" => "HKLM".to_string(), _ => "HKCU".to_string() },
            key: p.next()?.to_string(),
            value: p.next()?.to_string(),
            optimal: p.next()?.parse().ok()?,
            default: p.next()?.parse().ok()?,
        }),
        "feature" => match p.next()? {
            "stagger" => Some(StaggeredLaunch),
            "measurement" => Some(Measurement),
            _ => None,
        },
        "custom" => Some(Custom { detail: p.next()?.to_string() }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Boot time measurement
// ---------------------------------------------------------------------------

pub fn measurement_enabled() -> bool {
    load_enabled().get("measurement").copied().unwrap_or(false)
}

/// Record a boot-time sample (called by the daemon at logon).
pub fn record_boot_time(seconds: f32) {
    if !measurement_enabled() { return; }
    let _ = fs::create_dir_all(wsa_dir());
    use std::io::Write;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(bootlog_path()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs();
    let _ = writeln!(file, "{}\t{:.2}", now, seconds);
}

/// Return recent boot-time samples (newest last).
pub fn boot_times() -> Vec<(u64, f32)> {
    let text = match fs::read_to_string(bootlog_path()) { Ok(t) => t, Err(_) => return vec![] };
    text.lines().filter_map(|line| {
        let mut p = line.splitn(2, '\t');
        Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?))
    }).collect()
}

// ---------------------------------------------------------------------------
// Autostart task
// ---------------------------------------------------------------------------

pub fn set_autostart(enable: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let task_name = "WSA Startup Accelerator";
    let action = if enable { "/Create" } else { "/Delete" };
    let mut cmd = std::process::Command::new("schtasks.exe");
    cmd.arg(action).arg("/TN").arg(task_name).arg("/F");
    if enable {
        cmd.arg("/TR")
            .arg(format!("\"{}\" --daemon", exe.display()))
            .arg("/SC").arg("ONLOGON")
            .arg("/RL").arg("LIMITED");
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("schtasks 退出码: {}", status)) }
}

/// Logon daemon entry: run staggered launch if enabled, record boot time.
pub fn run_daemon() {
    let enabled = load_enabled();
    let stagger_on = enabled.get("stagger").copied().unwrap_or(true);

    let start = std::time::Instant::now();
    if stagger_on {
        let entries = crate::scanner::scan_all();
        // Reuse the old engine only for staggered launch of high-impact entries.
        let plan = crate::engine::build_plan(&entries, &crate::engine::Profile::default(), 3);
        let _ = crate::engine::apply_plan(&plan, false);
    }
    let elapsed = start.elapsed().as_secs_f32();
    record_boot_time(elapsed);
}
