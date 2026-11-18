//! A throwaway measuring tool for the Windows menu model (task 63). Three modes show Cmdr's real
//! menu bar as (A) muda's native Win32 bar, (B) an in-page title strip opening native popups, and
//! (C) a page-drawn bar, and an event log says which layer heard each key. See `README.md`.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod model;
#[cfg(windows)]
mod win;

use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{
    AppHandle, Emitter, LogicalPosition, Manager, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry,
};

use model::{Row, TopMenu, with_ampersand};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Native,
    Hybrid,
    Inpage,
}

pub struct Settings {
    pub mode: Mode,
    /// Mode A only: forward Alt, F10, and Alt+letter from WebView2 to the native bar.
    pub bridge: bool,
    /// Swallow a `SC_KEYMENU` with no bar to open (hybrid and in-page), so Windows' menu loop
    /// doesn't start.
    pub guard: bool,
    /// WebView2's browser keys (F5 reload, Ctrl+R, Alt+Left back, …).
    pub browser_keys: bool,
    /// `None` follows the system.
    pub theme: Option<Theme>,
}

pub struct Spike {
    pub settings: Mutex<Settings>,
    bar: Vec<TopMenu>,
    native_bar: Mutex<Option<Menu<Wry>>>,
    popups: Mutex<Vec<Submenu<Wry>>>,
    context: Mutex<Option<Menu<Wry>>>,
    log_file: Mutex<Option<std::fs::File>>,
}

impl Spike {
    /// The bar titles' mnemonic letters, lowercased.
    pub fn bar_mnemonics(&self) -> Vec<char> {
        self.bar
            .iter()
            .filter_map(|m| m.mnemonic.and_then(|i| m.label.chars().nth(i)))
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect()
    }
}

#[derive(Clone, Serialize)]
struct LogLine {
    layer: &'static str,
    text: String,
}

