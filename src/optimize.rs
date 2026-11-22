//! Boot optimization catalog and apply/restore engine.
//!
//! Everything here is reversible. Each change records a `Backup` so the user
//! can restore stock behaviour from the TUI at any time.

use crate::optmodel::{Backup, OptCategory, OptState, OptTarget, Optimization, Risk};
use crate::registry::{RegKey, ScHandle};
use crate::winapi as w;

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// Build the full optimization catalog with default (user = off) settings.
pub fn catalog() -> Vec<Optimization> {
    use OptCategory::*;
    use OptTarget::*;
    use Risk::*;

    vec![
        // ---- Delayed auto-start services (Microsoft-supported) ----
        svc("wsearch", "Windows Search 索引", Safe, 2.5,
            "搜索索引在后台重建索引，占用大量磁盘 I/O。改为延迟自动启动，登录后再运行。", "WSearch", true),
        svc("sysmain", "SysMain (Superfetch)", Safe, 2.0,
            "预读服务在开机时预加载应用。现代 SSD 上收益有限，延迟启动可减少前 30 秒争抢。", "SysMain", true),
        svc("spooler", "打印后台处理", Safe, 1.0,
            "打印服务无需在登录瞬间就绪。延迟启动，需要打印时系统会按需唤醒。", "Spooler", true),
        svc("bits", "后台智能传输 (BITS)", Safe, 1.5,
            "后台下载服务延迟启动，避免与开机磁盘读写争抢。", "BITS", true),
        svc("wuauserv", "Windows Update 服务", Safe, 2.0,
            "更新服务延迟启动，登录后再检查更新。", "wuauserv", true),
        svc("dosvc", "传递优化", Safe, 1.0,
            "Windows 更新 P2P 分发服务，延迟启动减少网络与磁盘争抢。", "DoSvc", true),
        svc("dps", "诊断策略服务", Safe, 1.0,
            "后台诊断服务对登录无影响，延迟启动。", "DPS", true),
        svc("wersvc", "Windows 错误报告", Safe, 0.8,
            "错误报告服务延迟启动，几乎无感。", "WerSvc", true),
        svc("iphlpsvc", "IP Helper (IPv6 隧道)", Safe, 0.8,
            "大多数家庭网络用不到 IPv6 隧道，延迟启动。", "iphlpsvc", true),
        svc("wpnservice", "Windows 推送通知", Safe, 1.0,
            "推送通知服务延迟启动，稍后仍会正常运行。", "WpnService", true),
        svc("cdpsvc", "连接设备平台服务", Safe, 0.8,
            "外设发现服务延迟启动，不影响已配对设备。", "CDPSvc", true),
        svc("fax", "传真服务", Low, 0.5,
            "几乎无人使用传真，延迟启动。如确实使用请勿勾选。", "Fax", true),
        svc("xblauth", "Xbox Live 身份验证", Moderate, 0.8,
            "不玩游戏时无需即时启动 Xbox 认证服务；玩游戏时系统会按需拉起。", "XblAuthManager", true),

        // ---- Registry tweaks ----
        reg("visualfx", "最佳性能视觉效果", Low, 0.8,
            "将视觉效果设为“最佳性能”，减少登录时的动画合成开销（可在外观设置中恢复）。",
            "HKCU", r"Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects", "VisualFXSetting", 2, 0, false),
        reg("minanimate", "关闭窗口动画", Safe, 0.5,
            "关闭窗口最小化/最大化动画，界面响应更干脆。",
            "HKCU", r"Control Panel\Desktop\WindowMetrics", "MinAnimate", 0, 1, false),
        reg("waittokill", "缩短服务终止超时", Low, 0.3,
            "将 WaitToKillServiceTimeout 从 5000ms 降至 2000ms，主要加快关机而非开机。",
            "HKLM", r"SYSTEM\CurrentControlSet\Control", "WaitToKillServiceTimeout", 2000, 5000, true),
        reg("autoendtasks", "自动结束无响应任务", Low, 0.3,
            "关机时自动结束无响应程序，减少卡顿等待。",
            "HKCU", r"Control Panel\Desktop", "AutoEndTasks", 1, 0, false),
        reg("menushowdelay", "加快菜单显示", Safe, 0.2,
            "将菜单弹出延迟从 400ms 降至 50ms，提升操作跟手度。",
            "HKCU", r"Control Panel\Desktop", "MenuShowDelay", 50, 400, false),

        // ---- WSA features ----
        Optimization {
            id: "stagger", name: "启动程序错峰延迟", category: StartupDelay,
            risk: Safe, gain_sec: 3.0,
            description: "将高影响的自启动程序分批延迟启动（默认每波 3 个），避免登录瞬间资源踩踏。守护进程在后台自动执行。",
            target: StaggeredLaunch, enabled: true, state: OptState::Unknown, needs_admin: false,
        },
        Optimization {
            id: "measurement", name: "开机耗时测量", category: OptCategory::Measurement,
            risk: Safe, gain_sec: 0.0,
            description: "记录每次登录的 startup 耗时并在 TUI 展示优化前后对比。默认关闭，开启后需要重启一次才能看到数据。",
            target: OptTarget::Measurement, enabled: false, state: OptState::Unknown, needs_admin: false,
        },
        Optimization {
            id: "prefetch", name: "预读优化 (Prefetch)", category: OptCategory::Other,
            risk: Low, gain_sec: 1.0,
            description: "确保 Prefetcher 完全开启（EnablePrefetcher=3）。现代 Windows 由 SysMain 管理，此项用于纠正被第三方工具改坏的情况。",
            target: OptTarget::Registry {
                root: "HKLM".to_string(),
                key: r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters".to_string(),
                value: "EnablePrefetcher".to_string(), optimal: 3, default: 3,
            },
            enabled: false, state: OptState::Unknown, needs_admin: true,
        },
    ]
}

