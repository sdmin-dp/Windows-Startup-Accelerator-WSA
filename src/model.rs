//! Core data model and startup-impact heuristics.
//!
//! This module is deliberately free of Win32 calls so the scoring logic can
//! be reasoned about (and unit-tested) independently of the OS layer.

#![allow(dead_code)]

use std::fmt;
use std::path::Path;

/// Where an auto-start entry is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// HKCU\Software\Microsoft\Windows\CurrentVersion\Run
    HkcuRun,
    /// HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run
    HklmRun,
    /// HKCU/HKLM ...\RunOnce
    RunOnce,
    /// The per-user Shell:startup folder
    StartupFolder,
    /// Windows service (SCM)
    Service,
    /// Task Scheduler task
    ScheduledTask,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::HkcuRun => "HKCU Run",
            Source::HklmRun => "HKLM Run",
            Source::RunOnce => "RunOnce",
            Source::StartupFolder => "Startup",
            Source::Service => "Service",
            Source::ScheduledTask => "Task",
        }
    }
}

/// How WSA proposes to neutralise an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Keep the entry exactly as-is.
    Keep,
    /// Disable by renaming the registry value / renaming the .lnk to .disabled.
    Disable,
    /// Move the auto-start from "at logon" to a delayed launch by WSA.
    Delay,
    /// Delay and launch at a lower CPU priority with reduced affinity.
    LowPriority,
    /// Not touchable without elevated rights / unsupported source.
    Manual,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Keep => "保持",
            Action::Disable => "禁用",
            Action::Delay => "延迟",
            Action::LowPriority => "低优先级",
            Action::Manual => "手动",
        }
    }
}

/// Coarse category used for recommendation and for the safety guardrails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Essential,
    Driver,
    Security,
    Utility,
    Communication,
    Media,
    Developer,
    Gaming,
    Unknown,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Essential => "系统核心",
            Category::Driver => "驱动/硬件",
            Category::Security => "安全软件",
            Category::Utility => "实用工具",
            Category::Communication => "通讯",
            Category::Media => "影音",
            Category::Developer => "开发工具",
            Category::Gaming => "游戏/平台",
            Category::Unknown => "其他",
        }
    }
}

/// A single auto-start entry discovered on the system.
#[derive(Debug, Clone)]
pub struct StartupEntry {
    /// Stable identity: source + location + name.
    pub id: String,
    pub name: String,
    pub command: String,
    pub source: Source,
    pub category: Category,
    /// 0..=100, higher means larger boot-time impact.
    pub impact: u8,
    /// Estimated delay in seconds from the recommendation engine.
    pub suggested_delay_ms: u32,
    pub action: Action,
    /// True for entries WSA refuses to disable automatically.
    pub protected: bool,
    /// Whether the corresponding exe currently appears in the process list.
    pub running: bool,
    /// Extra context: registry key path / service name / task path.
    pub location: String,
}

impl StartupEntry {
    pub fn new(
        name: String,
        command: String,
        source: Source,
        location: String,
    ) -> Self {
        let category = categorize(&name, &command);
        let protected = is_protected(&name, &command, category);
        let (impact, delay) = estimate_impact(&name, &command, source, category);
        let action = recommend_action(category, protected, impact);
        let id = format!("{}|{}|{}", source.label(), location, name);
        Self {
            id,
            name,
            command,
            source,
            category,
            impact,
            suggested_delay_ms: delay,
            action,
            protected,
            running: false,
            location,
        }
    }

    pub fn exe_name(&self) -> String {
        let trimmed = self.command.trim().trim_matches('"');
        let first = trimmed
            .split_whitespace()
            .next()
            .unwrap_or(trimmed);
        Path::new(first)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(first)
            .to_string()
    }
}

impl fmt::Display for StartupEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<28} {:<10} {:>3}  {}", self.name, self.source.label(), self.impact, self.command)
    }
}

