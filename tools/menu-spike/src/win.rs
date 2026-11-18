//! The Windows-only measuring hooks:
//!
//! - a subclass on the main window that logs `SC_KEYMENU` and the menu loop, and (the "guard")
//!   swallows a `SC_KEYMENU` that would start Windows' menu loop with no bar to open;
//! - WebView2's `AcceleratorKeyPressed`, logged, plus the optional "key bridge" that forwards a
//!   lone Alt, F10, and Alt+letter to the native bar;
//! - mode B's popups, with a message-filter hook so Left and Right walk between menus;
//! - `--selftest`, which types a fixed key script with `SendInput` and saves screenshots.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};
use std::time::Duration;

use tauri::menu::{ContextMenu, Submenu};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewWindow, Wry};
use webview2_com::AcceleratorKeyPressedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP,
    COREWEBVIEW2_PHYSICAL_KEY_STATUS, ICoreWebView2Settings3,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EndMenu, EnumThreadWindows, GetClassNameW, GetMenuItemCount, GetMenuState, GetSubMenu,
    GetSystemMetrics, HHOOK, HMENU, IsWindowVisible, MF_BYPOSITION, MF_HILITE, MSG, PostMessageW, SC_KEYMENU,
    SM_CXSCREEN, SM_CYSCREEN, SetForegroundWindow, SetWindowsHookExW, UnhookWindowsHookEx, WH_MSGFILTER,
    WM_ENTERMENULOOP, WM_EXITMENULOOP, WM_INITMENUPOPUP, WM_KEYDOWN, WM_SYSCOMMAND,
};
use windows::core::{BOOL, Interface};

use crate::{Mode, Spike, log};

static APP: OnceLock<AppHandle> = OnceLock::new();
static HWND_RAW: AtomicIsize = AtomicIsize::new(0);

const VK_MENU: u32 = 0x12;
const VK_RMENU: i32 = 0xA5;
const VK_CONTROL: i32 = 0x11;
const VK_SHIFT: i32 = 0x10;
const VK_F10: u32 = 0x79;
const MSGF_MENU: i32 = 2;

fn hwnd() -> HWND {
    HWND(HWND_RAW.load(Ordering::SeqCst) as _)
}

fn down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let _ = APP.set(app.clone());
    let window = app.get_webview_window("main").expect("main window");
    let hwnd = window.hwnd()?;
    HWND_RAW.store(hwnd.0 as isize, Ordering::SeqCst);
    unsafe {
        let _ = SetWindowSubclass(HWND(hwnd.0 as _), Some(subclass), 0x5b1e, 0);
    }
    let browser_keys = app.state::<Spike>().settings.lock().unwrap().browser_keys;
    set_browser_keys(&window, browser_keys);
    let handle = app.clone();
    window.with_webview(move |webview| unsafe {
        let controller = webview.controller();
        let mut token = 0i64;
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            args.KeyEventKind(&mut kind)?;
            let mut vk = 0u32;
            args.VirtualKey(&mut vk)?;
            let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
            args.PhysicalKeyStatus(&mut status)?;
            if status.WasKeyDown.as_bool()
                && (kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN)
            {
                return Ok(()); // auto-repeat
            }
            let handled = on_webview_key(&handle, kind, vk);
            if handled {
                args.SetHandled(true)?;
            }
            Ok(())
        }));
        if let Err(e) = controller.add_AcceleratorKeyPressed(&handler, &mut token) {
            eprintln!("AcceleratorKeyPressed: {e}");
        }
    })?;
    Ok(())
}

fn kind_name(kind: COREWEBVIEW2_KEY_EVENT_KIND) -> &'static str {
    match kind {
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN => "down",
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP => "up",
        COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN => "sysdown",
        COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP => "sysup",
        _ => "?",
    }
}

fn vk_name(vk: u32) -> String {
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x10 => "Shift".into(),
        0x11 => "Ctrl".into(),
        0x12 => "Alt".into(),
        0x1B => "Escape".into(),
        0x20 => "Space".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("vk 0x{vk:02X}"),
    }
}

