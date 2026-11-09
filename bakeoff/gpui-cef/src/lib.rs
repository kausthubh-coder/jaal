//! Glue between GPUI and CEF on macOS, shared by the GPUI bake-off variants:
//! the NSApp protocol patch, the external message pump, the CEF client, CEF
//! browsers hosted as native child views of a GPUI window (gpui-a), and the
//! text field both variants draw (GPUI ships none).
//!
//! Startup order matters, see `start`.

#![cfg(target_os = "macos")]

pub mod gpui_patch;
pub mod nsapp;
pub mod posted_events;
pub mod pump;
pub mod text_input;

use cef::*;
use objc2::msg_send;
use objc2::runtime::{AnyObject, Bool};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::ffi::c_void;
use std::path::Path;
use std::rc::Rc;

pub use cef;

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
    /// A Cmd+letter key down that reached the page. The handler returns true
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

/// Returns true when it handled the event (only matters for `CommandKey` and
/// `Console`).
pub type EventHandler = Rc<dyn Fn(BrowserEvent) -> bool>;

/// Loads the framework, patches NSApp and GPUI's window classes, and
/// initializes CEF with the external pump. Call first thing in `main`, on the main thread, before
/// `gpui::Application::new()`. Keep the returned loader alive until exit.
///
/// The order is required: the protocols the NSApp patch adds live in the CEF
/// framework, and NSApp must be a `GPUIApplication` before either CEF or
/// GPUI creates it.
pub fn start(profile_dir: &Path) -> library_loader::LibraryLoader {
    let exe = std::env::current_exe().expect("current exe");
    let loader = library_loader::LibraryLoader::new(&exe, false);
    assert!(loader.load(), "could not load the CEF framework");
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    nsapp::install_cef_app_protocol();
    gpui_patch::fix_key_status_deadlock();

    let args = args::Args::new();
    // The main executable only ever runs as the browser process: the bundle's
    // helper apps run the others. execute_process returns -1 for it.
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
    loader
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
            if let Some(command_line) = command_line {
                // Without it Chromium asks the login keychain for its cookie
                // key, which opens a password dialog nobody can answer on an
                // unattended Mac. The bake-off keeps no secrets.
                command_line.append_switch(Some(&CefString::from("use-mock-keychain")));
            }
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

        fn on_cursor_change(&self, browser: Option<&mut Browser>, _cursor: *mut u8, type_: CursorType, _custom_cursor_info: Option<&CursorInfo>) -> ::std::os::raw::c_int {
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
            // send performClose: to it, which closes the whole GPUI window
            // for one tab. Returning 1 means we destroy the browser's view
            // ourselves: CEF calls on_before_close once that NSView is gone.
            let view = host.window_handle();
            if !view.is_null() {
                unsafe {
                    let _: () = msg_send![view.cast::<AnyObject>(), removeFromSuperview];
                }
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
        fn on_pre_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: *mut u8, _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            // While the page has keyboard focus its NSView swallows key
            // equivalents, so GPUI never sees Cmd+T. Hand them back here.
            let Some(event) = event else { return 0 };
            let command = event.modifiers & sys::cef_event_flags_t::EVENTFLAG_COMMAND_DOWN.0 != 0;
            if event.type_ != KeyEventType::RAWKEYDOWN || !command {
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

/// A windowed CEF browser whose NSView is a child of a GPUI window's content
/// view, placed above GPUI's own view.
#[derive(Clone)]
pub struct ChildBrowser {
    pub browser: Browser,
    view: *mut AnyObject,
}

impl ChildBrowser {
    /// `gpui_view` is the NSView from the GPUI window's raw window handle.
    /// The browser starts hidden; place it with `set_frame` and show it.
    pub fn create(gpui_view: *mut c_void, url: &str, client: &mut Client) -> Option<ChildBrowser> {
        let content_view = unsafe { content_view_of(gpui_view.cast()) };
        let window_info = WindowInfo {
            runtime_style: RuntimeStyle::ALLOY,
            ..Default::default()
        }
        .set_as_child(
            content_view.cast(),
            &Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(client),
            Some(&CefString::from(url)),
            Some(&BrowserSettings::default()),
            None,
            None,
        )?;
        let view: *mut AnyObject = browser.host()?.window_handle().cast();
        if view.is_null() {
            return None;
        }
        unsafe {
            // Width and height follow the window, margins stay fixed. During a
            // live resize AppKit then resizes the page in the same pass as the
            // window, before GPUI's next frame catches up with set_frame.
            let _: () = msg_send![view, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
            let _: () = msg_send![view, setHidden: Bool::YES];
        }
        Some(ChildBrowser { browser, view })
    }

    pub fn id(&self) -> i32 {
        self.browser.identifier()
    }

    /// Places the page at `x, y, width, height` in GPUI's window coordinates
    /// (points, origin top left).
    pub fn set_frame(&self, x: f64, y: f64, width: f64, height: f64) {
        unsafe {
            let superview: *mut AnyObject = msg_send![self.view, superview];
            if superview.is_null() {
                return;
            }
            // The content view isn't flipped: AppKit's origin is bottom left.
            let parent: NSRect = msg_send![superview, bounds];
            let frame = NSRect::new(
                NSPoint::new(x, parent.size.height - y - height),
                NSSize::new(width, height),
            );
            let current: NSRect = msg_send![self.view, frame];
            if current != frame {
                let _: () = msg_send![self.view, setFrame: frame];
            }
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _: () = msg_send![self.view, setHidden: Bool::new(!visible)];
        }
        if visible && let Some(host) = self.browser.host() {
            host.set_focus(1);
        }
    }

    pub fn load_url(&self, url: &str) {
        if let Some(frame) = self.browser.main_frame() {
            frame.load_url(Some(&CefString::from(url)));
        }
    }

    pub fn run_js(&self, code: &str) {
        if let Some(frame) = self.browser.main_frame() {
            frame.execute_java_script(Some(&CefString::from(code)), None, 0);
        }
    }

    /// Starts closing; `BeforeClose` follows once CEF is done.
    pub fn close(&self) {
        unsafe {
            let _: () = msg_send![self.view, setHidden: Bool::YES];
        }
        if let Some(host) = self.browser.host() {
            host.close_browser(1);
        }
    }
}

const NS_VIEW_WIDTH_SIZABLE: usize = 2;
const NS_VIEW_HEIGHT_SIZABLE: usize = 16;

unsafe fn content_view_of(view: *mut AnyObject) -> *mut AnyObject {
    unsafe {
        let window = ns_window_of(view.cast());
        msg_send![window, contentView]
    }
}

/// The NSWindow of a GPUI window's NSView.
///
/// # Safety
/// `gpui_view` must be a live NSView.
pub unsafe fn ns_window_of(gpui_view: *mut c_void) -> *mut AnyObject {
    unsafe { msg_send![gpui_view.cast::<AnyObject>(), window] }
}

/// The window's CGWindowID, what `screencapture -l` takes.
///
/// # Safety
/// `gpui_view` must be a live NSView in a window.
pub unsafe fn window_number(gpui_view: *mut c_void) -> isize {
    unsafe { msg_send![ns_window_of(gpui_view), windowNumber] }
}