fn svc(id: &'static str, name: &'static str, risk: Risk, gain: f32, desc: &'static str,
       service: &'static str, admin: bool) -> Optimization {
    Optimization {
        id, name, category: OptCategory::ServiceDelay, risk, gain_sec: gain,
        description: desc, target: OptTarget::Service { name: service.to_string() },
        enabled: true, state: OptState::Unknown, needs_admin: admin,
    }
}

fn reg(id: &'static str, name: &'static str, risk: Risk, gain: f32, desc: &'static str,
       root: &'static str, key: &'static str, value: &'static str,
       optimal: u32, default: u32, admin: bool) -> Optimization {
    Optimization {
        id, name, category: OptCategory::RegistryTweak, risk, gain_sec: gain,
        description: desc,
        target: OptTarget::Registry {
            root: root.to_string(), key: key.to_string(), value: value.to_string(),
            optimal, default,
        },
        enabled: false, state: OptState::Unknown, needs_admin: admin,
    }
}

// ---------------------------------------------------------------------------
// Detect current state
// ---------------------------------------------------------------------------

/// Refresh the `state` field of each optimization based on the live system.
pub fn detect_all(catalog: &mut [Optimization]) {
    for opt in catalog.iter_mut() {
        opt.state = detect_one(&opt.target);
    }
}

fn detect_one(target: &OptTarget) -> OptState {
    match target {
        OptTarget::Service { name } => {
            let scm = match ScHandle::open_scm() { Some(s) => s, None => return OptState::Unknown };
            let svc = match scm.open_service(name, w::SERVICE_QUERY_CONFIG) {
                Some(s) => s,
                None => return OptState::Unknown,
            };
            let cfg = match svc.query_config() { Some(c) => c, None => return OptState::Unknown };
            // Only auto-start services can be delayed.
            if cfg.dwStartType != w::SERVICE_AUTO_START {
                return OptState::Default;
            }
            if svc.is_delayed_auto_start() {
                OptState::Optimized
            } else {
                OptState::Default
            }
        }
        OptTarget::Registry { root, key, value, optimal, .. } => {
            let hroot = root_key(root);
            let k = match RegKey::open(hroot, key) { Some(k) => k, None => return OptState::Unknown };
            match k.read_dword(value) {
                Some(v) if v == *optimal => OptState::Optimized,
                Some(_) => OptState::Default,
                None => {
                    // REG_SZ values (MinAnimate, MenuShowDelay) are strings.
                    match k.read_string(value) {
                        Some(s) if s.parse::<u32>().ok() == Some(*optimal) => OptState::Optimized,
                        Some(_) => OptState::Default,
                        None => OptState::Unknown,
                    }
                }
            }
        }
        OptTarget::StaggeredLaunch | OptTarget::Measurement => {
            // State loaded from settings by the caller.
            OptState::Unknown
        }
        OptTarget::Custom { .. } => OptState::Unknown,
    }
}

fn root_key(name: &str) -> w::HKEY {
    if name == "HKLM" { w::HKEY_LOCAL_MACHINE } else { w::HKEY_CURRENT_USER }
}

// ---------------------------------------------------------------------------
// Apply / restore
// ---------------------------------------------------------------------------

/// Apply one optimization. Returns a backup describing the original value.
pub fn apply_one(opt: &Optimization) -> Result<Backup, String> {
    match &opt.target {
        OptTarget::Service { name } => apply_service(name),
        OptTarget::Registry { root, key, value, optimal, .. } => {
            apply_registry(root, key, value, *optimal)
        }
        OptTarget::StaggeredLaunch | OptTarget::Measurement | OptTarget::Custom { .. } => {
            // Feature switches are handled via settings persistence.
            Ok(Backup {
                opt_id: opt.id.to_string(),
                target: opt.target.clone(),
                original: "feature".to_string(),
            })
        }
    }
}