/// What the host hears from WebView2 for one key. Returns whether the bridge took it.
fn on_webview_key(app: &AppHandle, kind: COREWEBVIEW2_KEY_EVENT_KIND, vk: u32) -> bool {
    let altgr = down(VK_RMENU) && down(VK_CONTROL);
    let mods = format!(
        "{}{}{}{}",
        if down(VK_CONTROL) { "Ctrl+" } else { "" },
        if down(VK_SHIFT) { "Shift+" } else { "" },
        if down(0x12) && vk != VK_MENU { "Alt+" } else { "" },
        if altgr { "(AltGr) " } else { "" }
    );
    log(
        app,
        "webview2 accel",
        format!("{} {mods}{}", kind_name(kind), vk_name(vk)),
    );

    let spike = app.state::<Spike>();
    let (mode, bridge) = {
        let s = spike.settings.lock().unwrap();
        (s.mode, s.bridge)
    };
    if mode != Mode::Native || !bridge {
        return false;
    }
    let post = |letter: isize| unsafe {
        let _ = PostMessageW(Some(hwnd()), WM_SYSCOMMAND, WPARAM(SC_KEYMENU as usize), LPARAM(letter));
    };
    // A lone Alt needs no bridge: WebView2 hands an unhandled Alt release to `DefWindowProc`, which
    // sends `SC_KEYMENU` itself (measured on Windows 10 22H2, 2026-11-18). Forwarding it too opened
    // the menu loop twice.
    if kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN && vk == VK_F10 && !down(VK_SHIFT) {
        log(app, "bridge", "F10 -> SC_KEYMENU 0");
        post(0);
        return true;
    }
    if kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
        && !altgr
        && !down(VK_CONTROL)
        && let Some(letter) = char::from_u32(vk).filter(char::is_ascii_alphanumeric)
    {
        let letter = letter.to_ascii_lowercase();
        let is_title = spike.bar_mnemonics().contains(&letter);
        if is_title {
            log(app, "bridge", format!("Alt+{letter} -> SC_KEYMENU '{letter}'"));
            post(letter as isize);
            return true;
        }
    }
    false
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if let Some(app) = APP.get() {
        match msg {
            WM_SYSCOMMAND if (wparam.0 & 0xFFF0) == SC_KEYMENU as usize => {
                let letter = lparam.0;
                let what = if letter == 0 {
                    "0 (lone Alt or F10)".to_string()
                } else {
                    format!("'{}'", char::from_u32(letter as u32).unwrap_or('?'))
                };
                let guard = app.state::<Spike>().settings.lock().unwrap().guard;
                if guard && letter != 0x20 {
                    log(app, "host", format!("SC_KEYMENU {what}: swallowed by the guard"));
                    return LRESULT(0);
                }
                log(app, "host", format!("SC_KEYMENU {what}: to Windows"));
            }
            WM_ENTERMENULOOP => log(app, "host", "menu loop started"),
            WM_EXITMENULOOP => log(app, "host", "menu loop ended"),
            WM_INITMENUPOPUP => log(app, "host", "a menu opened"),
            _ => {}
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

pub fn set_browser_keys(window: &WebviewWindow, on: bool) {
    let _ = window.with_webview(move |webview| unsafe {
        let result = webview
            .controller()
            .CoreWebView2()
            .and_then(|core| core.Settings())
            .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
            .and_then(|settings| settings.SetAreBrowserAcceleratorKeysEnabled(on));
        if let Err(e) = result {
            eprintln!("browser keys: {e}");
        }
    });
}

static DIRECTION: AtomicI32 = AtomicI32::new(0);
static TOP_MENU: AtomicIsize = AtomicIsize::new(0);

/// Shows `submenu` at (`x`, `y`) and returns -1 or 1 when Left or Right at its top level asked
/// for the neighbor, 0 otherwise.
pub fn popup_with_arrows(window: &WebviewWindow, submenu: &Submenu<Wry>, x: f64, y: f64) -> i32 {
    DIRECTION.store(0, Ordering::SeqCst);
    TOP_MENU.store(submenu.hpopupmenu().unwrap_or(0), Ordering::SeqCst);
    let hook = unsafe { SetWindowsHookExW(WH_MSGFILTER, Some(menu_filter), None, GetCurrentThreadId()) };
    let _ = window.popup_menu_at(submenu, LogicalPosition::new(x, y));
    if let Ok(hook) = hook {
        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
    }
    DIRECTION.load(Ordering::SeqCst)
}

unsafe extern "system" fn count_menu_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut class = [0u16; 16];
    let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    if String::from_utf16_lossy(&class[..n]) == "#32768" && unsafe { IsWindowVisible(hwnd) }.as_bool() {
        unsafe { *(lparam.0 as *mut i32) += 1 };
    }
    BOOL(1)
}

fn open_menu_windows() -> i32 {
    let mut count = 0i32;
    unsafe {
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(count_menu_windows),
            LPARAM(&mut count as *mut i32 as isize),
        );
    }
    count
}

/// Whether the highlighted row of the top-level popup opens a submenu (then Right is its own).
fn highlighted_has_submenu() -> bool {
    let menu = HMENU(TOP_MENU.load(Ordering::SeqCst) as _);
    unsafe {
        let count = GetMenuItemCount(Some(menu));
        for i in 0..count.max(0) {
            if GetMenuState(menu, i as u32, MF_BYPOSITION) & MF_HILITE.0 != 0 {
                return !GetSubMenu(menu, i).is_invalid();
            }
        }
    }
    false
}

unsafe extern "system" fn menu_filter(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == MSGF_MENU {
        let msg = unsafe { &*(lparam.0 as *const MSG) };
        if msg.message == WM_KEYDOWN && open_menu_windows() <= 1 {
            let step = match msg.wParam.0 as u32 {
                0x25 => -1,
                0x27 if !highlighted_has_submenu() => 1,
                _ => 0,
            };
            if step != 0 {
                DIRECTION.store(step, Ordering::SeqCst);
                unsafe {
                    let _ = EndMenu();
                }
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) }
}

// ---------------------------------------------------------------------------------------------
// Selftest: types a fixed key script into the focused page and saves a screenshot per step.

#[derive(Clone, Copy)]
enum Step {
    Key(u16),
    Chord(&'static [u16], u16),
    AltGr(u16),
    /// Alt+letter, a pause, then one more key (Right, to walk to the next menu).
    AltThen(u16, u16),
}

fn key_input(vk: u16, up: bool) -> INPUT {
    let extended = matches!(vk, 0x25..=0x28 | 0xA5 | 0xA3 | 0x2D | 0x2E | 0x24 | 0x23 | 0x21 | 0x22);
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(keys: &[(u16, bool)]) -> u32 {
    let inputs: Vec<INPUT> = keys.iter().map(|&(vk, up)| key_input(vk, up)).collect();
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) }
}

fn press(step: Step) -> u32 {
    match step {
        Step::Key(vk) => send(&[(vk, false), (vk, true)]),
        Step::Chord(mods, vk) => {
            let mut keys: Vec<(u16, bool)> = mods.iter().map(|&m| (m, false)).collect();
            keys.push((vk, false));
            keys.push((vk, true));
            keys.extend(mods.iter().rev().map(|&m| (m, true)));
            send(&keys)
        }
        // Right Alt; on a layout with AltGr, Windows adds the left Ctrl itself.
        Step::AltGr(vk) => send(&[(0xA5, false), (vk, false), (vk, true), (0xA5, true)]),
        Step::AltThen(letter, next) => {
            let sent = press(Step::Chord(&[ALT], letter));
            std::thread::sleep(Duration::from_millis(700));
            sent + press(Step::Key(next))
        }
    }
}

const ALT: u16 = 0x12;
const CTRL: u16 = 0x11;
const SHIFT: u16 = 0x10;
const ESC: u16 = 0x1B;

fn script() -> Vec<(&'static str, Step)> {
    vec![
        ("alt-tap", Step::Key(ALT)),
        ("f10", Step::Key(0x79)),
        ("alt-f", Step::Chord(&[ALT], b'F' as u16)),
        ("alt-e", Step::Chord(&[ALT], b'E' as u16)),
        ("alt-f-right", Step::AltThen(b'F' as u16, 0x27)),
        ("f5", Step::Key(0x74)),
        ("ctrl-r", Step::Chord(&[CTRL], b'R' as u16)),
        ("alt-left", Step::Chord(&[ALT], 0x25)),
        ("ctrl-shift-c", Step::Chord(&[CTRL, SHIFT], b'C' as u16)),
        ("ctrl-shift-p", Step::Chord(&[CTRL, SHIFT], b'P' as u16)),
        ("shift-f10", Step::Chord(&[SHIFT], 0x79)),
        ("altgr-q", Step::AltGr(b'Q' as u16)),
        ("altgr-v", Step::AltGr(b'V' as u16)),
    ]
}

/// Runs the key script in each mode. `spec` is the screenshot folder ("-" for none).
pub fn run_selftest(app: AppHandle, spec: String) {
    std::thread::spawn(move || {
        let shots = (spec != "-").then(|| std::path::PathBuf::from(&spec));
        if let Some(dir) = &shots {
            let _ = std::fs::create_dir_all(dir);
        }
        std::thread::sleep(Duration::from_millis(4000));
        let runs: [(&str, Mode, bool); 4] = [
            ("A", Mode::Native, false),
            ("A-bridge", Mode::Native, true),
            ("B", Mode::Hybrid, false),
            ("C", Mode::Inpage, false),
        ];
        for (name, mode, bridge) in runs {
            let _ = app.emit("selftest-mode", (mode, bridge));
            std::thread::sleep(Duration::from_millis(1500));
            for (label, step) in script() {
                if label.starts_with("altgr") {
                    let _ = app.emit("selftest-focus", "text");
                } else {
                    let _ = app.emit("selftest-focus", "list");
                }
                std::thread::sleep(Duration::from_millis(400));
                unsafe {
                    let _ = SetForegroundWindow(hwnd());
                }
                log(&app, "selftest", format!("--- {name} {label}"));
                let sent = press(step);
                if sent == 0 {
                    log(&app, "selftest", "SendInput sent nothing (locked or no input desktop?)");
                }
                std::thread::sleep(Duration::from_millis(1400));
                if let Some(dir) = &shots {
                    save_screen(&dir.join(format!("{name}-{label}.bmp")));
                }
                send(&[(ESC, false), (ESC, true)]);
                std::thread::sleep(Duration::from_millis(300));
                send(&[(ESC, false), (ESC, true)]);
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        log(&app, "selftest", "done");
        std::thread::sleep(Duration::from_millis(500));
        app.exit(0);
    });
}

/// Saves the whole primary screen as a 24-bit BMP.
fn save_screen(path: &std::path::Path) {
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 {
            return;
        }
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(memory, bitmap.into());
        let _ = BitBlt(memory, 0, 0, width, height, Some(screen), 0, 0, SRCCOPY);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: height,
                biPlanes: 1,
                biBitCount: 24,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let stride = ((width * 3 + 3) & !3) as usize;
        let mut pixels = vec![0u8; stride * height as usize];
        GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        let mut file = Vec::with_capacity(54 + pixels.len());
        let size = (54 + pixels.len()) as u32;
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&size.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&54u32.to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&width.to_le_bytes());
        file.extend_from_slice(&height.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&24u16.to_le_bytes());
        file.extend_from_slice(&[0u8; 24]);
        file.extend_from_slice(&pixels);
        let _ = std::fs::write(path, file);
    }
}
