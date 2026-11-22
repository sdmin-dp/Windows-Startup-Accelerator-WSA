//! Interactive TUI — WSA settings panel for boot optimizations.

#![cfg(windows)]

use crate::console::{ConsoleGuard, InputEvent};
use crate::daemon;
use crate::engine::Pulse;
use crate::frame::{pad_left, pad_to};
use crate::optmodel::{OptState, Optimization, Risk};
use crate::optimize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::time::Instant;

// Virtual key codes
const VK_UP: u16 = 0x26;
const VK_DOWN: u16 = 0x28;
const VK_PGUP: u16 = 0x21;
const VK_PGDN: u16 = 0x22;
const VK_HOME: u16 = 0x24;
const VK_END: u16 = 0x23;
const VK_ENTER: u16 = 0x0D;
const VK_ESC: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_TAB: u16 = 0x09;

// Colors (ANSI 256)
const C_RESET: &str = "\x1b[0m";
const C_BOLD: &str = "\x1b[1m";
const C_DIM: &str = "\x1b[2m";
const C_CYAN: &str = "\x1b[38;5;45m";
const C_GREEN: &str = "\x1b[38;5;114m";
const C_YELLOW: &str = "\x1b[38;5;214m";
const C_RED: &str = "\x1b[38;5;203m";
const C_GRAY: &str = "\x1b[38;5;243m";
const C_BG_HEADER: &str = "\x1b[48;5;235m";

/// Selected row style: white background, black foreground.
///
/// Uses 24-bit true colour rather than the 16-colour `30;107` pair, because
/// conhost's interpretation of the legacy bright-white background varies with
/// the user's colour scheme and can come out grey or invisible.
const C_SEL: &str = "\x1b[38;2;0;0;0m\x1b[48;2;255;255;255m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab { Optimize, BootTime, Help }

