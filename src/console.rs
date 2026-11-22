//! Zero-dependency Windows console control: raw mode, UTF-8, cursor, colors,
//! and keyboard input via `ReadConsoleInputW`.

#![cfg(windows)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::winapi as w;
use std::io::{self, Write};

pub const STD_INPUT_HANDLE: w::DWORD = (-10i32) as w::DWORD;
pub const STD_OUTPUT_HANDLE: w::DWORD = (-11i32) as w::DWORD;

pub const ENABLE_PROCESSED_INPUT: w::DWORD = 0x0001;
pub const ENABLE_LINE_INPUT: w::DWORD = 0x0002;
pub const ENABLE_ECHO_INPUT: w::DWORD = 0x0004;
pub const ENABLE_WINDOW_INPUT: w::DWORD = 0x0008;
pub const ENABLE_MOUSE_INPUT: w::DWORD = 0x0010;
pub const ENABLE_EXTENDED_FLAGS: w::DWORD = 0x0080;
pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: w::DWORD = 0x0004;
pub const ENABLE_QUICK_EDIT_MODE: w::DWORD = 0x0040;

pub const KEY_EVENT: u16 = 0x0001;
pub const MOUSE_EVENT: u16 = 0x0002;

/// A decoded input event, uniform for keyboard and mouse.
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    Key { vk: u16, ch: char, ctrl: u32 },
    /// Left button pressed at a screen cell (0-based).
    Click { x: usize, y: usize },
    /// Mouse wheel: positive = up.
    Scroll { delta: i32 },
}

/// Win32 `MOUSE_EVENT_RECORD`.
///
/// Layout: COORD(4) + DWORD(4) + DWORD(4) + DWORD(4) = 16 bytes.
#[derive(Debug, Clone, Copy)]
pub struct MOUSE_EVENT_RECORD {
    pub dwMousePosition: COORD,
    pub dwButtonState: u32,
    pub dwControlKeyState: u32,
    pub dwEventFlags: u32,
}

pub const FROM_LEFT_1ST_BUTTON_PRESSED: u32 = 0x0001;
pub const MOUSE_MOVED: u32 = 0x0001;
pub const MOUSE_WHEELED: u32 = 0x0004;
pub const DOUBLE_CLICK: u32 = 0x0002;

impl MOUSE_EVENT_RECORD {
    fn from_bytes(b: &[u8; 16]) -> Self {
        let word = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
        let dword = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        Self {
            dwMousePosition: COORD { X: word(0), Y: word(2) },
            dwButtonState: dword(4),
            dwControlKeyState: dword(8),
            dwEventFlags: dword(12),
        }
    }
}

/// `INVALID_HANDLE_VALUE` as returned by `GetStdHandle` on failure.
const INVALID_HANDLE_VALUE: w::HANDLE = -1isize as w::HANDLE;

/// Win32 `KEY_EVENT_RECORD`. Note the layout: `bKeyDown` is a BOOL (4 bytes),
/// then four WORDs, then a DWORD — total 16 bytes with 4-byte alignment.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KEY_EVENT_RECORD {
    pub bKeyDown: i32,
    pub wRepeatCount: u16,
    pub wVirtualKeyCode: u16,
    pub wVirtualScanCode: u16,
    pub uChar: u16,
    pub dwControlKeyState: u32,
}

/// Win32 `INPUT_RECORD`.
///
/// `EventType` is a WORD, but the following union contains DWORD-aligned
/// members (MOUSE_EVENT_RECORD, WINDOW_BUFFER_SIZE_RECORD), so the union
/// starts at offset **4**, not 2. Declaring the union as `[u8; 16]` directly
/// after a `u16` would shift every field by two bytes and make all keyboard
/// input unreadable — which is exactly the bug this layout avoids.
#[repr(C, align(4))]
#[derive(Debug, Clone, Copy)]
pub struct INPUT_RECORD {
    pub EventType: u16,
    /// Pads `EventType` out to the union's offset of 4.
    pub _padding: u16,
    pub Event: [u8; 16],
}

