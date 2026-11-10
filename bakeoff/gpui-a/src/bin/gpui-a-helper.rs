//! The CEF subprocess (renderer, GPU, utility). On macOS bundle-cef-app
//! copies it into the bundle's helper apps; on Windows it sits next to
//! gpui-a.exe. It uses no GPUI code.

// Without it every renderer would open a console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use cef::{args::Args, *};

fn main() {
    let args = Args::new();

    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };

    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), true);
        assert!(loader.load());
        loader
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let code = execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    std::process::exit(code);
}
