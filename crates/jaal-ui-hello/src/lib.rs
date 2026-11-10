//! The smallest jaal UI library (milestone M1): opens one window with one tab
//! through the engine's C API, and quits when that window closes.
//!
//! For tests:
//! - JAAL_HELLO_URL: the page to open (default chrome://version).
//! - JAAL_HELLO_LOG: a file that gets one line per engine event.
//! - A page that sets its title to something starting with `jaal:quit` makes
//!   the app quit, so a test can start a quit from inside a page.

use std::io::Write;

use jaal_engine::{ABI_MAJOR, Engine, Event, sys};

#[unsafe(no_mangle)]
pub extern "C" fn jaal_ui_abi_major() -> u32 {
    if cfg!(feature = "wrong-abi") {
        ABI_MAJOR + 1
    } else {
        ABI_MAJOR
    }
}

fn log(line: &str) {
    if let Some(path) = std::env::var_os("JAAL_HELLO_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// # Safety
/// Called once by the engine on its UI thread, with its own `get_api`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jaal_ui_main(get_api: sys::jaal_engine_get_api_fn) {
    // SAFETY: the engine passes its own get_api, on its UI thread.
    let engine = match unsafe { Engine::from_get_api(get_api) } {
        Ok(engine) => engine,
        Err(error) => return log(&format!("no engine: {error}")),
    };
    let url = std::env::var("JAAL_HELLO_URL").unwrap_or_else(|_| "chrome://version".into());
    let window = engine.create_window().expect("create a window");
    let tab = engine.open_tab(window, &url, true).expect("open a tab");
    log(&format!("opened {window:?} {tab:?} {url}"));

    let mut last_quit_title = String::new();
    let on_event = move |event: Event| {
        log(&format!("{event:?}"));
        match event {
            Event::WindowClosed(closed) if closed == window => {
                let _ = engine.quit();
            }
            Event::TabState(state)
                if state.title.starts_with("jaal:quit") && state.title != last_quit_title =>
            {
                last_quit_title = state.title;
                let _ = engine.quit();
            }
            _ => {}
        }
    };
    engine.on_event(on_event).expect("set the event handler");
}