/// Keyword based categorisation. Ordered from most specific to least specific.
pub fn categorize(name: &str, command: &str) -> Category {
    let hay = format!("{} {}", name, command).to_lowercase();
    let table: &[(&[&str], Category)] = &[
        (&["nvidia", "amd", "radeon", "intel", "realtek", "logitech", "synaptics", "hidfind", "elan"], Category::Driver),
        (&["defender", "msmpeng", "security", "antivirus", "avast", "avg", "kaspersky", "360", "火绒", "huorong", "bitdefender", "nod32", "eset", "trojan", "malware"], Category::Security),
        (&["docker", "git", "node", "rust", "cargo", "vscode", "code.exe", "jetbrains", "idea", "android", "sdk", "wsl", "powershell", "terminal", "python"], Category::Developer),
        (&["steam", "epic", "riot", "battle.net", "discord", "game", "xbox", "gaming", "ubisoft", "origin"], Category::Gaming),
        (&["wechat", "qq", "telegram", "slack", "teams", "skype", "dingtalk", "feishu", "outlook", "微信", "企业微信"], Category::Communication),
        (&["spotify", "itunes", "potplayer", "vlc", "netease", "网易云", "music", "media", "stream"], Category::Media),
        (&["onedrive", "dropbox", "google drive", "backup", "synctray", "clash", "v2ray", "shadowrocket", "fiddler", "notion", "obsidian"], Category::Utility),
        (&["ctfmon", "runtimebroker", "shell", "explorer", "sihost", "audiodg", "winlogon", "lsass", "fontdrvhost", "spoolsv", "searchindexer", "windows", "microsoft"], Category::Essential),
    ];
    for (keys, cat) in table {
        if keys.iter().any(|k| hay.contains(k)) {
            return *cat;
        }
    }
    Category::Unknown
}

/// Entries WSA will never disable without explicit user confirmation.
pub fn is_protected(name: &str, command: &str, category: Category) -> bool {
    let hay = format!("{} {}", name, command).to_lowercase();
    if matches!(category, Category::Essential | Category::Driver | Category::Security) {
        return true;
    }
    const CRITICAL: &[&str] = &[
        "ctfmon", "securityhealth", "msmpeng", "nvidia", "amd", "intel",
        "realtek", "defender", "bitlocker", "lvm",
    ];
    CRITICAL.iter().any(|k| hay.contains(k))
}

/// Estimate boot impact and a corresponding delay budget (milliseconds).
pub fn estimate_impact(
    name: &str,
    command: &str,
    source: Source,
    category: Category,
) -> (u8, u32) {
    let hay = format!("{} {}", name, command).to_lowercase();
    let mut score: i32 = 20;
    let heavy: &[(&str, i32)] = &[
        ("updater", 25), ("update", 18), ("cloud", 15), ("sync", 18),
        ("antivirus", 30), ("security", 20), ("node", 12), ("electron", 20),
        ("java", 15), ("docker", 30), ("steam", 22), ("discord", 18),
        ("office", 15), ("adobe", 20), ("chrome", 12), ("msedge", 10),
        ("onedrive", 16), ("dropbox", 16), ("game", 10), ("helper", 8),
    ];
    for (key, weight) in heavy {
        if hay.contains(key) {
            score += weight;
        }
    }
    if source == Source::Service {
        score += 10;
    }
    if source == Source::ScheduledTask {
        score += 5;
    }
    if matches!(category, Category::Utility | Category::Media | Category::Gaming) {
        score += 8;
    }
    if matches!(category, Category::Essential | Category::Driver) {
        score -= 10;
    }
    let score = score.clamp(1, 99) as u8;
    // Heavier entries get pushed further back, capped at 15 s.
    let delay = ((score as u32) * 160).min(15_000);
    (score, delay)
}

pub fn recommend_action(category: Category, protected: bool, impact: u8) -> Action {
    if protected {
        return if matches!(category, Category::Essential | Category::Driver) {
            Action::Keep
        } else {
            Action::Manual
        };
    }
    if impact >= 65 {
        Action::Disable
    } else if impact >= 40 {
        Action::LowPriority
    } else {
        Action::Delay
    }
}
