//! Safe Rust API over `jaal_engine.h`, the engine's C API.
//!
//! The UI library gets an [`Engine`] from the `get_api` function the engine
//! passes to `jaal_ui_main`. Every call runs on the engine's UI thread (the
//! main thread), and events arrive later from the top of the loop, never from
//! inside a call. See docs/engine.md › The C API.

use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt;
use std::marker::PhantomData;

pub use jaal_engine_sys as sys;

/// The engine ABI this crate was built against.
pub const ABI_MAJOR: u32 = sys::JAAL_ENGINE_ABI_MAJOR;

/// Why a call failed. The engine never reports an unfinished feature as an
/// empty success; it answers [`Error::Unsupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    /// The window or tab is gone.
    Gone,
    Cancelled,
    Unsupported,
    Denied,
    Internal,
    /// A status this build doesn't know, from a newer engine.
    Unknown(i32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for Error {}

fn check(status: sys::jaal_status) -> Result<(), Error> {
    match status {
        sys::jaal_status::JAAL_OK => Ok(()),
        sys::jaal_status::JAAL_ERR_INVALID => Err(Error::Invalid),
        sys::jaal_status::JAAL_ERR_GONE => Err(Error::Gone),
        sys::jaal_status::JAAL_ERR_CANCELLED => Err(Error::Cancelled),
        sys::jaal_status::JAAL_ERR_UNSUPPORTED => Err(Error::Unsupported),
        sys::jaal_status::JAAL_ERR_DENIED => Err(Error::Denied),
        sys::jaal_status::JAAL_ERR_INTERNAL => Err(Error::Internal),
        other => Err(Error::Unknown(other.0)),
    }
}

/// A browser window. Stale after its [`Event::WindowClosed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Window(sys::jaal_handle);

/// A tab. Stale after its [`Event::TabRemoved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tab(sys::jaal_handle);

impl Tab {
    /// A tab from the engine's handle for it. A handle the engine never gave
    /// out names nothing: calls with it answer [`Error::Gone`].
    pub fn from_raw(handle: sys::jaal_handle) -> Self {
        Self(handle)
    }

    pub fn raw(self) -> sys::jaal_handle {
        self.0
    }
}

/// Rectangle in a window's client area, in DIPs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Where something sits in a window's client area, in DIPs, like CSS absolute
/// positioning. Each edge is a distance from that edge of the client area.
/// On each axis:
/// - two edges: the box stretches between them;
/// - one edge and a size: the box keeps that size at that edge;
/// - both edges and a size: the box keeps that size, centered between them;
/// - a size alone: centered in the client area.
///
/// The engine applies it again on every resize, in the same layout pass as
/// the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Placement {
    pub left: Option<i32>,
    pub top: Option<i32>,
    pub right: Option<i32>,
    pub bottom: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

impl Placement {
    /// The whole client area, shrunk by these distances from each edge.
    pub fn inset(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            right: Some(right),
            bottom: Some(bottom),
            ..Self::default()
        }
    }

    fn to_raw(self) -> sys::jaal_placement {
        // JAAL_AUTO in jaal_engine.h, which bindgen can't evaluate.
        let raw = |value: Option<i32>| value.unwrap_or(i32::MIN);
        sys::jaal_placement {
            left: raw(self.left),
            top: raw(self.top),
            right: raw(self.right),
            bottom: raw(self.bottom),
            width: raw(self.width),
            height: raw(self.height),
        }
    }
}

/// One of the app's own native views: an HWND on Windows, an NSView on the
/// Mac. The engine checks that it is a live view of the UI thread before it
/// uses it, so a wrong handle is refused, not followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeView(*mut c_void);

impl NativeView {
    pub fn from_raw(handle: *mut c_void) -> Self {
        Self(handle)
    }

    pub fn as_ptr(self) -> *mut c_void {
        self.0
    }
}

