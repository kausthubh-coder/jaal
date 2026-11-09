//! jaal's web-ui bake-off browser: all UI is HTML drawn by Chromium.

mod app;
mod bench;
#[cfg(target_os = "macos")]
mod mac;

use cef::*;

#[cfg(target_os = "macos")]
use mac::window_number;

#[cfg(not(target_os = "macos"))]
fn window_number(_handle: sys::cef_window_handle_t) -> Option<isize> {
    None
}

fn main() {
    bench::mark("main");

    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), false);
        assert!(loader.load(), "could not load the CEF framework");
        loader
    };
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    #[cfg(target_os = "macos")]
    mac::install_application();

    let args = args::Args::new();
    // Subprocesses run from the helper binary, so this is always the browser
    // process and execute_process returns -1 straight away.
    let code = execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    assert_eq!(code, -1, "web-ui was started as a subprocess");

    app::run(&args);
}
