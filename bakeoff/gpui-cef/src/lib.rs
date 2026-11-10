//! Glue between GPUI and CEF, shared by the GPUI bake-off variants: the
//! external message pump, the CEF client, CEF browsers hosted as native
//! child views of a GPUI window (gpui-a), and the text field both variants
//! draw (GPUI ships none). On macOS also the NSApp protocol patch and a GPUI
//! deadlock fix.
//!
//! macOS and Windows. Startup order matters, see `start`.

#![cfg(any(target_os = "macos", target_os = "windows"))]

#[cfg(target_os = "macos")]
mod child_mac;
#[cfg(target_os = "windows")]
mod child_win;
#[cfg(target_os = "macos")]
pub mod gpui_patch;
#[cfg(target_os = "macos")]
pub mod nsapp;
#[cfg(target_os = "macos")]
pub mod posted_events;
pub mod pump;
pub mod text_input;

#[cfg(target_os = "macos")]
pub use child_mac::*;
#[cfg(target_os = "windows")]
pub use child_win::*;

use cef::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::path::Path;
use std::rc::Rc;

pub use cef;
#[cfg(target_os = "windows")]
pub use pump::run_after_gpui;

/// The native view of a GPUI window: its NSView on macOS, its HWND on
/// Windows. CEF child browsers and the gpui-a overlay take it.
pub fn native_view(window: &gpui::Window) -> *mut c_void {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        #[cfg(target_os = "macos")]
        Ok(RawWindowHandle::AppKit(handle)) => handle.ns_view.as_ptr(),
        #[cfg(target_os = "windows")]
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as *mut c_void,
        other => panic!("unexpected GPUI window handle: {other:?}"),
    }
}

/// What a browser reports back to the UI. Every event is delivered on the
/// main thread, from inside `cef::do_message_loop_work`, so the handler must
/// not re-enter GPUI's `App` directly; queue the event instead.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserEvent {
    LoadEnd {
        browser_id: i32,
        main_frame: bool,
    },
    AddressChange {
        browser_id: i32,
        url: String,
    },
    TitleChange {
        browser_id: i32,
        title: String,
    },
    BeforeClose {
        browser_id: i32,
    },
    /// A `console.log` from the page. Handlers that return true keep it out
    /// of CEF's log.
    Console {
        browser_id: i32,
        message: String,
    },
    /// A Cmd+letter (Ctrl+letter on Windows) key down that reached the page. The handler returns true
    /// to take it as an app shortcut, so the page never sees it.
    CommandKey {
        browser_id: i32,
        key: char,
    },
    /// The page wants a different mouse cursor. Windowed browsers set it
    /// themselves when the handler returns false; windowless ones need the
    /// UI to show it.
    CursorChange {
        browser_id: i32,
        cursor: CursorType,
    },
}

// The cef crate types these callback arguments per platform.
#[cfg(target_os = "macos")]
type NativeCursor = *mut u8;
#[cfg(target_os = "windows")]
type NativeCursor = *mut sys::HICON__;
#[cfg(target_os = "macos")]
type NativeKeyEvent<'a> = *mut u8;
#[cfg(target_os = "windows")]
type NativeKeyEvent<'a> = Option<&'a mut sys::tagMSG>;

/// Returns true when it handled the event (only matters for `CommandKey` and
/// `Console`).
pub type EventHandler = Rc<dyn Fn(BrowserEvent) -> bool>;

/// Keeps what `start` loaded alive. Drop it only at exit.
pub struct Started {
    #[cfg(target_os = "macos")]
    _loader: library_loader::LibraryLoader,
}

