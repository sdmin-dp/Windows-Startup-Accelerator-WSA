//! Double-buffered ANSI frame renderer.
//!
//! Instead of clearing the whole screen on every redraw (which flickers), we
//! parse the rendered ANSI text into a cell grid, diff it against the previous
//! frame, and emit only the cells that actually changed. This makes cursor
//! movement and live status updates completely flicker-free.

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: u8,   // 0 = terminal default
    pub bg: u8,   // 0 = terminal default
    pub bold: bool,
    pub dim: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    style: Style,
    /// True for the trailing column of a double-width (CJK) glyph.
    wide_tail: bool,
}

impl Default for Cell {
    fn default() -> Self {
        // A blank cell must be a space, never '\0' — writing NUL bytes to the
        // console is what caused spurious repaints and visual garbage.
        Self { ch: ' ', style: Style::default(), wide_tail: false }
    }
}

/// Terminal columns occupied by a character (East Asian Width approximation).
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    const WIDE: &[(u32, u32)] = &[
        (0x1100, 0x115F),   // Hangul Jamo
        (0x2E80, 0x303E),   // CJK radicals, Kangxi
        (0x3041, 0x33FF),   // Hiragana .. CJK compatibility
        (0x3400, 0x4DBF),   // CJK ext A
        (0x4E00, 0x9FFF),   // CJK unified
        (0xA000, 0xA4CF),   // Yi
        (0xAC00, 0xD7A3),   // Hangul syllables
        (0xF900, 0xFAFF),   // CJK compatibility ideographs
        (0xFE30, 0xFE6F),   // CJK compatibility forms
        (0xFF00, 0xFF60),   // Fullwidth forms
        (0xFFE0, 0xFFE6),   // Fullwidth signs
        (0x1F300, 0x1F64F), // Emoji
        (0x1F900, 0x1F9FF),
        (0x20000, 0x2FFFD),
        (0x30000, 0x3FFFD),
    ];
    for &(lo, hi) in WIDE {
        if u >= lo && u <= hi {
            return 2;
        }
    }
    1
}

/// Display width of a string measured in terminal columns.
pub fn text_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Pad `s` with trailing spaces so it occupies exactly `width` columns.
/// Truncation never splits a double-width glyph.
pub fn pad_to(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let cw = char_width(c);
        if used + cw > width {
            break;
        }
        out.push(c);
        used += cw;
    }
    while used < width {
        out.push(' ');
        used += 1;
    }
    out
}

/// Right-align `s` within `width` columns.
pub fn pad_left(s: &str, width: usize) -> String {
    let w = text_width(s);
    if w >= width {
        pad_to(s, width)
    } else {
        format!("{}{}", " ".repeat(width - w), s)
    }
}

pub struct FrameBuffer {
    w: usize,
    h: usize,
    cur: Vec<Cell>,
    prev: Vec<Cell>,
    has_prev: bool,
}

impl FrameBuffer {
    pub fn new(w: usize, h: usize) -> Self {
        let n = w * h;
        Self {
            w, h,
            cur: vec![Cell::default(); n],
            prev: vec![Cell::default(); n],
            has_prev: false,
        }
    }

    pub fn width(&self) -> usize { self.w }
    pub fn height(&self) -> usize { self.h }

    /// Resize the grid, preserving nothing (a full redraw follows).
    pub fn resize(&mut self, w: usize, h: usize) {
        if w != self.w || h != self.h {
            *self = Self::new(w, h);
        }
    }