/// Writes one line to the page's event log (and the `--log` file).
pub fn log(app: &AppHandle, layer: &'static str, text: impl Into<String>) {
    let text = text.into();
    if let Some(spike) = app.try_state::<Spike>()
        && let Some(file) = spike.log_file.lock().unwrap().as_mut()
    {
        use std::io::Write;
        let _ = writeln!(file, "{:>10} [{layer}] {text}", now_ms());
    }
    let _ = app.emit("spike-log", LogLine { layer, text });
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn build_rows(app: &AppHandle, rows: &[Row], register: bool) -> tauri::Result<Vec<Box<dyn IsMenuItem<Wry>>>> {
    let mut out: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    for row in rows {
        match row {
            Row::Separator => out.push(Box::new(PredefinedMenuItem::separator(app)?)),
            Row::Submenu { label, mnemonic, rows } => {
                let children = build_rows(app, rows, register)?;
                let refs: Vec<&dyn IsMenuItem<Wry>> = children.iter().map(|c| c.as_ref()).collect();
                out.push(Box::new(Submenu::with_items(
                    app,
                    with_ampersand(label, *mnemonic),
                    true,
                    &refs,
                )?));
            }
            Row::Item {
                id,
                label,
                mnemonic,
                accelerator,
                shown,
                enabled,
                checked,
            } => {
                let mut text = with_ampersand(label, *mnemonic);
                let mut accel = if register { accelerator.clone() } else { None };
                // What the menu only shows goes after a tab, which Win32 right-aligns.
                let display = shown
                    .clone()
                    .or_else(|| if register { None } else { accelerator.clone() });
                if accel.is_none()
                    && let Some(display) = display
                {
                    text = format!("{text}\t{display}");
                }
                let built: tauri::Result<Box<dyn IsMenuItem<Wry>>> = match checked {
                    Some(c) => CheckMenuItem::with_id(app, id, &text, *enabled, *c, accel.as_deref())
                        .map(|i| Box::new(i) as Box<dyn IsMenuItem<Wry>>),
                    None => MenuItem::with_id(app, id, &text, *enabled, accel.as_deref())
                        .map(|i| Box::new(i) as Box<dyn IsMenuItem<Wry>>),
                };
                let built = match built {
                    Ok(item) => item,
                    Err(e) => {
                        // An accelerator muda can't parse: show it instead of registering it.
                        log(
                            app,
                            "setup",
                            format!("accelerator {accel:?} on {id} refused ({e}); shown only"),
                        );
                        let text = format!(
                            "{}\t{}",
                            with_ampersand(label, *mnemonic),
                            accel.take().unwrap_or_default()
                        );
                        Box::new(MenuItem::with_id(app, id, &text, *enabled, None::<&str>)?)
                    }
                };
                out.push(built);
            }
        }
    }
    Ok(out)
}

fn build_top(app: &AppHandle, menu: &TopMenu, register: bool) -> tauri::Result<Submenu<Wry>> {
    let children = build_rows(app, &menu.rows, register)?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = children.iter().map(|c| c.as_ref()).collect();
    Submenu::with_items(app, with_ampersand(&menu.label, menu.mnemonic), true, &refs)
}

fn main_window(app: &AppHandle) -> WebviewWindow {
    app.get_webview_window("main").expect("main window")
}

/// Puts the window in `mode`: attaches or removes the native bar.
fn apply_mode(app: &AppHandle) -> tauri::Result<()> {
    let spike = app.state::<Spike>();
    let window = main_window(app);
    let mode = spike.settings.lock().unwrap().mode;
    match mode {
        Mode::Native => {
            let menu = spike.native_bar.lock().unwrap().clone();
            if let Some(menu) = menu {
                window.set_menu(menu)?;
            }
        }
        Mode::Hybrid | Mode::Inpage => {
            let _ = window.remove_menu();
        }
    }
    {
        let mut settings = spike.settings.lock().unwrap();
        settings.guard = mode != Mode::Native;
    }
    log(app, "setup", format!("mode {mode:?}"));
    Ok(())
}

#[derive(Serialize)]
struct State {
    mode: Mode,
    bridge: bool,
    guard: bool,
    browser_keys: bool,
    theme: &'static str,
    bar: Vec<TopMenu>,
    windows: bool,
}

#[tauri::command]
fn get_state(app: AppHandle) -> State {
    let spike = app.state::<Spike>();
    let settings = spike.settings.lock().unwrap();
    State {
        mode: settings.mode,
        bridge: settings.bridge,
        guard: settings.guard,
        browser_keys: settings.browser_keys,
        theme: match settings.theme {
            Some(Theme::Dark) => "dark",
            Some(Theme::Light) => "light",
            _ => "system",
        },
        bar: spike.bar.clone(),
        windows: cfg!(windows),
    }
}

#[tauri::command]
fn set_mode(app: AppHandle, mode: Mode) -> Result<(), String> {
    app.state::<Spike>().settings.lock().unwrap().mode = mode;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = apply_mode(&handle) {
            log(&handle, "setup", format!("mode switch: {e}"));
        }
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_flags(app: AppHandle, bridge: bool, guard: bool, browser_keys: bool) {
    let spike = app.state::<Spike>();
    {
        let mut s = spike.settings.lock().unwrap();
        s.bridge = bridge;
        s.guard = guard;
        s.browser_keys = browser_keys;
    }
    #[cfg(windows)]
    win::set_browser_keys(&main_window(&app), browser_keys);
    log(
        &app,
        "setup",
        format!("bridge={bridge} guard={guard} browserKeys={browser_keys}"),
    );
}

#[tauri::command]
fn set_theme(app: AppHandle, theme: String) {
    let theme = match theme.as_str() {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None,
    };
    app.state::<Spike>().settings.lock().unwrap().theme = theme;
    let _ = main_window(&app).set_theme(theme);
    log(&app, "setup", format!("theme {theme:?}"));
}

/// The page's own log lines (keydown and friends) also go to the `--log` file.
#[tauri::command]
fn page_log(app: AppHandle, text: String) {
    if let Some(file) = app.state::<Spike>().log_file.lock().unwrap().as_mut() {
        use std::io::Write;
        let _ = writeln!(file, "{:>10} [page] {text}", now_ms());
    }
}

#[tauri::command]
fn page_click(app: AppHandle, id: String) {
    log(&app, "in-page menu", format!("click {id}"));
}

#[tauri::command]
fn show_context_menu(app: AppHandle, x: f64, y: f64) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let menu = handle.state::<Spike>().context.lock().unwrap().clone();
        if let Some(menu) = menu {
            log(&handle, "native popup", "context menu open");
            let _ = main_window(&handle).popup_menu_at(&menu, LogicalPosition::new(x, y));
            log(&handle, "native popup", "context menu closed");
        }
    });
}