/// How one of the app's native views sits in an engine window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOptions {
    pub placement: Placement,
    pub visible: bool,
    /// For a see-through view over the page: the engine blurs what is under
    /// it with this Gaussian sigma, in DIPs. 0 is no blur.
    pub backdrop_blur: f32,
    /// The blurred area is the view's rectangle shrunk by this many DIPs on
    /// every side, so the view can draw its own shadow around it.
    pub backdrop_inset: i32,
    pub backdrop_corner_radius: f32,
}

impl ViewOptions {
    /// An opaque view: shown, no blur under it.
    pub fn opaque(placement: Placement) -> Self {
        Self {
            placement,
            visible: true,
            backdrop_blur: 0.,
            backdrop_inset: 0,
            backdrop_corner_radius: 0.,
        }
    }

    fn to_raw(self) -> sys::jaal_view_options {
        sys::jaal_view_options {
            struct_size: std::mem::size_of::<sys::jaal_view_options>() as u32,
            placement: self.placement.to_raw(),
            visible: self.visible as u8,
            backdrop_blur: self.backdrop_blur,
            backdrop_inset: self.backdrop_inset,
            backdrop_corner_radius: self.backdrop_corner_radius,
        }
    }
}

/// How a new window is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowOptions {
    /// No system title bar: the app draws its own minimize, maximize and
    /// close buttons and starts window drags with
    /// [`Engine::begin_window_drag`]. The window keeps the system's resize
    /// borders, shadow, rounded corners and snapping.
    pub frameless: bool,
}

/// What the engine paints in a window's client area around the page. Colors
/// are `0xAARRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowStyle {
    /// Behind the page and the app's views: a top-to-bottom gradient.
    pub background_top: u32,
    pub background_bottom: u32,
    /// The page's corners, in DIPs.
    pub page_corner_radius: f32,
    /// A soft shadow under the page. Zero alpha draws none.
    pub page_shadow_color: u32,
    pub page_shadow_blur: f32,
    pub page_shadow_offset_y: f32,
    /// A hairline around the page. Zero alpha draws none.
    pub page_outline_color: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

impl WindowState {
    fn to_raw(self) -> sys::jaal_window_state {
        match self {
            WindowState::Normal => sys::jaal_window_state::JAAL_WINDOW_NORMAL,
            WindowState::Minimized => sys::jaal_window_state::JAAL_WINDOW_MINIMIZED,
            WindowState::Maximized => sys::jaal_window_state::JAAL_WINDOW_MAXIMIZED,
            WindowState::Fullscreen => sys::jaal_window_state::JAAL_WINDOW_FULLSCREEN,
        }
    }

    fn from_raw(state: sys::jaal_window_state) -> Self {
        match state {
            sys::jaal_window_state::JAAL_WINDOW_MINIMIZED => WindowState::Minimized,
            sys::jaal_window_state::JAAL_WINDOW_MAXIMIZED => WindowState::Maximized,
            sys::jaal_window_state::JAAL_WINDOW_FULLSCREEN => WindowState::Fullscreen,
            _ => WindowState::Normal,
        }
    }
}

/// A key: a Windows virtual-key code and which modifiers were down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: u32,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Key {
    fn from_raw(code: u32, modifiers: u32) -> Self {
        Self {
            code,
            shift: modifiers & sys::JAAL_MOD_SHIFT != 0,
            ctrl: modifiers & sys::JAAL_MOD_CTRL != 0,
            alt: modifiers & sys::JAAL_MOD_ALT != 0,
            meta: modifiers & sys::JAAL_MOD_META != 0,
        }
    }
}

/// A window's size and state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowInfo {
    pub window: Window,
    pub state: WindowState,
    /// Whether it is the active window.
    pub active: bool,
    /// The client area, in DIPs.
    pub client_width: i32,
    pub client_height: i32,
    /// Physical pixels per DIP.
    pub scale: f32,
}

