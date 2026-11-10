//! Safe Rust API over `jaal_engine.h`, the engine's C API.
//!
//! The UI library gets an [`Engine`] from the `get_api` function the engine
//! passes to `jaal_ui_main`. Every call runs on the engine's UI thread (the
//! main thread), and events arrive later from the top of the loop, never from
//! inside a call. See docs/engine.md › The C API.

use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt;

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

/// Rectangle in a window's client area, in DIPs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    let state = || TabState {
        window: handle_or_none(event.window).map(Window),
        tab: Tab(event.tab),
        url: text(event.url),
        title: text(event.title),
        loading: event.loading != 0,
        active: event.active != 0,
    };
    match event.kind {
        sys::jaal_event_kind::JAAL_EVENT_STARTED => Event::Started,
        sys::jaal_event_kind::JAAL_EVENT_WILL_QUIT => Event::WillQuit,
        sys::jaal_event_kind::JAAL_EVENT_QUIT_CANCELLED => Event::QuitCancelled,
        sys::jaal_event_kind::JAAL_EVENT_WINDOW_CLOSED => Event::WindowClosed(Window(event.window)),
        sys::jaal_event_kind::JAAL_EVENT_TAB_INSERTED => Event::TabInserted(state()),
        sys::jaal_event_kind::JAAL_EVENT_TAB_REMOVED => Event::TabRemoved(Tab(event.tab)),
        sys::jaal_event_kind::JAAL_EVENT_TAB_STATE => Event::TabState(state()),
        other => Event::Unknown(other.0),
    }
}

/// The engine, as seen from the UI library. Cheap to copy; all copies talk to
/// the same engine.
#[derive(Clone, Copy)]
pub struct Engine {
    api: &'static sys::jaal_engine_api,
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
        Ok(Self { api })
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
}

#[cfg(test)]
mod tests;
