//! jaal's UI library. The engine loads it in the browser process and calls
//! `jaal_ui_main` on Chromium's UI thread; GPUI then runs inside Chromium's
//! message loop (docs/engine.md › Process and startup).
//!
//! This first version is M2's layout test: does GPUI's DirectComposition
//! surface work in a child HWND of the engine's window, and does system blur
//! apply to a pop-up owned by it?

use jaal_engine::{ABI_MAJOR, sys};

#[cfg(target_os = "windows")]
mod layout_test;

#[unsafe(no_mangle)]
pub extern "C" fn jaal_ui_abi_major() -> u32 {
    ABI_MAJOR
}

/// # Safety
/// Called once by the engine on its UI thread, with its own `get_api`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jaal_ui_main(get_api: sys::jaal_engine_get_api_fn) {
    #[cfg(target_os = "windows")]
    // SAFETY: the engine passes its own get_api, on its UI thread.
    unsafe {
        layout_test::start(get_api)
    };
    #[cfg(not(target_os = "windows"))]
    {
        let _ = get_api;
        eprintln!("jaal-ui: the engine UI runs on Windows only so far");
    }
}