/// What a tab shows now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabState {
    pub window: Option<Window>,
    pub tab: Tab,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub active: bool,
    /// Whether the tab has history to go back or forward to.
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// What [`Engine::navigate`] does to a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    Back,
    Forward,
    Reload,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Started,
    /// Every window passed its beforeunload checks; the process is ending.
    WillQuit,
    /// A page's beforeunload cancelled the quit.
    QuitCancelled,
    WindowClosed(Window),
    TabInserted(TabState),
    TabRemoved(Tab),
    TabState(TabState),
    /// A key went down while a page had focus. `reserved` keys are the ones
    /// the shortcut filter claimed, which the page never saw; the others are
    /// keys the page left unhandled.
    Key {
        window: Option<Window>,
        key: Key,
        reserved: bool,
    },
    /// The window's size, state or activation changed. Also arrives once
    /// after `create_window`.
    WindowState(WindowInfo),
    /// A kind this build doesn't know, from a newer engine.
    Unknown(i32),
}

fn handle_or_none(handle: sys::jaal_handle) -> Option<sys::jaal_handle> {
    (handle.slot != 0).then_some(handle)
}

/// Copies an event out of the engine's borrowed strings.
///
/// # Safety
/// `event` must point to a valid `jaal_event` whose strings are null or valid
/// NUL-terminated UTF-8 for the duration of the call.
pub unsafe fn event_from_raw(event: *const sys::jaal_event) -> Event {
    // SAFETY: the caller guarantees `event` is valid for this call.
    let event = unsafe { &*event };
    let text = |ptr: *const c_char| -> String {
        if ptr.is_null() {
            String::new()
        } else {
            // SAFETY: non-null strings are NUL-terminated per the header.
            unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    };
    let has_1_1_fields = event.struct_size as usize >= std::mem::size_of::<sys::jaal_event>();
    let state = || TabState {
        window: handle_or_none(event.window).map(Window),
        tab: Tab(event.tab),
        url: text(event.url),
        title: text(event.title),
        loading: event.loading != 0,
        active: event.active != 0,
        can_go_back: has_1_1_fields && event.can_go_back != 0,
        can_go_forward: has_1_1_fields && event.can_go_forward != 0,
    };
    match event.kind {
        sys::jaal_event_kind::JAAL_EVENT_STARTED => Event::Started,
        sys::jaal_event_kind::JAAL_EVENT_WILL_QUIT => Event::WillQuit,
        sys::jaal_event_kind::JAAL_EVENT_QUIT_CANCELLED => Event::QuitCancelled,
        sys::jaal_event_kind::JAAL_EVENT_WINDOW_CLOSED => Event::WindowClosed(Window(event.window)),
        sys::jaal_event_kind::JAAL_EVENT_TAB_INSERTED => Event::TabInserted(state()),
        sys::jaal_event_kind::JAAL_EVENT_TAB_REMOVED => Event::TabRemoved(Tab(event.tab)),
        sys::jaal_event_kind::JAAL_EVENT_TAB_STATE => Event::TabState(state()),
        // The fields these two read were added in 1.1; an event from an
        // older engine is too short to hold them.
        sys::jaal_event_kind::JAAL_EVENT_KEY if has_1_1_fields => Event::Key {
            window: handle_or_none(event.window).map(Window),
            key: Key::from_raw(event.key_code, event.modifiers),
            reserved: event.key_reserved != 0,
        },
        sys::jaal_event_kind::JAAL_EVENT_WINDOW_STATE if has_1_1_fields => {
            Event::WindowState(WindowInfo {
                window: Window(event.window),
                state: WindowState::from_raw(event.window_state),
                active: event.window_active != 0,
                client_width: event.client_width,
                client_height: event.client_height,
                scale: event.scale,
            })
        }
        other => Event::Unknown(other.0),
    }
}

/// The engine, as seen from the UI library. Cheap to copy; all copies talk to
/// the same engine.
///
/// Every call must run on the engine's UI thread, so an `Engine` can't be
/// moved to another thread or shared with one:
///
/// ```compile_fail,E0277
/// fn call_from_a_worker(engine: jaal_engine::Engine) {
///     std::thread::spawn(move || engine.quit());
/// }
/// ```
///
/// ```compile_fail,E0277
/// fn share_with_a_worker(engine: &'static jaal_engine::Engine) {
///     std::thread::spawn(move || engine.quit());
/// }
/// ```
#[derive(Clone, Copy)]
pub struct Engine {
    api: &'static sys::jaal_engine_api,
    // A raw pointer is neither Send nor Sync, and so is anything holding one.
    // Without it the reference above would make Engine both, and safe code
    // could call the engine from a worker thread.
    ui_thread_only: PhantomData<*const ()>,
}

impl Engine {
    /// Asks the engine for its call table.
    ///
    /// Returns [`Error::Unsupported`] when the engine's ABI major differs from
    /// [`ABI_MAJOR`].
    ///
    /// # Safety
    /// `get_api` must be the function the engine passed to `jaal_ui_main`, and
    /// this must run on the engine's UI thread.
    pub unsafe fn from_get_api(get_api: sys::jaal_engine_get_api_fn) -> Result<Self, Error> {
        let get_api = get_api.ok_or(Error::Invalid)?;
        let size = std::mem::size_of::<sys::jaal_engine_api>() as u32;
        // SAFETY: the engine guarantees get_api is callable on this thread.
        let api = unsafe { get_api(ABI_MAJOR, size) };
        if api.is_null() {
            return Err(Error::Unsupported);
        }
        // SAFETY: the engine's table lives for the whole process.
        let api = unsafe { &*api };
        if api.abi_major != ABI_MAJOR
            || (api.struct_size as usize) < std::mem::size_of::<sys::jaal_engine_api>()
        {
            return Err(Error::Unsupported);
        }
        Ok(Self {
            api,
            ui_thread_only: PhantomData,
        })
    }

    /// Routes every engine event to `on_event`, replacing any earlier handler.
    /// The handler lives until the process ends.
    pub fn on_event(&self, on_event: impl FnMut(Event) + 'static) -> Result<(), Error> {
        type Handler = Box<dyn FnMut(Event)>;
        unsafe extern "C" fn trampoline(user_data: *mut c_void, event: *const sys::jaal_event) {
            // SAFETY: user_data is the leaked Handler below; the engine calls
            // this on the UI thread only, never re-entrantly.
            let handler = unsafe { &mut *(user_data as *mut Handler) };
            // SAFETY: the engine passes a valid event for this call.
            handler(unsafe { event_from_raw(event) });
        }
        // Leaked on purpose: the engine may deliver events until the process
        // ends, and a later on_event() replaces the sink but not the memory.
        let user_data = Box::into_raw(Box::new(Box::new(on_event) as Handler)) as *mut c_void;
        let set = self.api.set_event_sink.ok_or(Error::Unsupported)?;
        // SAFETY: trampoline matches jaal_event_sink and user_data outlives it.
        check(unsafe { set(Some(trampoline), user_data) })
    }

    /// Starts quitting. Ends in [`Event::WillQuit`] or [`Event::QuitCancelled`].
    pub fn quit(&self) -> Result<(), Error> {
        let quit = self.api.quit.ok_or(Error::Unsupported)?;
        // SAFETY: no arguments.
        check(unsafe { quit() })
    }

    pub fn create_window(&self) -> Result<Window, Error> {
        let create = self.api.window_create.ok_or(Error::Unsupported)?;
        let mut out = sys::jaal_handle::default();
        // SAFETY: out is a valid pointer for the call.
        check(unsafe { create(&mut out) })?;
        Ok(Window(out))
    }

    pub fn create_window_with(&self, options: WindowOptions) -> Result<Window, Error> {
        let create = self.api.window_create_with.ok_or(Error::Unsupported)?;
        let options = sys::jaal_window_options {
            struct_size: std::mem::size_of::<sys::jaal_window_options>() as u32,
            frameless: options.frameless as u8,
        };
        let mut out = sys::jaal_handle::default();
        // SAFETY: both pointers are valid for the call.
        check(unsafe { create(&options, &mut out) })?;
        Ok(Window(out))
    }

    pub fn set_window_style(&self, window: Window, style: WindowStyle) -> Result<(), Error> {
        let set = self.api.window_set_style.ok_or(Error::Unsupported)?;
        let style = sys::jaal_window_style {
            struct_size: std::mem::size_of::<sys::jaal_window_style>() as u32,
            background_top: style.background_top,
            background_bottom: style.background_bottom,
            page_corner_radius: style.page_corner_radius,
            page_shadow_color: style.page_shadow_color,
            page_shadow_blur: style.page_shadow_blur,
            page_shadow_offset_y: style.page_shadow_offset_y,
            page_outline_color: style.page_outline_color,
        };
        // SAFETY: style is valid for the call.
        check(unsafe { set(window.0, &style) })
    }

    /// Starts moving the window with the mouse, as a drag on a title bar
    /// does. Call it from a mouse-down in one of the app's views, while the
    /// button is still down.
    pub fn begin_window_drag(&self, window: Window) -> Result<(), Error> {
        let begin = self.api.window_begin_drag.ok_or(Error::Unsupported)?;
        // SAFETY: plain values.
        check(unsafe { begin(window.0) })
    }

    /// Asks the window to close; a page's beforeunload may refuse.
    pub fn close_window(&self, window: Window) -> Result<(), Error> {
        let close = self.api.window_close.ok_or(Error::Unsupported)?;
        // SAFETY: plain values.
        check(unsafe { close(window.0) })
    }

    /// The window's HWND on Windows or NSWindow pointer on the Mac.
    pub fn native_handle(&self, window: Window) -> Result<*mut c_void, Error> {
        let get = self.api.window_native_handle.ok_or(Error::Unsupported)?;
        let mut out = std::ptr::null_mut();
        // SAFETY: out is a valid pointer for the call.
        check(unsafe { get(window.0, &mut out) })?;
        Ok(out)
    }

    pub fn set_page_rect(&self, window: Window, rect: Rect) -> Result<(), Error> {
        let set = self.api.window_set_page_rect.ok_or(Error::Unsupported)?;
        let rect = sys::jaal_rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        };
        // SAFETY: plain values.
        check(unsafe { set(window.0, rect) })
    }

    pub fn open_tab(&self, window: Window, url: &str, foreground: bool) -> Result<Tab, Error> {
        let open = self.api.tab_open.ok_or(Error::Unsupported)?;
        let url = CString::new(url).map_err(|_| Error::Invalid)?;
        let mut out = sys::jaal_handle::default();
        // SAFETY: url is NUL-terminated and borrowed for the call only.
        check(unsafe { open(window.0, url.as_ptr(), foreground as u8, &mut out) })?;
        Ok(Tab(out))
    }

    pub fn close_tab(&self, tab: Tab) -> Result<(), Error> {
        let close = self.api.tab_close.ok_or(Error::Unsupported)?;
        // SAFETY: plain values.
        check(unsafe { close(tab.0) })
    }

    pub fn activate_tab(&self, tab: Tab) -> Result<(), Error> {
        let activate = self.api.tab_activate.ok_or(Error::Unsupported)?;
        // SAFETY: plain values.
        check(unsafe { activate(tab.0) })
    }

    pub fn load(&self, tab: Tab, url: &str) -> Result<(), Error> {
        let load = self.api.tab_load.ok_or(Error::Unsupported)?;
        let url = CString::new(url).map_err(|_| Error::Invalid)?;
        // SAFETY: url is NUL-terminated and borrowed for the call only.
        check(unsafe { load(tab.0, url.as_ptr()) })
    }

    /// Goes back or forward in the tab's history, reloads it, or stops its
    /// load. Back and forward answer [`Error::Invalid`] when the tab has
    /// nowhere to go.
    pub fn navigate(&self, tab: Tab, action: NavAction) -> Result<(), Error> {
        let navigate = self.api.tab_navigate.ok_or(Error::Unsupported)?;
        let action = match action {
            NavAction::Back => sys::jaal_nav_action::JAAL_NAV_BACK,
            NavAction::Forward => sys::jaal_nav_action::JAAL_NAV_FORWARD,
            NavAction::Reload => sys::jaal_nav_action::JAAL_NAV_RELOAD,
            NavAction::Stop => sys::jaal_nav_action::JAAL_NAV_STOP,
        };
        // SAFETY: plain values.
        check(unsafe { navigate(tab.0, action) })
    }

    /// Names the app's reserved shortcuts: keys the page must never get,
    /// such as Ctrl+T. The engine calls `is_reserved` from the middle of
    /// Chrome's key handling, so it is a plain function that can only look
    /// the key up. A reserved key then arrives as an [`Event::Key`].
    pub fn set_shortcut_filter(&self, is_reserved: fn(Key) -> bool) -> Result<(), Error> {
        unsafe extern "C" fn trampoline(
            user_data: *mut c_void,
            key_code: u32,
            modifiers: u32,
        ) -> u8 {
            // SAFETY: user_data is the fn pointer passed below.
            let is_reserved: fn(Key) -> bool = unsafe { std::mem::transmute(user_data) };
            is_reserved(Key::from_raw(key_code, modifiers)) as u8
        }
        let set = self.api.set_shortcut_filter.ok_or(Error::Unsupported)?;
        // SAFETY: trampoline matches jaal_shortcut_filter, and a fn pointer
        // is valid for the whole process.
        check(unsafe { set(Some(trampoline), is_reserved as *mut c_void) })
    }

    /// Where the page goes, kept as the window resizes.
    pub fn set_page_placement(&self, window: Window, placement: Placement) -> Result<(), Error> {
        let set = self
            .api
            .window_set_page_placement
            .ok_or(Error::Unsupported)?;
        // SAFETY: plain values.
        check(unsafe { set(window.0, placement.to_raw()) })
    }

    /// Shows one of the app's native views (an HWND on Windows) inside the
    /// window, above the page. The engine makes it a child of the window and
    /// keeps it placed; later views sit above earlier ones. The app still
    /// owns the view. The engine moves it from a later task, never from
    /// inside this call.
    pub fn attach_view(
        &self,
        window: Window,
        native_view: NativeView,
        options: ViewOptions,
    ) -> Result<(), Error> {
        let attach = self.api.window_attach_view.ok_or(Error::Unsupported)?;
        let options = options.to_raw();
        // SAFETY: options is valid for the call; the engine checks that
        // native_view is a live window of this thread before using it.
        check(unsafe { attach(window.0, native_view.0, &options) })
    }

    pub fn update_view(
        &self,
        window: Window,
        native_view: NativeView,
        options: ViewOptions,
    ) -> Result<(), Error> {
        let update = self.api.window_update_view.ok_or(Error::Unsupported)?;
        let options = options.to_raw();
        // SAFETY: as in attach_view.
        check(unsafe { update(window.0, native_view.0, &options) })
    }

    pub fn detach_view(&self, window: Window, native_view: NativeView) -> Result<(), Error> {
        let detach = self.api.window_detach_view.ok_or(Error::Unsupported)?;
        // SAFETY: the engine only compares native_view with views it holds.
        check(unsafe { detach(window.0, native_view.0) })
    }

    /// Gives keyboard focus back to the page, after one of the app's views
    /// had it.
    pub fn focus_page(&self, window: Window) -> Result<(), Error> {
        let focus = self.api.window_focus_page.ok_or(Error::Unsupported)?;
        // SAFETY: plain values.
        check(unsafe { focus(window.0) })
    }

    /// Minimizes, maximizes, restores or fullscreens the window. The change
    /// arrives as an [`Event::WindowState`].
    pub fn set_window_state(&self, window: Window, state: WindowState) -> Result<(), Error> {
        let set = self.api.window_set_state.ok_or(Error::Unsupported)?;
        // SAFETY: plain values.
        check(unsafe { set(window.0, state.to_raw()) })
    }
}

#[cfg(test)]
mod tests;