fn apply_service(name: &str) -> Result<Backup, String> {
    let scm = ScHandle::open_scm().ok_or_else(|| "无法打开服务管理器（权限不足）".to_string())?;
    let svc = scm
        .open_service(name, w::SERVICE_CHANGE_CONFIG | w::SERVICE_QUERY_CONFIG)
        .ok_or_else(|| format!("无法打开服务 {}（权限不足，需管理员）", name))?;
    let cfg = svc.query_config().ok_or_else(|| "无法查询服务配置".to_string())?;

    let was_delayed = svc.is_delayed_auto_start();
    let original_start = cfg.dwStartType;

    // Delayed auto-start requires the service to be AUTO_START.
    if cfg.dwStartType != w::SERVICE_AUTO_START {
        if !svc.set_start_type(w::SERVICE_AUTO_START) {
            return Err(format!("无法将服务 {} 设为自动启动", name));
        }
    }
    if !svc.set_delayed_auto_start(true) {
        return Err(format!("无法将服务 {} 设为延迟启动（权限不足，需管理员）", name));
    }
    Ok(Backup {
        opt_id: format!("svc:{}", name),
        target: OptTarget::Service { name: name.to_string() },
        original: format!("{}:{}", original_start, if was_delayed { 1 } else { 0 }),
    })
}

fn apply_registry(root: &String, key: &String, value: &String, optimal: u32) -> Result<Backup, String> {
    let k = RegKey::open_with_write(root_key(root), key)
        .ok_or_else(|| format!("无法打开注册表键 {}（权限不足，需管理员）", key))?;
    let original = k
        .read_dword(value)
        .or_else(|| k.read_string(value).and_then(|s| s.parse::<u32>().ok()))
        .ok_or_else(|| format!("无法读取原值 {}", value))?;

    //数值类型需与原始类型一致：REG_SZ 写字符串，其余写 DWORD。
    let is_string = k.read_dword(value).is_none();
    let ok = if is_string {
        k.set_string(value, &optimal.to_string())
    } else {
        k.write_dword(value, optimal)
    };
    if !ok {
        return Err(format!("无法写入 {}", value));
    }
    Ok(Backup {
        opt_id: format!("reg:{}:{}", key, value),
        target: OptTarget::Registry {
            root: root.clone(),
            key: key.clone(),
            value: value.clone(),
            optimal, default: original,
        },
        original: format!("{}:{}", original, if is_string { 1 } else { 0 }),
    })
}

/// Restore a previously applied change.
pub fn restore(backup: &Backup) -> Result<(), String> {
    match &backup.target {
        OptTarget::Service { name } => {
            let scm = ScHandle::open_scm().ok_or_else(|| "无法打开服务管理器".to_string())?;
            let svc = scm
                .open_service(name, w::SERVICE_CHANGE_CONFIG)
                .ok_or_else(|| format!("无法打开服务 {}", name))?;
            let mut parts = backup.original.split(':');
            let orig_start: w::DWORD = parts.next().and_then(|s| s.parse().ok()).unwrap_or(w::SERVICE_AUTO_START);
            let orig_delayed: w::DWORD = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            if !svc.set_delayed_auto_start(orig_delayed != 0) {
                return Err(format!("无法恢复服务 {} 的延迟设置", name));
            }
            if !svc.set_start_type(orig_start) {
                return Err(format!("无法恢复服务 {} 的启动类型", name));
            }
            Ok(())
        }
        OptTarget::Registry { root, key, value, .. } => {
            let k = RegKey::open_with_write(root_key(root), key)
                .ok_or_else(|| format!("无法打开注册表键 {}", key))?;
            let mut parts = backup.original.split(':');
            let orig: u32 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| "备份值无效".to_string())?;
            let is_string: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let ok = if is_string == 1 {
                k.set_string(value, &orig.to_string())
            } else {
                k.write_dword(value, orig)
            };
            if !ok { return Err(format!("无法恢复 {}", value)); }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Outcome of applying a batch of optimizations.
pub struct ApplyOutcome {
    pub applied: usize,
    pub failed: usize,
    /// Subset of failures caused by insufficient privileges.
    pub needs_admin: usize,
    pub gain: f32,
    /// Backups of every change, to be persisted for later restore.
    pub backups: Vec<Backup>,
}

/// Apply all enabled optimizations, collecting backups for restoration.
pub fn apply_enabled(catalog: &[Optimization]) -> ApplyOutcome {
    let mut outcome = ApplyOutcome {
        applied: 0, failed: 0, needs_admin: 0, gain: 0.0, backups: Vec::new(),
    };
    for opt in catalog.iter().filter(|o| o.enabled) {
        if opt.state == OptState::Optimized {
            continue;
        }
        match apply_one(opt) {
            Ok(backup) => {
                // Feature switches have no reversible system state.
                if !matches!(opt.target, OptTarget::StaggeredLaunch | OptTarget::Measurement | OptTarget::Custom { .. }) {
                    outcome.backups.push(backup);
                }
                outcome.applied += 1;
                outcome.gain += opt.gain_sec;
            }
            Err(e) => {
                if e.contains("权限") || e.contains("管理员") || e.contains("denied") {
                    outcome.needs_admin += 1;
                }
                outcome.failed += 1;
            }
        }
    }
    outcome
}
