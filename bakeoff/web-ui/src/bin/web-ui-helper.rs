//! The helper app CEF starts for renderer, GPU and utility processes.

use cef::args::Args;
use cef::*;

fn main() {
    let args = Args::new();

    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };

    // Helpers live in `Contents/Frameworks/<name> Helper.app/Contents/MacOS`,
    // so the loader is told it runs from a helper to find the framework.
    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), true);
        assert!(loader.load(), "could not load the CEF framework");
        loader
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let mut app = web_ui::helper_app();
    execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
}
