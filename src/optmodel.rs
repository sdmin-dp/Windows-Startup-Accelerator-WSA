//! Data model for system boot optimizations.

#![allow(dead_code)]

use std::fmt;

/// Broad category of an optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptCategory {
    /// Delay non-critical Windows services (delayed auto-start).
    ServiceDelay,
    /// Registry tweaks that change boot / logon behaviour.
    RegistryTweak,
    /// Delay third-party auto-start programs (staggered launch).
    StartupDelay,
    /// Measure and report boot time.
    Measurement,
    /// Other miscellaneous speed-ups.
    Other,
}

impl OptCategory {
    pub fn label(self) -> &'static str {
        match self {
            OptCategory::ServiceDelay => "服务延迟",
            OptCategory::RegistryTweak => "系统设置",
            OptCategory::StartupDelay => "启动程序",
            OptCategory::Measurement => "耗时测量",
            OptCategory::Other => "其他优化",
        }
    }
}

/// Risk level of applying an optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Microsoft-supported, fully reversible, no user-visible change.
    Safe,
    /// Reversible, minor behaviour change (e.g. animation off).
    Low,
    /// Changes functionality some users rely on; read the description.
    Moderate,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Risk::Safe => "安全",
            Risk::Low => "低",
            Risk::Moderate => "中",
        }
    }
}

/// Current applied state of an optimization on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptState {
    /// Already configured as recommended.
    Optimized,
    /// Still at the default / non-optimized value.
    Default,
    /// Could not be determined (permission denied, missing key, etc.).
    Unknown,
}

impl OptState {
    pub fn label(self) -> &'static str {
        match self {
            OptState::Optimized => "已优化",
            OptState::Default => "默认",
            OptState::Unknown => "未知",
        }
    }
}

/// What kind of underlying change an optimization performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptTarget {
    /// A service -> delayed auto-start.
    Service { name: String },
    /// A registry value. `optimal` is the target DWORD; `default` is stock.
    Registry {
        root: String,
        key: String,
        value: String,
        optimal: u32,
        default: u32,
    },
    /// Staggered launch of all high-impact startup programs (global switch).
    StaggeredLaunch,
    /// Boot-time measurement feature (global switch).
    Measurement,
    /// Custom action with a description (informational only).
    Custom { detail: String },
}

/// A single optimization the user can enable.
#[derive(Debug, Clone)]
pub struct Optimization {
    pub id: &'static str,
    pub name: &'static str,
    pub category: OptCategory,
    pub risk: Risk,
    /// Estimated boot-time saving in seconds (rough).
    pub gain_sec: f32,
    pub description: &'static str,
    pub target: OptTarget,
    /// Whether the user wants WSA to apply this (persisted setting).
    pub enabled: bool,
    /// Detected current state.
    pub state: OptState,
    /// Needs elevation to apply.
    pub needs_admin: bool,
}

impl fmt::Display for Optimization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:<28} {:<10} {:<6} {:>4.1}s  {}",
            self.name,
            self.category.label(),
            self.risk.label(),
            self.gain_sec,
            self.state.label()
        )
    }
}

/// A saved change, used to restore the original value.
#[derive(Debug, Clone)]
pub struct Backup {
    pub opt_id: String,
    pub target: OptTarget,
    /// Serialized original value for restoration.
    pub original: String,
}