    /// Parse ANSI text into the current cell grid.
    pub fn parse(&mut self, text: &str) {
        for c in self.cur.iter_mut() { *c = Cell::default(); }
        let (w, h) = (self.w, self.h);
        let mut x = 0usize;
        let mut y = 0usize;
        let mut st = Style::default();
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // CSI sequences: ESC [ ... final_byte
                if chars.peek() == Some(&'[') {
                    chars.next();
                    let mut params = String::new();
                    let mut final_byte = '\0';
                    while let Some(&p) = chars.peek() {
                        chars.next();
                        if p.is_ascii_alphabetic() {
                            final_byte = p;
                            break;
                        }
                        params.push(p);
                    }
                    match final_byte {
                        'H' | 'f' => {
                            let mut it = params.split(';');
                            let row = it.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
                            let col = it.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
                            y = row.saturating_sub(1);
                            x = col.saturating_sub(1);
                        }
                        'J' => {
                            // 2J clears the entire screen (treat as full reset).
                            if params.contains('2') {
                                for c in self.cur.iter_mut() { *c = Cell::default(); }
                            }
                        }
                        'K' => {
                            // Erase to end of line.
                            if y < h {
                                for cx in x..w {
                                    self.cur[y * w + cx] = Cell::default();
                                }
                            }
                        }
                        'm' => {
                            for code in params.split(';') {
                                match code {
                                    "" | "0" => st = Style::default(),
                                    "1" => st.bold = true,
                                    "2" => st.dim = true,
                                    "22" => { st.bold = false; st.dim = false; }
                                    "39" => st.fg = 0,
                                    "49" => st.bg = 0,
                                    _ => {
                                        if let Some(v) = code.strip_prefix("38;5;") {
                                            st.fg = v.parse().unwrap_or(0);
                                        } else if let Some(v) = code.strip_prefix("48;5;") {
                                            st.bg = v.parse().unwrap_or(0);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                // OSC sequences like ESC]... BEL — skip until BEL or ST.
                if chars.peek() == Some(&']') {
                    chars.next();
                    while let Some(&p) = chars.peek() {
                        chars.next();
                        if p == '\x07' { break; }
                    }
                    continue;
                }
                // ESC followed by a single byte (e.g. ?25h/l cursor) — skip.
                if let Some(&p) = chars.peek() {
                    if p == '?' || p == '(' || p == ')' {
                        chars.next();
                        while let Some(&q) = chars.peek() {
                            chars.next();
                            if q.is_ascii_alphabetic() { break; }
                        }
                    }
                }
                continue;
            }

            match c {
                '\n' => { y += 1; x = 0; }
                '\r' => { x = 0; }
                '\t' => { x = (x / 8 + 1) * 8; }
                c if c.is_control() => {}
                c => {
                    let cw = char_width(c);
                    if y < h && x < w {
                        self.cur[y * w + x] = Cell { ch: c, style: st, wide_tail: false };
                        // Mark the trailing column so flush() does not emit it
                        // (the glyph itself already advanced the terminal).
                        if cw == 2 && x + 1 < w {
                            self.cur[y * w + x + 1] = Cell { ch: ' ', style: st, wide_tail: true };
                        }
                    }
                    x += cw;
                }
            }
        }
    }

    /// Emit only the differences from the previous frame.
    pub fn flush(&mut self) -> String {
        let mut out = String::new();
        let mut style = Style::default();
        let mut cursor: Option<(usize, usize)> = None; // (x, y) last written position

        for y in 0..self.h {
            for x in 0..self.w {
                let idx = y * self.w + x;
                let cell = self.cur[idx];
                if cell.wide_tail {
                    // Covered by the preceding double-width glyph.
                    continue;
                }
                if self.has_prev && cell == self.prev[idx] {
                    continue;
                }
                // Position the cursor only when it is not already where we
                // need it (the previous cell write advances it by one).
                match cursor {
                    Some((cx, cy)) if cx == x && cy == y => {}
                    _ => {
                        out.push_str(&format!("\x1b[{};{}H", y + 1, x + 1));
                    }
                }
                if cell.style != style {
                    out.push_str(&sgr(cell.style, style));
                    style = cell.style;
                }
                out.push(cell.ch);
                cursor = Some((x + char_width(cell.ch), y));
            }
        }

        // Reset attributes so the next frame starts from a known state.
        // Only emit when something was actually drawn — an unchanged frame
        // must produce zero bytes, otherwise the terminal keeps receiving
        // writes and the UI visibly trembles.
        if !out.is_empty() {
            out.push_str("\x1b[0m");
        }
        self.prev.copy_from_slice(&self.cur);
        self.has_prev = true;
        out
    }

    /// Force the next flush to repaint everything.
    #[allow(dead_code)]
    pub fn invalidate(&mut self) {
        self.has_prev = false;
    }
}

/// Generate the minimal SGR delta from the old style to the new style.
fn sgr(new: Style, old: Style) -> String {
    let mut codes: Vec<String> = Vec::new();
    if new.fg != old.fg {
        if new.fg == 0 { codes.push("39".into()); }
        else { codes.push(format!("38;5;{}", new.fg)); }
    }
    if new.bg != old.bg {
        if new.bg == 0 { codes.push("49".into()); }
        else { codes.push(format!("48;5;{}", new.bg)); }
    }
    if new.bold != old.bold || new.dim != old.dim {
        if !new.bold && !new.dim { codes.push("22".into()); }
        else if new.bold { codes.push("1".into()); }
        else { codes.push("2".into()); }
    }
    if codes.is_empty() { String::new() } else { format!("\x1b[{}m", codes.join(";")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CJK characters must occupy two columns.
    #[test]
    fn char_width_cjk() {
        assert_eq!(char_width('A'), 1);
        assert_eq!(char_width('─'), 1);
        assert_eq!(char_width('中'), 2);
        assert_eq!(char_width('服'), 2);
    }

    /// Padding is measured in display columns, not chars.
    #[test]
    fn pad_uses_display_width() {
        assert_eq!(text_width(&pad_to("中文", 10)), 10);
        assert_eq!(text_width(&pad_to("abc", 10)), 10);
        // A wide glyph that would overflow the budget is dropped, then the
        // remaining columns are filled with spaces — never exceeding `width`.
        let padded = pad_to("中文中文中文", 5);
        assert_eq!(text_width(&padded), 5, "must fill exactly 5 columns");
        assert!(padded.starts_with("中文"), "first glyph fits");
        // Only two glyphs (4 columns) fit before the 5th column is padding.
        assert_eq!(padded.chars().filter(|c| *c == '中' || *c == '文').count(), 2);
    }

    /// An unchanged frame must produce no output at all — this is what makes
    /// the TUI immune to flicker.
    #[test]
    fn identical_frame_emits_nothing() {
        let mut fb = FrameBuffer::new(40, 5);
        let frame = "hello world\nsecond line\n";
        fb.parse(frame);
        let first = fb.flush();
        assert!(!first.is_empty(), "initial paint must draw");

        fb.parse(frame);
        let second = fb.flush();
        assert!(second.is_empty(), "unchanged frame must emit nothing");
    }

    /// Only the changed cell is emitted, not a full repaint.
    #[test]
    fn incremental_update_is_minimal() {
        let mut fb = FrameBuffer::new(40, 5);
        fb.parse("aaaa\nbbbb\n");
        let _ = fb.flush();

        fb.parse("aaaa\nbbXb\n");
        let delta = fb.flush();
        assert!(delta.contains('X'), "changed cell must be drawn");
        assert!(!delta.contains("aaaa"), "unchanged row must not be redrawn");
    }

    /// Wide glyphs must not desynchronise the following columns.
    #[test]
    fn wide_glyph_keeps_columns_aligned() {
        let mut fb = FrameBuffer::new(20, 3);
        fb.parse("中X\n");
        let out = fb.flush();
        // "中" occupies columns 1-2, so X is emitted immediately after it
        // without an intervening cursor move (they are adjacent on screen).
        assert!(out.contains("中X"), "X must follow the wide glyph directly, got: {:?}", out);
        assert!(!out.contains("\x1b[1;2H"), "must not try to write into the wide glyph's tail column");
    }
}