struct Log { text: String, tone: &'static str }

pub fn run() {
    let guard = match ConsoleGuard::new() {
        Some(g) => g,
        None => {
            eprintln!("错误：无法初始化控制台。");
            eprintln!("请在 Windows 终端 / cmd / PowerShell 窗口中直接运行 wsa.exe，");
            eprintln!("不要通过管道或重定向启动。");
            return;
        }
    };
    guard.hide_cursor();
    let mut state = AppState::new();
    let mut last_pulse = Instant::now();
    let mut dirty = true;
    let (w0, h0) = guard.size();
    let mut fb = crate::frame::FrameBuffer::new(w0, h0);

    // Initial full paint.
    state.pulse = Some(crate::engine::pulse());
    let text = render(&state, w0, h0);
    fb.parse(&text);
    print!("{}", fb.flush());
    let _ = io::stdout().flush();

    loop {
        let (w, h) = guard.size();
        let resized = w != fb.width() || h != fb.height();
        if resized {
            fb.resize(w, h);
            dirty = true;
        }
        if dirty || last_pulse.elapsed().as_millis() > 500 {
            state.pulse = Some(crate::engine::pulse());
            let text = render(&state, w, h);
            fb.parse(&text);
            let delta = fb.flush();
            if !delta.is_empty() {
                print!("{}", delta);
                let _ = io::stdout().flush();
            }
            last_pulse = Instant::now();
            dirty = false;
        }
        if guard.has_input() {
            match guard.read_event() {
                Some(event) => {
                    dirty = handle_event(&mut state, event, h);
                    if state.quit { break; }
                }
                // A record we couldn't turn into an event: yield briefly so we
                // never spin at 100% CPU on unhandled console records.
                None => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    }
}

struct AppState {
    quit: bool,
    tab: Tab,
    catalog: Vec<Optimization>,
    selected: usize,
    scroll: usize,
    logs: Vec<Log>,
    enabled: HashMap<String, bool>,
    filter: String,
    filtering: bool,
    pulse: Option<Pulse>,
    confirm_restore: bool,
}

impl AppState {
    fn new() -> Self {
        let mut catalog = optimize::catalog();
        let enabled = daemon::load_enabled();
        // Restore saved enabled flags; default = catalog default.
        for o in &mut catalog {
            if let Some(v) = enabled.get(o.id) { o.enabled = *v; }
        }
        optimize::detect_all(&mut catalog);
        Self {
            quit: false, tab: Tab::Optimize, catalog,
            selected: 0, scroll: 0,
            logs: vec![Log { text: "已加载优化项目录，按空格勾选，A 一键应用".into(), tone: C_GREEN }],
            enabled, filter: String::new(), filtering: false,
            pulse: None, confirm_restore: false,
        }
    }

    /// Build a state backed by the static catalog without touching the live
    /// system (no registry/service probing), for render unit tests.
    #[cfg(test)]
    fn new_for_test() -> Self {
        let catalog = optimize::catalog();
        Self {
            quit: false, tab: Tab::Optimize, catalog,
            selected: 0, scroll: 0,
            logs: vec![Log { text: "test".into(), tone: C_GREEN }],
            enabled: HashMap::new(), filter: String::new(), filtering: false,
            pulse: None, confirm_restore: false,
        }
    }

    fn visible_indices(&self) -> Vec<usize> {
        (0..self.catalog.len()).filter(|&i| {
            if self.filter.is_empty() { return true; }
            let f = self.filter.to_lowercase();
            let o = &self.catalog[i];
            o.name.to_lowercase().contains(&f)
                || o.description.to_lowercase().contains(&f)
                || o.category.label().contains(&f)
        }).collect()
    }

    fn log(&mut self, text: impl Into<String>, tone: &'static str) {
        self.logs.push(Log { text: text.into(), tone });
        if self.logs.len() > 100 { self.logs.remove(0); }
    }

    fn save(&mut self) {
        for o in &self.catalog { self.enabled.insert(o.id.to_string(), o.enabled); }
        if let Err(e) = daemon::save_enabled(&self.enabled) {
            self.log(format!("保存设置失败: {}", e), C_RED);
        }
    }

    fn apply_all(&mut self) {
        let r = optimize::apply_enabled(&self.catalog);
        if let Err(e) = daemon::append_backups(&r.backups) {
            self.log(format!("保存备份失败: {}", e), C_RED);
        }
        optimize::detect_all(&mut self.catalog);
        let extra = if r.needs_admin > 0 { format!(" ({} 项需管理员)", r.needs_admin) } else { String::new() };
        self.log(format!("应用完成: 成功 {} 项, 失败 {} 项{}, 预计节省 {:.1}s", r.applied, r.failed, extra, r.gain),
            if r.failed == 0 { C_GREEN } else { C_YELLOW });
    }

    fn toggle(&mut self) {
        let idx = match self.visible_indices().get(self.selected) {
            Some(&i) => i, None => return,
        };
        self.catalog[idx].enabled = !self.catalog[idx].enabled;
        self.save();
        let o = &self.catalog[idx];
        self.log(format!("{} {} 优化", o.name, if o.enabled { "已勾选" } else { "已取消" }), C_CYAN);
    }

    fn restore_all(&mut self) {
        let backups = daemon::load_backups();
        if backups.is_empty() {
            self.log("没有可恢复的更改", C_GRAY);
            return;
        }
        if !self.confirm_restore {
            self.confirm_restore = true;
            self.log(format!("确认恢复 {} 项更改？再按一次 R 确认", backups.len()), C_YELLOW);
            return;
        }
        self.confirm_restore = false;
        let mut ok = 0;
        let mut fail = 0;
        for b in &backups {
            match optimize::restore(b) { Ok(()) => ok += 1, Err(_) => fail += 1 }
        }
        let _ = daemon::clear_backups();
        optimize::detect_all(&mut self.catalog);
        self.log(format!("已恢复 {} 项, 失败 {} 项", ok, fail), C_GREEN);
    }
}

fn handle_key(s: &mut AppState, vk: u16, ch: char, _ctrl: u32) -> bool {
    let vlen = s.visible_indices().len();
    let page = 12usize;

    if s.filtering {
        match vk {
            VK_ENTER | VK_ESC => { s.filtering = false; return true; }
            0x08 => { s.filter.pop(); return true; }
            _ => {
                if ch.is_ascii_graphic() || ch == ' ' { s.filter.push(ch); return true; }
                return false;
            }
        }
    }

    match vk {
        VK_ESC => {
            if s.confirm_restore { s.confirm_restore = false; s.log("已取消恢复确认", C_GRAY); return true; }
            s.quit = true; return true;
        }
        VK_TAB => {
            s.tab = match s.tab { Tab::Optimize => Tab::BootTime, Tab::BootTime => Tab::Help, Tab::Help => Tab::Optimize };
            return true;
        }
        VK_UP => { if s.selected > 0 { s.selected -= 1; } s.scroll = s.scroll.min(s.selected); return true; }
        VK_DOWN => {
            if s.selected + 1 < vlen { s.selected += 1; }
            if s.selected >= s.scroll + page { s.scroll = s.selected - page + 1; }
            return true;
        }
        VK_PGUP => { s.selected = s.selected.saturating_sub(page); s.scroll = s.scroll.saturating_sub(page); return true; }
        VK_PGDN => { s.selected = (s.selected + page).min(vlen.saturating_sub(1)); s.scroll = s.scroll.saturating_add(page).min(vlen.saturating_sub(page)); return true; }
        VK_HOME => { s.selected = 0; s.scroll = 0; return true; }
        VK_END => { s.selected = vlen.saturating_sub(1); s.scroll = s.selected.saturating_sub(page - 1); return true; }
        VK_SPACE | VK_ENTER => { s.toggle(); return true; }
        _ => {}
    }

    match ch.to_ascii_lowercase() {
        'q' => s.quit = true,
        'a' => { s.apply_all(); return true; }
        'r' => { s.restore_all(); return true; }
        '/' | 'f' => { s.filtering = true; return true; }
        'c' => { s.filter.clear(); return true; }
        's' => { optimize::detect_all(&mut s.catalog); s.log("已重新检测优化状态", C_GREEN); return true; }
        '1' => s.tab = Tab::Optimize,
        '2' => s.tab = Tab::BootTime,
        '3' => s.tab = Tab::Help,
        _ => return false,
    }
    true
}

fn render(s: &AppState, w: usize, h: usize) -> String {
    let mut out = String::with_capacity(w * h * 3);
    if w < 64 || h < 18 {
        out.push_str(&format!("{}WSA{} 终端尺寸过小，请至少 64x18", C_CYAN, C_RESET));
        return out;
    }
    draw_header(&mut out, s, w);
    draw_tabs(&mut out, s, w);
    match s.tab {
        Tab::Optimize => draw_optimize(&mut out, s, w, h),
        Tab::BootTime => draw_boottime(&mut out, s, w, h),
        Tab::Help => draw_help(&mut out, s, w),
    }
    draw_footer(&mut out, s, w, h);
    out
}

fn draw_header(out: &mut String, s: &AppState, w: usize) {
    let enabled = s.catalog.iter().filter(|o| o.enabled).count();
    let optimized = s.catalog.iter().filter(|o| o.state == OptState::Optimized).count();
    let gain: f32 = s.catalog.iter().filter(|o| o.enabled && o.state != OptState::Optimized).map(|o| o.gain_sec).sum();
    let title = format!("{}WSA{} {}", C_CYAN, C_RESET, C_BOLD);
    let sub = format!("Windows 开机启动优化器  {}v0.2.0{}", C_DIM, C_RESET);
    let live = match s.pulse {
        Some(p) => format!("  内存 {}%  进程 {}", p.memory_load_pct, p.process_count),
        None => String::new(),
    };
    let stats = format!("已勾选 {} 项  已优化 {} 项  预计可省 {:.1}s{}", enabled, optimized, gain, live);
    out.push_str(&format!("{}{}\n", title, sub));
    out.push_str(&format!("{}\n", center(&stats, w)));
    out.push_str(&format!("{}{}\n", C_GRAY, "─".repeat(w)));
}

fn draw_tabs(out: &mut String, s: &AppState, w: usize) {
    let tabs = [("1 优化项", Tab::Optimize), ("2 开机耗时", Tab::BootTime), ("3 帮助", Tab::Help)];
    let mut line = String::new();
    for (label, tab) in tabs {
        if tab == s.tab { line.push_str(&format!("{} {} {} ", C_BG_HEADER, C_BOLD, label)); }
        else { line.push_str(&format!(" {} ", label)); }
    }
    out.push_str(&format!("{}{}{}\n", line, C_RESET, C_GRAY));
    out.push_str(&format!("{}{}\n", "─".repeat(w), C_RESET));
}

// Fixed column widths (measured in terminal display columns, not chars).
const COL_CHECK: usize = 3;   // "[✓]"
const COL_NAME: usize = 26;
const COL_CAT: usize = 10;
const COL_RISK: usize = 6;
const COL_GAIN: usize = 6;    // right-aligned " 2.5s"

/// Screen row (0-based) where the first data row is drawn: header(3) +
/// tabs(2) + column header(1) = 6.
const FIRST_DATA_ROW: usize = 6;

/// Route a decoded input event. Returns true when the screen must repaint.
fn handle_event(s: &mut AppState, event: InputEvent, h: usize) -> bool {
    match event {
        InputEvent::Key { vk, ch, ctrl } => handle_key(s, vk, ch, ctrl),
        InputEvent::Click { x: _, y } => handle_click(s, y, h),
        InputEvent::Scroll { delta } => {
            // Wheel scrolls the list a few rows at a time.
            let step = 3usize;
            let vlen = s.visible_indices().len();
            if delta > 0 {
                s.selected = s.selected.saturating_sub(step);
            } else {
                s.selected = (s.selected + step).min(vlen.saturating_sub(1));
            }
            s.scroll = s.scroll.min(s.selected);
            true
        }
    }
}

/// Map a click at screen row `y` onto a data row and select it.
fn handle_click(s: &mut AppState, y: usize, h: usize) -> bool {
    // Tabs sit on rows 3-4; a click there switches pages.
    if s.tab == Tab::Optimize && y == 3 {
        return true;
    }
    if y >= FIRST_DATA_ROW {
        let list_h = h.saturating_sub(10).max(1);
        let row = y - FIRST_DATA_ROW;
        if row < list_h {
            let vlen = s.visible_indices().len();
            let idx = s.scroll + row;
            if idx < vlen {
                s.selected = idx;
                return true;
            }
        }
    }
    false
}

fn draw_optimize(out: &mut String, s: &AppState, w: usize, h: usize) {
    let vis = s.visible_indices();
    let list_h = h.saturating_sub(10).max(1);

    // Header — padded by display width so CJK labels stay aligned.
    out.push_str(C_BOLD);
    out.push_str(C_BG_HEADER);
    out.push_str(C_DIM);
    out.push_str(&format!(
        "{} {} {} {} {} {} {}\n",
        pad_to("", COL_CHECK),
        pad_to("优化项", COL_NAME),
        pad_to("类别", COL_CAT),
        pad_to("风险", COL_RISK),
        pad_left("收益", COL_GAIN),
        pad_to("状态", 6),
        ""
    ));
    out.push_str(C_RESET);

    for row in 0..list_h {
        let idx = s.scroll + row;
        if idx >= vis.len() {
            // Keep the frame rectangular: an empty row must still occupy a
            // line so the row/column mapping used by mouse clicks stays true.
            out.push('\n');
            continue;
        }
        let oi = vis[idx];
        let o = &s.catalog[oi];
        let selected = idx == s.selected;
        let check = if o.enabled { "[x]" } else { "[ ]" };

        // Build the row text first, then paint it either with the per-column
        // colours or as one solid white-on-black selected bar.
        let check_color = if o.enabled { C_GREEN } else { C_GRAY };
        let state_color = match o.state {
            OptState::Optimized => C_GREEN,
            OptState::Default => C_YELLOW,
            OptState::Unknown => C_GRAY,
        };
        let risk_color = match o.risk {
            Risk::Safe => C_GREEN,
            Risk::Low => C_YELLOW,
            Risk::Moderate => C_RED,
        };

        let name = pad_to(&truncate(&o.name, COL_NAME), COL_NAME);
        let cat = pad_to(o.category.label(), COL_CAT);
        let risk = pad_to(o.risk.label(), COL_RISK);
        let gain = pad_left(&format!("{:.1}s", o.gain_sec), COL_GAIN);
        let state = pad_to(o.state.label(), 6);

        if selected {
            // Plain text (no SGR inside) so the whole line shares one style,
            // then pad out to the terminal width so the bar reaches the edge.
            let plain = format!(
                "{} {} {} {} {} {}",
                pad_to(check, COL_CHECK), name, cat, risk, gain, state
            );
            let used = crate::frame::text_width(&plain);
            let fill = w.saturating_sub(used);
            out.push_str(C_SEL);
            out.push_str(&plain);
            out.push_str(&" ".repeat(fill));
            out.push_str(C_RESET);
            out.push('\n');
        } else {
            out.push_str(&format!(
                "{}{} {} {} {}{} {}{} {}{}\n",
                check_color, check,
                name,
                cat,
                risk_color, risk,
                state_color, gain,
                state,
                C_RESET,
            ));
        }
    }

    // Detail panel for the selected item.
    if let Some(&oi) = vis.get(s.selected) {
        let o = &s.catalog[oi];
        let admin = if o.needs_admin { "（需管理员）" } else { "" };
        out.push_str(&format!("{}{}── {}{} ──{}\n", C_RESET, C_CYAN, o.name, admin, C_RESET));
        out.push_str(&format!("{}{}{}\n", C_DIM, wrap(&o.description, w.saturating_sub(2)), C_RESET));
    }
    if s.filtering {
        let used = 6 + crate::frame::text_width(&s.filter) + 1;
        out.push_str(&format!("\n{}筛选> {}{}▌{}{}\n", C_YELLOW, s.filter, C_RESET, " ".repeat(w.saturating_sub(used)), C_RESET));
    }
}

fn draw_boottime(out: &mut String, _s: &AppState, w: usize, h: usize) {
    let times = daemon::boot_times();
    let enabled = daemon::measurement_enabled();
    out.push_str(&format!("{}开机耗时测量{}  {}\n", C_BOLD, C_RESET,
        if enabled { format!("{}已开启{}", C_GREEN, C_RESET) } else { format!("{}默认关闭{}（在优化项页勾选“开机耗时测量”开启）", C_GRAY, C_RESET) }));
    out.push_str(&format!("{}{}\n", "─".repeat(w), C_GRAY));

    if times.is_empty() {
        out.push_str(&format!("{}暂无测量数据。开启测量并重启后，每次登录的耗时会记录在这里。{}\n", C_DIM, C_RESET));
    } else {
        let n = times.len();
        let avg: f32 = times.iter().map(|x| x.1).sum::<f32>() / n as f32;
        let best = times.iter().cloned().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap();
        let last = times[n - 1].1;
        out.push_str(&format!("  记录 {} 次  平均 {:.2}s  最快 {:.2}s  最近 {:.2}s\n", n, avg, best.1, last));
        out.push_str(&format!("{}\n", "─".repeat(w)));
        let max_bar = w.saturating_sub(24).min(40);
        let max_t = times.iter().map(|x| x.1).fold(0.0f32, f32::max).max(0.01);
        let show: Vec<_> = times.iter().rev().take(h.saturating_sub(9)).collect();
        for (ts, sec) in show.iter() {
            let bar = ((sec / max_t) * max_bar as f32) as usize;
            let time = chrono_like(*ts);
            out.push_str(&format!("  {} {:>6.2}s {} {}\n", time, sec, C_CYAN, "█".repeat(bar)));
        }
    }
    let _ = h;
}

fn draw_help(out: &mut String, _s: &AppState, w: usize) {
    let lines = [
        ("WSA 开机启动优化器 — 使用说明", C_BOLD),
        ("", C_RESET),
        ("  WSA 不再“管理启动项”，而是真正优化 Windows 开机过程：", C_CYAN),
        ("  · 将非必要系统服务改为延迟自动启动（微软官方支持、可逆）", C_RESET),
        ("  · 应用系统注册表性能 tweak（窗口动画、菜单延迟等）", C_RESET),
        ("  · 高影响启动程序分批错峰启动，避免资源踩踏", C_RESET),
        ("  · 可选的开机耗时测量与前后对比", C_RESET),
        ("", C_RESET),
        ("快捷键", C_BOLD),
        ("  ↑ ↓ / PgUp PgDn / Home End   移动选择", C_RESET),
        ("  空格 / Enter                 勾选/取消该优化项", C_RESET),
        ("  A                            一键应用所有已勾选项", C_RESET),
        ("  R                            恢复所有更改（按两次确认）", C_RESET),
        ("  S                            重新检测当前状态", C_RESET),
        ("  / 或 F                       筛选优化项", C_RESET),
        ("  1 / 2 / 3                    切换页面", C_RESET),
        ("  Esc                          退出（或取消恢复确认）", C_RESET),
        ("", C_RESET),
        ("安全说明", C_BOLD),
        ("  所有优化均可逆；服务延迟启动与注册表 tweak 会记录原值。", C_GREEN),
        ("  标记“需管理员”的项目需要以管理员身份运行 WSA 才能应用。", C_YELLOW),
        ("  开机自动执行需先运行: wsa --autostart-on", C_DIM),
    ];
    for (t, c) in lines { out.push_str(&format!("{}{}{}\n", c, t, C_RESET)); }
    let _ = w;
}

fn draw_footer(out: &mut String, s: &AppState, w: usize, h: usize) {
    out.push_str(&format!("{}{}\n", "─".repeat(w), C_GRAY));
    let recent: Vec<&Log> = s.logs.iter().rev().take(2).collect();
    for l in recent.iter().rev() {
        out.push_str(&format!("{}› {} {}\n", l.tone, truncate(&l.text, w.saturating_sub(3)), C_RESET));
    }
    out.push_str(&format!("{}{}↑↓选择  空格勾选  A应用  R恢复  S重检  /筛选  Tab换页  Esc退出{}",
        C_DIM, C_GRAY, C_RESET));
    let _ = h;
}

fn truncate(s: &str, max: usize) -> String {
    use crate::frame::{char_width, text_width};
    if text_width(s) <= max { return s.to_string(); }
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let cw = char_width(c);
        // Reserve one column for the ellipsis.
        if used + cw + 1 > max {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

fn wrap(s: &str, width: usize) -> String {
    use crate::frame::text_width;
    let mut out = String::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let need = text_width(&line) + text_width(word) + if line.is_empty() { 0 } else { 1 };
        if need > width && !line.is_empty() {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() { line.push(' '); }
        line.push_str(word);
    }
    if !line.is_empty() { out.push_str(&line); }
    out
}

fn center(s: &str, w: usize) -> String {
    let pad = w.saturating_sub(strip_ansi_len(s)) / 2;
    format!("{}{}", " ".repeat(pad), s)
}

/// Display width of a string that may contain SGR escape sequences.
fn strip_ansi_len(s: &str) -> usize {
    use crate::frame::char_width;
    let mut n = 0; let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' { while let Some(&x) = it.peek() { it.next(); if x == 'm' { break; } } }
        else { n += char_width(c); }
    }
    n
}

fn chrono_like(ts: u64) -> String {
    // Minimal UTC→local-ish display without chrono: show MM-DD HH:MM in UTC+8.
    let t = ts + 8 * 3600;
    let days = t / 86400;
    let secs = t % 86400;
    // Civil date from days since epoch (Howard Hinnant's algorithm).
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let _year = y + if m <= 2 { 1 } else { 0 };
    format!("{:02}-{:02} {:02}:{:02}", m, d, secs / 3600, (secs % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The selected row must be painted with an explicit white background and
    /// black foreground, and must extend across the full terminal width.
    #[test]
    fn selected_row_is_white_on_black() {
        let mut state = AppState::new_for_test();
        state.selected = 0;
        let text = render(&state, 100, 30);

        // Find the line that starts the selected-row bar.
        let sel_line = text
            .lines()
            .find(|l| l.contains(C_SEL))
            .expect("a row must use the selected style");

        assert!(
            sel_line.contains("48;2;255;255;255"),
            "selected row must set a white background"
        );
        assert!(
            sel_line.contains("38;2;0;0;0"),
            "selected row must set a black foreground"
        );

        // Strip SGR and measure: the bar must reach the terminal edge.
        let visible = crate::frame::text_width(&strip_sgr(sel_line));
        assert_eq!(visible, 100, "selected bar must span the full width");
    }

    /// Only one row may be highlighted at a time.
    #[test]
    fn only_one_row_is_selected() {
        let state = AppState::new_for_test();
        let text = render(&state, 100, 30);
        let count = text.lines().filter(|l| l.contains(C_SEL)).count();
        assert_eq!(count, 1, "exactly one row should be highlighted");
    }

    fn strip_sgr(s: &str) -> String {
        let mut out = String::new();
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            if c == '\x1b' {
                while let Some(&n) = it.peek() {
                    it.next();
                    if n == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