/// Loads CEF, applies the platform patches, and initializes CEF with the
/// external pump. Call first thing in `main`, on the main thread, before
/// `gpui::Application::new()`. Keep the result alive until exit.
///
/// On macOS the order is required: the protocols the NSApp patch adds live
/// in the CEF framework, and NSApp must be a `GPUIApplication` before either
/// CEF or GPUI creates it.
pub fn start(profile_dir: &Path) -> Started {
    #[cfg(target_os = "macos")]
    let loader = {
        let exe = std::env::current_exe().expect("current exe");
        let loader = library_loader::LibraryLoader::new(&exe, false);
        assert!(loader.load(), "could not load the CEF framework");
        loader
    };
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    #[cfg(target_os = "macos")]
    {
        nsapp::install_cef_app_protocol();
        gpui_patch::fix_key_status_deadlock();
    }

    let args = args::Args::new();
    // The main executable only ever runs as the browser process: the helpers
    // run the others. execute_process returns -1 for it.
    let code = execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    assert_eq!(code, -1, "main executable started as a CEF subprocess");

    std::fs::create_dir_all(profile_dir).expect("create profile dir");
    let profile = CefString::from(profile_dir.to_str().expect("utf-8 profile path"));
    let log_file = CefString::from(profile_dir.join("cef.log").to_str().unwrap());
    let settings = Settings {
        external_message_pump: 1,
        multi_threaded_message_loop: 0,
        root_cache_path: profile.clone(),
        cache_path: profile,
        log_file,
        log_severity: LogSeverity::WARNING,
        // On macOS the bundle's helper apps run the subprocesses. On Windows
        // there is no bundle: `<exe>-helper.exe` sits next to the exe, so
        // renderers don't load GPUI. The sandbox needs CEF's bootstrap.exe
        // layout, which the bake-off doesn't use.
        #[cfg(target_os = "windows")]
        browser_subprocess_path: CefString::from(helper_path().to_str().expect("utf-8 exe path")),
        #[cfg(target_os = "windows")]
        no_sandbox: 1,
        ..Default::default()
    };
    let mut app = PumpApp::new();
    let ok = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    assert_eq!(
        ok,
        1,
        "cef::initialize failed; see {}",
        profile_dir.display()
    );
    pump::start();
    Started {
        #[cfg(target_os = "macos")]
        _loader: loader,
    }
}

/// `gpui-a.exe` runs `gpui-a-helper.exe` from the same folder.
#[cfg(target_os = "windows")]
fn helper_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let stem = exe.file_stem().expect("exe name").to_string_lossy();
    let helper = exe.with_file_name(format!("{stem}-helper.exe"));
    assert!(helper.exists(), "missing {}", helper.display());
    helper
}

/// Stops the pump and shuts CEF down. Close every browser and wait for their
/// `BeforeClose` first; CEF asserts otherwise. Call from a fresh main-queue
/// task, not from inside a CEF callback.
pub fn shutdown() {
    pump::stop();
    cef::shutdown();
}

wrap_app! {
    struct PumpApp;

    impl App {
        fn on_before_command_line_processing(&self, _process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            let Some(command_line) = command_line else { return };
            // Without it Chromium asks the login keychain for its cookie
            // key, which opens a password dialog nobody can answer on an
            // unattended Mac. The bake-off keeps no secrets.
            #[cfg(target_os = "macos")]
            command_line.append_switch(Some(&CefString::from("use-mock-keychain")));
            // Windows needs no keychain switch: Chromium keeps its cookie key
            // with DPAPI, which never prompts. What can prompt is Windows
            // Firewall, the first time the Media Router's cast discovery
            // listens for mDNS. The bake-off casts nothing.
            #[cfg(target_os = "windows")]
            command_line.append_switch_with_value(
                Some(&CefString::from("disable-features")),
                Some(&CefString::from("MediaRouter")),
            );
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(PumpBrowserProcessHandler::new())
        }
    }
}

wrap_browser_process_handler! {
    struct PumpBrowserProcessHandler;

    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            pump::schedule(delay_ms);
        }
    }
}

/// A CEF client that turns the callbacks the bake-off needs into
/// `BrowserEvent`s.
pub fn client(handler: EventHandler) -> Client {
    EventClient::new(handler, None)
}

/// The same client for a windowless browser, which also needs a render
/// handler: CEF asks it for the view size and hands it the painted frames.
pub fn windowless_client(handler: EventHandler, render_handler: RenderHandler) -> Client {
    EventClient::new(handler, Some(render_handler))
}