/// Mode B: opens title `index`'s menu as a native popup under it. Left and Right at the top level
/// close it and open the neighbor, as a menu bar does.
#[tauri::command]
fn open_bar_popup(app: AppHandle, index: usize, anchors: Vec<(f64, f64)>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let popups = handle.state::<Spike>().popups.lock().unwrap().clone();
        if popups.is_empty() || anchors.len() != popups.len() {
            return;
        }
        let window = main_window(&handle);
        let mut index = index.min(popups.len() - 1);
        loop {
            let _ = handle.emit("hybrid-open", index);
            log(&handle, "native popup", format!("bar menu {index} open"));
            let (x, y) = anchors[index];
            #[cfg(windows)]
            let direction = win::popup_with_arrows(&window, &popups[index], x, y);
            #[cfg(not(windows))]
            let direction = {
                let _ = window.popup_menu_at(&popups[index], LogicalPosition::new(x, y));
                0i32
            };
            log(
                &handle,
                "native popup",
                format!("bar menu {index} closed (arrow {direction})"),
            );
            if direction == 0 {
                break;
            }
            index = (index as i32 + direction).rem_euclid(popups.len() as i32) as usize;
        }
        let _ = handle.emit("hybrid-closed", ());
    });
}

fn parse_args() -> (Settings, Option<String>, Option<String>) {
    let mut settings = Settings {
        mode: Mode::Native,
        bridge: false,
        guard: false,
        browser_keys: true,
        theme: None,
    };
    let mut log_path = None;
    let mut selftest = None;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--mode" => {
                settings.mode = match value.as_str() {
                    "hybrid" | "b" | "B" => Mode::Hybrid,
                    "inpage" | "c" | "C" => Mode::Inpage,
                    _ => Mode::Native,
                };
                i += 1;
            }
            "--theme" => {
                settings.theme = match value.as_str() {
                    "dark" => Some(Theme::Dark),
                    "light" => Some(Theme::Light),
                    _ => None,
                };
                i += 1;
            }
            "--bridge" => settings.bridge = true,
            "--no-browser-keys" => settings.browser_keys = false,
            "--log" => {
                log_path = Some(value);
                i += 1;
            }
            "--selftest" => {
                selftest = Some(value);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    settings.guard = settings.mode != Mode::Native;
    (settings, log_path, selftest)
}

fn main() {
    let (settings, log_path, selftest) = parse_args();
    let theme = settings.theme;
    let log_file = log_path.and_then(|p| std::fs::File::create(p).ok());
    tauri::Builder::default()
        .manage(Spike {
            settings: Mutex::new(settings),
            bar: model::bar(),
            native_bar: Mutex::new(None),
            popups: Mutex::new(Vec::new()),
            context: Mutex::new(None),
            log_file: Mutex::new(log_file),
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_mode,
            set_flags,
            set_theme,
            page_log,
            page_click,
            show_context_menu,
            open_bar_popup
        ])
        .on_menu_event(|app, event| {
            log(app, "menu event", event.id().0.to_string());
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let spike = handle.state::<Spike>();

            // Mode A's bar registers the accelerators, as the app's bar does today.
            let tops: Vec<Submenu<Wry>> = spike
                .bar
                .iter()
                .map(|m| build_top(&handle, m, true))
                .collect::<tauri::Result<_>>()?;
            let refs: Vec<&dyn IsMenuItem<Wry>> = tops.iter().map(|t| t as &dyn IsMenuItem<Wry>).collect();
            *spike.native_bar.lock().unwrap() = Some(Menu::with_items(&handle, &refs)?);

            // Mode B's popups only SHOW keys: a popup's accelerators never fire anyway.
            *spike.popups.lock().unwrap() = spike
                .bar
                .iter()
                .map(|m| build_top(&handle, m, false))
                .collect::<tauri::Result<_>>()?;

            let context_rows = build_rows(&handle, &model::file_context_menu(), false)?;
            let refs: Vec<&dyn IsMenuItem<Wry>> = context_rows.iter().map(|c| c.as_ref()).collect();
            *spike.context.lock().unwrap() = Some(Menu::with_items(&handle, &refs)?);

            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Cmdr menu spike")
                .inner_size(1100.0, 760.0)
                .theme(theme)
                .build()?;
            let _ = window;

            #[cfg(windows)]
            win::install(&handle)?;

            apply_mode(&handle)?;

            if let Some(script) = selftest.clone() {
                #[cfg(windows)]
                win::run_selftest(handle.clone(), script);
                #[cfg(not(windows))]
                log(
                    &handle,
                    "setup",
                    format!("the selftest ({script}) runs on Windows only"),
                );
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the menu spike");
}
