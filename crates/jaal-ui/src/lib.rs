//! jaal's UI library. The engine loads it in the browser process and calls
//! `jaal_ui_main` on Chromium's UI thread. GPUI then runs inside Chromium's
//! message loop (docs/engine.md › Process and startup): the engine owns each
//! window and its page, and GPUI draws the sidebar and the command bar in
//! child windows the engine keeps placed.
//!
//! What a launch can ask for, through the environment:
//! - `JAAL_UI_URL`: the tabs to open, separated by `|` (default about:blank).
//! - `JAAL_UI_FAKE_ROWS`: how many of the bake-off's fake rows to list in the
//!   sidebar, for the scroll test (default none).
//! - `JAAL_UI_THEME`: `light` or `dark` (default: the system's).
//! - `JAAL_UI_FRAME=system`: keep the system title bar.
//! - `JAAL_UI_LOG`: a file that gets one line per engine event and UI action.
//! - `JAAL_BENCH_LOG`, `JAAL_BENCH_FRAMES`, `JAAL_BENCH_SCROLL`: the
//!   bake-off's bench marks and its scroll run (bakeoff/shared), which
//!   engine/scripts/bench.ts reads.

pub mod command_bar;
pub mod icons;
pub mod shell;
pub mod text_input;
pub mod theme;

use jaal_engine::{ABI_MAJOR, sys};
use std::ffi::c_void;
use std::io::Write;

#[unsafe(no_mangle)]
pub extern "C" fn jaal_ui_abi_major() -> u32 {
    ABI_MAJOR
}

pub(crate) fn log(line: &str) {
    if let Some(path) = std::env::var_os("JAAL_UI_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// A GPUI window's native view: its HWND on Windows, its NSView on macOS.
/// The engine takes it in `attach_view`.
pub(crate) fn native_view(window: &gpui::Window) -> jaal_engine::NativeView {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    jaal_engine::NativeView::from_raw(
        match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as *mut c_void,
            Ok(RawWindowHandle::AppKit(handle)) => handle.ns_view.as_ptr(),
            other => panic!("unexpected GPUI window handle: {other:?}"),
        },
    )
}

fn config_from_env() -> shell::Config {
    let urls: Vec<String> = std::env::var("JAAL_UI_URL")
        .unwrap_or_else(|_| "about:blank".into())
        .split('|')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(String::from)
        .collect();
    shell::Config {
        urls,
        fake_rows: std::env::var("JAAL_UI_FAKE_ROWS")
            .ok()
            .and_then(|rows| rows.parse().ok())
            .unwrap_or(0),
        dark: match std::env::var("JAAL_UI_THEME").as_deref() {
            Ok("dark") => Some(true),
            Ok("light") => Some(false),
            _ => None,
        },
        frameless: std::env::var("JAAL_UI_FRAME").as_deref() != Ok("system"),
        bench: jaal_bakeoff_shared::bench_modes::BenchConfig::from_env(),
    }
}

/// # Safety
/// Called once by the engine on its UI thread, with its own `get_api`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jaal_ui_main(get_api: sys::jaal_engine_get_api_fn) {
    use gpui::AppContext as _;
    use jaal_engine::{Engine, WindowOptions};

    jaal_bakeoff_shared::bench::mark("ui_main");

    // SAFETY: the engine passes its own get_api, on its UI thread.
    let engine = match unsafe { Engine::from_get_api(get_api) } {
        Ok(engine) => engine,
        Err(error) => return log(&format!("no engine: {error}")),
    };
    let config = config_from_env();

    // Engine events arrive from Chromium's loop, outside any GPUI update.
    // The sidebar reads them from this channel.
    let (sender, receiver) = futures::channel::mpsc::unbounded();
    let window = engine
        .create_window_with(WindowOptions {
            frameless: config.frameless,
        })
        .expect("create a window");
    engine
        .on_event(move |event| {
            log(&format!("{event:?}"));
            // Handled here, not in the sidebar: the sidebar is a child of
            // the window and is gone by the time the window has closed.
            if event == jaal_engine::Event::WindowClosed(window) {
                let _ = engine.quit();
            }
            let _ = sender.unbounded_send(event);
        })
        .expect("set the event handler");
    engine
        .set_shortcut_filter(shell::is_reserved_shortcut)
        .expect("set the shortcut filter");
    for (index, url) in config.urls.iter().enumerate() {
        if let Err(error) = engine.open_tab(window, url, index == 0) {
            log(&format!("could not open {url}: {error}"));
        }
    }

    gpui::Application::new()
        .with_assets(icons::Icons)
        .run_embedded(move |cx| {
            text_input::bind_keys(cx);
            shell::bind_keys(cx);
            command_bar::bind_keys(cx);
            cx.open_window(
                command_bar::child_window_options(theme::SIDEBAR_WIDTH, 600),
                |gpui_window, cx| {
                    cx.new(|cx| {
                        shell::Shell::new(engine, window, &config, receiver, gpui_window, cx)
                    })
                },
            )
            .expect("open the sidebar");
        });
    log("gpui started in embedded mode");
}