wrap_client! {
    struct EventClient {
        handler: EventHandler,
        render_handler: Option<RenderHandler>,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            self.render_handler.clone()
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(EventDisplayHandler::new(self.handler.clone()))
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(EventLifeSpanHandler::new(self.handler.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(EventLoadHandler::new(self.handler.clone()))
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(EventKeyboardHandler::new(self.handler.clone()))
        }
    }
}

fn browser_id(browser: Option<&mut Browser>) -> i32 {
    browser.map_or(-1, |b| b.identifier())
}

wrap_display_handler! {
    struct EventDisplayHandler {
        handler: EventHandler,
    }

    impl DisplayHandler {
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.is_some_and(|f| f.is_main() == 1) {
                let url = url.map(CefString::to_string).unwrap_or_default();
                (self.handler)(BrowserEvent::AddressChange { browser_id: browser_id(browser), url });
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let title = title.map(CefString::to_string).unwrap_or_default();
            (self.handler)(BrowserEvent::TitleChange { browser_id: browser_id(browser), title });
        }

        fn on_cursor_change(&self, browser: Option<&mut Browser>, _cursor: NativeCursor, type_: CursorType, _custom_cursor_info: Option<&CursorInfo>) -> ::std::os::raw::c_int {
            (self.handler)(BrowserEvent::CursorChange { browser_id: browser_id(browser), cursor: type_ }) as i32
        }

        fn on_console_message(&self, browser: Option<&mut Browser>, _level: LogSeverity, message: Option<&CefString>, _source: Option<&CefString>, _line: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let message = message.map(CefString::to_string).unwrap_or_default();
            (self.handler)(BrowserEvent::Console { browser_id: browser_id(browser), message }) as i32
        }
    }
}

wrap_life_span_handler! {
    struct EventLifeSpanHandler {
        handler: EventHandler,
    }

    impl LifeSpanHandler {
        fn do_close(&self, browser: Option<&mut Browser>) -> ::std::os::raw::c_int {
            // A windowless browser has no window or view to close, so CEF
            // can go straight on to on_before_close. (Its window_handle is
            // the parent view it was given, GPUI's, which must stay.)
            let Some(host) = browser.and_then(|b| b.host()) else { return 0 };
            if host.is_window_rendering_disabled() != 0 {
                return 0;
            }
            // A windowed browser: we own the window. Returning 0 lets CEF
            // close the top-level window (performClose: on macOS, WM_CLOSE on
            // Windows), which is the whole GPUI window, for one tab.
            // Returning 1 means we destroy the browser's view ourselves: CEF
            // calls on_before_close once it is gone.
            let view = native_handle(host.window_handle());
            if !view.is_null() {
                unsafe { destroy_browser_view(view) };
            }
            1
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            (self.handler)(BrowserEvent::BeforeClose { browser_id: browser_id(browser) });
        }
    }
}

wrap_load_handler! {
    struct EventLoadHandler {
        handler: EventHandler,
    }

    impl LoadHandler {
        fn on_load_end(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, _http_status_code: ::std::os::raw::c_int) {
            let main_frame = frame.is_some_and(|f| f.is_main() == 1);
            (self.handler)(BrowserEvent::LoadEnd { browser_id: browser_id(browser), main_frame });
        }
    }
}

wrap_keyboard_handler! {
    struct EventKeyboardHandler {
        handler: EventHandler,
    }

    impl KeyboardHandler {
        fn on_pre_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: NativeKeyEvent<'_>, _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            // While the page has keyboard focus its native view takes every
            // key, so GPUI never sees Cmd+T (Ctrl+T on Windows). Hand them
            // back here.
            let Some(event) = event else { return 0 };
            if event.type_ != KeyEventType::RAWKEYDOWN || !is_shortcut_modifier(event.modifiers) {
                return 0;
            }
            let Some(key) = char::from_u32(event.windows_key_code as u32).filter(char::is_ascii_alphanumeric) else {
                return 0;
            };
            let key = key.to_ascii_lowercase();
            (self.handler)(BrowserEvent::CommandKey { browser_id: browser_id(browser), key }) as i32
        }
    }
}

/// Cmd on macOS. Ctrl on Windows, but not with Alt: AltGr arrives as
/// Ctrl+Alt and types characters on many keyboard layouts.
fn is_shortcut_modifier(modifiers: u32) -> bool {
    use sys::cef_event_flags_t as flags;
    // The flags are u32 on macOS and i32 on Windows.
    let has = |flag: flags| i64::from(modifiers) & i64::from(flag.0) != 0;
    if cfg!(target_os = "macos") {
        has(flags::EVENTFLAG_COMMAND_DOWN)
    } else {
        has(flags::EVENTFLAG_CONTROL_DOWN) && !has(flags::EVENTFLAG_ALT_DOWN)
    }
}
