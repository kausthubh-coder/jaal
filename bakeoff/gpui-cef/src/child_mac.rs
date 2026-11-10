//! macOS: a CEF page is a windowed browser whose NSView is a child of the
//! GPUI window's content view.

use cef::*;
use objc2::msg_send;
use objc2::runtime::{AnyObject, Bool};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::ffi::c_void;

/// A CEF window handle as a plain pointer.
pub(crate) fn native_handle(handle: sys::cef_window_handle_t) -> *mut c_void {
    handle
}

/// `LifeSpanHandler::do_close` for a windowed browser.
///
/// # Safety
/// `view` is the browser's live NSView, on the main thread.
pub(crate) unsafe fn destroy_browser_view(view: *mut c_void) {
    unsafe {
        let _: () = msg_send![view.cast::<AnyObject>(), removeFromSuperview];
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

/// Gives keyboard focus back to the GPUI view after the page had it. AppKit
/// already does this when GPUI's view is clicked.
///
/// # Safety
/// `gpui_view` is a GPUI window's NSView, on the main thread.
pub unsafe fn focus_gpui_view(_gpui_view: *mut c_void) {}