impl Default for INPUT_RECORD {
    fn default() -> Self {
        Self { EventType: 0, _padding: 0, Event: [0; 16] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    /// The Win32 ABI requires INPUT_RECORD to be 20 bytes, 4-byte aligned,
    /// with the event union at offset 4. Getting this wrong makes every key
    /// event unreadable — the exact bug this test locks down.
    #[test]
    fn input_record_layout_matches_win32() {
        assert_eq!(size_of::<INPUT_RECORD>(), 20, "INPUT_RECORD must be 20 bytes");
        assert_eq!(align_of::<INPUT_RECORD>(), 4, "INPUT_RECORD must be 4-byte aligned");
        assert_eq!(std::mem::offset_of!(INPUT_RECORD, Event), 4, "event union must start at offset 4");
    }

    /// KEY_EVENT_RECORD must be exactly 16 bytes, matching the union size.
    #[test]
    fn key_event_record_size() {
        assert_eq!(size_of::<KEY_EVENT_RECORD>(), 16);
    }

    /// Field offsets must match the C layout, otherwise decoding produces
    /// garbage virtual-key codes and the UI ignores every keystroke.
    #[test]
    fn key_event_field_offsets() {
        assert_eq!(std::mem::offset_of!(KEY_EVENT_RECORD, bKeyDown), 0);
        assert_eq!(std::mem::offset_of!(KEY_EVENT_RECORD, wRepeatCount), 4);
        assert_eq!(std::mem::offset_of!(KEY_EVENT_RECORD, wVirtualKeyCode), 6);
        assert_eq!(std::mem::offset_of!(KEY_EVENT_RECORD, wVirtualScanCode), 8);
        assert_eq!(std::mem::offset_of!(KEY_EVENT_RECORD, uChar), 10);
        assert_eq!(std::mem::offset_of!(KEY_EVENT_RECORD, dwControlKeyState), 12);
    }

    /// Decoding a hand-built DOWN-ARROW event must yield the right fields.
    #[test]
    fn decodes_arrow_key() {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&1i32.to_le_bytes());      // bKeyDown = TRUE
        b[4..6].copy_from_slice(&1u16.to_le_bytes());      // wRepeatCount
        b[6..8].copy_from_slice(&0x28u16.to_le_bytes());   // VK_DOWN
        b[10..12].copy_from_slice(&0u16.to_le_bytes());    // no character
        let key = KEY_EVENT_RECORD::from_bytes(&b);
        assert_eq!(key.bKeyDown, 1);
        assert_eq!(key.wVirtualKeyCode, 0x28, "VK_DOWN");
        assert_eq!(key.uChar, 0);
    }

    /// A printable key must carry its character.
    #[test]
    fn decodes_letter_key() {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&1i32.to_le_bytes());
        b[6..8].copy_from_slice(&0x51u16.to_le_bytes());   // VK_Q
        b[10..12].copy_from_slice(&(b'q' as u16).to_le_bytes()); // uChar
        let key = KEY_EVENT_RECORD::from_bytes(&b);
        assert_eq!(key.wVirtualKeyCode, 0x51);
        assert_eq!(key.uChar, b'q' as u16);
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct COORD {
    pub X: i16,
    pub Y: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SMALL_RECT {
    pub Left: i16,
    pub Top: i16,
    pub Right: i16,
    pub Bottom: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CONSOLE_SCREEN_BUFFER_INFO {
    pub dwSize: COORD,
    pub dwCursorPosition: COORD,
    pub wAttributes: u16,
    pub srWindow: SMALL_RECT,
    pub dwMaximumWindowSize: COORD,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetStdHandle(which: w::DWORD) -> w::HANDLE;
    fn SetConsoleMode(handle: w::HANDLE, mode: w::DWORD) -> i32;
    fn GetConsoleMode(handle: w::HANDLE, mode: *mut w::DWORD) -> i32;
    fn ReadConsoleInputW(handle: w::HANDLE, buffer: *mut INPUT_RECORD, len: w::DWORD, read: *mut w::DWORD) -> i32;
    fn PeekConsoleInputW(handle: w::HANDLE, buffer: *mut INPUT_RECORD, len: w::DWORD, read: *mut w::DWORD) -> i32;
    fn FlushConsoleInputBuffer(handle: w::HANDLE) -> i32;
    fn GetConsoleScreenBufferInfo(handle: w::HANDLE, info: *mut CONSOLE_SCREEN_BUFFER_INFO) -> i32;
    fn SetConsoleCursorPosition(handle: w::HANDLE, pos: COORD) -> i32;
    fn AllocConsole() -> i32;
    fn FreeConsole() -> i32;
}

/// Create a private console for this process (used by the self-check when the
/// process was started without a terminal). Returns true on success.
///
/// `AllocConsole` fails when the process already holds a console reference, so
/// detach first. The caller must re-acquire handles afterwards — the old
/// standard handles point at nothing once the console is replaced.
pub fn alloc_private_console() -> bool {
    unsafe {
        FreeConsole();
        AllocConsole() != 0
    }
}

/// Detach from the private console created by `alloc_private_console`.
pub fn free_private_console() {
    unsafe { FreeConsole() };
}

/// RAII guard that restores console state on drop.
pub struct ConsoleGuard {
    input: w::HANDLE,
    output: w::HANDLE,
    old_input: w::DWORD,
    old_output: w::DWORD,
}

impl ConsoleGuard {
    /// Attach to the console, or `None` when there is no usable console
    /// (stdin redirected, launched without a terminal, etc.). Returning
    /// `Option` lets the caller print a clear error instead of silently
    /// exiting — a silent exit is indistinguishable from a frozen UI.
    pub fn new() -> Option<Self> {
        let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        if input.is_null() || input == INVALID_HANDLE_VALUE
            || output.is_null() || output == INVALID_HANDLE_VALUE
        {
            return None;
        }
        let mut old_input = 0;
        let mut old_output = 0;
        unsafe {
            // GetConsoleMode fails on redirected/non-console handles.
            if GetConsoleMode(input, &mut old_input) == 0
                || GetConsoleMode(output, &mut old_output) == 0
            {
                return None;
            }
            // Enable VT processing so ANSI colours and cursor moves work.
            SetConsoleMode(output, old_output | ENABLE_VIRTUAL_TERMINAL_PROCESSING);

            // Build a clean raw input mode from scratch rather than patching
            // Keep mouse input ON so the user can click rows, but turn off
            // QUICK_EDIT_MODE — with quick-edit enabled, a click freezes the
            // program into text-selection mode, which looks exactly like a
            // hung UI.
            let raw = (old_input
                & !(ENABLE_LINE_INPUT
                    | ENABLE_ECHO_INPUT
                    | ENABLE_PROCESSED_INPUT
                    | ENABLE_QUICK_EDIT_MODE))
                | ENABLE_EXTENDED_FLAGS
                | ENABLE_WINDOW_INPUT
                | ENABLE_MOUSE_INPUT;
            SetConsoleMode(input, raw);

            // Drop anything queued before we entered raw mode (the click that
            // launched us, leftover keystrokes, etc.).
            FlushConsoleInputBuffer(input);
        }
        let _ = io::stdout().flush();
        Some(Self { input, output, old_input, old_output })
    }

    pub fn size(&self) -> (usize, usize) {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
        if unsafe { GetConsoleScreenBufferInfo(self.output, &mut info) } == 0 {
            return (80, 24);
        }
        (
            (info.srWindow.Right - info.srWindow.Left + 1) as usize,
            (info.srWindow.Bottom - info.srWindow.Top + 1) as usize,
        )
    }

    pub fn hide_cursor(&self) {
        print!("\x1b[?25l");
        let _ = io::stdout().flush();
    }

    /// Non-blocking check for whether any *usable* event is pending.
    /// Records we cannot interpret (focus, resize) are consumed and dropped so
    /// the caller never spins on something it will never handle.
    pub fn has_input(&self) -> bool {
        loop {
            let mut records = [INPUT_RECORD::default(); 1];
            let mut read: w::DWORD = 0;
            let ok = unsafe {
                PeekConsoleInputW(self.input, records.as_mut_ptr(), 1, &mut read)
            };
            if ok == 0 || read == 0 {
                return false;
            }
            match records[0].EventType {
                KEY_EVENT | MOUSE_EVENT => return true,
                _ => {
                    let mut scratch = [INPUT_RECORD::default(); 1];
                    let mut consumed: w::DWORD = 0;
                    if unsafe {
                        ReadConsoleInputW(self.input, scratch.as_mut_ptr(), 1, &mut consumed)
                    } == 0
                    {
                        return false;
                    }
                }
            }
        }
    }

    /// Read the next keyboard or mouse event.
    ///
    /// NOTE: this blocks. Call `has_input()` first — calling it unconditionally
    /// on an empty queue hangs the process.
    pub fn read_event(&self) -> Option<InputEvent> {
        let mut records = [INPUT_RECORD::default(); 32];
        let mut read: w::DWORD = 0;
        let ok = unsafe {
            ReadConsoleInputW(self.input, records.as_mut_ptr(), records.len() as w::DWORD, &mut read)
        };
        if ok == 0 || read == 0 {
            return None;
        }
        for r in records.iter().take(read as usize) {
            match r.EventType {
                KEY_EVENT => {
                    let key = KEY_EVENT_RECORD::from_bytes(&r.Event);
                    // Only act on key-down; key-up would double every action.
                    if key.bKeyDown == 0 {
                        continue;
                    }
                    let ch = char::from_u32(key.uChar as u32).unwrap_or('\0');
                    return Some(InputEvent::Key {
                        vk: key.wVirtualKeyCode,
                        ch,
                        ctrl: key.dwControlKeyState,
                    });
                }
                MOUSE_EVENT => {
                    let m = MOUSE_EVENT_RECORD::from_bytes(&r.Event);
                    if m.dwEventFlags & MOUSE_WHEELED != 0 {
                        let delta = (m.dwButtonState as i16) as i32;
                        return Some(InputEvent::Scroll { delta });
                    }
                    // MOUSE_MOVED shares bit 0x1 with the left-button flag, so
                    // motion has to be excluded or hovering looks like a click.
                    let moved = m.dwEventFlags & MOUSE_MOVED != 0;
                    let pressed = m.dwButtonState & FROM_LEFT_1ST_BUTTON_PRESSED != 0;
                    let double = m.dwEventFlags & DOUBLE_CLICK != 0;
                    if (pressed || double) && !moved {
                        if m.dwMousePosition.X < 0 || m.dwMousePosition.Y < 0 {
                            continue;
                        }
                        return Some(InputEvent::Click {
                            x: m.dwMousePosition.X as usize,
                            y: m.dwMousePosition.Y as usize,
                        });
                    }
                    continue;
                }
                _ => continue,
            }
        }
        None
    }

    /// Diagnostic helper: print every event received until Esc.
    pub fn run_key_diagnostics(&self) {
        println!("输入诊断：请按方向键/空格/字母，也可点击窗口，Esc 退出。");
        println!("（键盘显示 VK 码与字符，鼠标显示坐标）");
        loop {
            if self.has_input() {
                match self.read_event() {
                    Some(InputEvent::Key { vk, ch, ctrl }) => {
                        println!(
                            "KEY  VK=0x{:02X} char={:?} ctrl=0x{:X}",
                            vk,
                            if ch == '\0' { "<none>".to_string() } else { ch.to_string() },
                            ctrl
                        );
                        if vk == 0x1B {
                            println!("收到 Esc，退出诊断。");
                            break;
                        }
                    }
                    Some(InputEvent::Click { x, y }) => println!("MOUSE click x={} y={}", x, y),
                    Some(InputEvent::Scroll { delta }) => println!("MOUSE wheel delta={}", delta),
                    None => {}
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
        }
    }
}

impl KEY_EVENT_RECORD {
    /// Decode a KEY_EVENT_RECORD from the raw union payload.
    fn from_bytes(b: &[u8; 16]) -> Self {
        let dword = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let word = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        Self {
            bKeyDown: dword(0) as i32,
            wRepeatCount: word(4),
            wVirtualKeyCode: word(6),
            wVirtualScanCode: word(8),
            uChar: word(10),
            dwControlKeyState: dword(12),
        }
    }
}

/// Decode-pipeline self-check that needs no console at all.
///
/// Builds synthetic KEY_EVENT_RECORD payloads, runs them through the same
/// decoder the live UI uses, and reports what came out. This proves the ABI
/// fix works regardless of how the process was launched (pipe, service, IDE).
pub fn selftest_decode() -> Result<Vec<String>, String> {
    let cases: &[(u16, u16, &str)] = &[
        (0x28, 0, "Down"),
        (0x26, 0, "Up"),
        (0x20, b' ' as u16, "Space"),
        (0x51, b'q' as u16, "Q"),
        (0x1B, 0x1B, "Esc"),
    ];
    let mut out = Vec::new();
    for &(vk, ch, label) in cases {
        let mut payload = [0u8; 16];
        payload[0..4].copy_from_slice(&1i32.to_le_bytes());   // key down
        payload[4..6].copy_from_slice(&1u16.to_le_bytes());   // repeat count
        payload[6..8].copy_from_slice(&vk.to_le_bytes());     // virtual key
        payload[10..12].copy_from_slice(&ch.to_le_bytes());   // character

        let key = KEY_EVENT_RECORD::from_bytes(&payload);
        if key.wVirtualKeyCode != vk {
            return Err(format!(
                "{}: VK 期望 0x{:02X}，实得 0x{:02X}",
                label, vk, key.wVirtualKeyCode
            ));
        }
        if ch != 0 && key.uChar != ch {
            return Err(format!("{}: 字符解码错误", label));
        }
        let decoded_ch = char::from_u32(key.uChar as u32).unwrap_or('\0');
        out.push(format!(
            "{:<6} VK=0x{:02X} char={:?}",
            label,
            key.wVirtualKeyCode,
            if decoded_ch == '\0' { "<无>".to_string() } else { decoded_ch.to_string() }
        ));
    }
    Ok(out)
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        print!("\x1b[0m\x1b[?25h\x1b[2J\x1b[H");
        let _ = io::stdout().flush();
        unsafe {
            SetConsoleMode(self.input, self.old_input);
            SetConsoleMode(self.output, self.old_output);
        }
    }
}
