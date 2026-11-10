//! Windows: a CEF page is a windowed browser whose HWND is a child of the
//! GPUI window. GPUI draws through DirectComposition with its visuals below
//! child windows (see third_party/gpui/JAAL.md), so the page shows over
//! GPUI's drawing.

use cef::*;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::MapWindowPoints;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetParent, GetWindowRect, IsWindow, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOZORDER, SetWindowPos, ShowWindow, WS_VISIBLE,
};

/// A CEF window handle as a plain pointer.
pub(crate) fn native_handle(handle: sys::cef_window_handle_t) -> *mut c_void {
    handle.0.cast()
}

/// `LifeSpanHandler::do_close` for a windowed browser, which has just
/// returned 1.
///
/// On Windows, returning 1 makes CEF cancel the close and wait for its
/// window to be destroyed: destroying it closes the browser for good and
/// on_before_close follows. (Posting WM_CLOSE instead asks CEF to close
/// again, which calls do_close again, forever.) Destroyed from the message
/// loop, not under CEF's close call.
///
/// # Safety
/// `view` is the browser's HWND.
pub(crate) unsafe fn destroy_browser_view(view: *mut c_void) {
    crate::run_after_gpui(move || unsafe {
        if IsWindow(view) != 0 {
            DestroyWindow(view);
        }
    });
}

/// Gives keyboard focus back to the GPUI window after the page had it.
/// Clicking GPUI's part of the window doesn't move Win32 focus off the
/// page's child window by itself, so typed keys would still go to the page.
///
/// # Safety
/// `gpui_view` is a GPUI window's HWND, on the main thread.
pub unsafe fn focus_gpui_view(gpui_view: *mut c_void) {
    unsafe {
        if GetFocus() != gpui_view {
            SetFocus(gpui_view);
        }
    }
}

/// A windowed CEF browser whose HWND is a child of a GPUI window.
#[derive(Clone)]
pub struct ChildBrowser {
    pub browser: Browser,
    hwnd: HWND,
}

impl ChildBrowser {
    /// `gpui_view` is the HWND from the GPUI window's raw window handle.
    /// The browser starts hidden; place it with `set_frame` and show it.
    pub fn create(gpui_view: *mut c_void, url: &str, client: &mut Client) -> Option<ChildBrowser> {
        let mut window_info = WindowInfo {
            runtime_style: RuntimeStyle::ALLOY,
            ..Default::default()
        }
        .set_as_child(
            sys::HWND(gpui_view.cast()),
            &Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        window_info.style &= !WS_VISIBLE;
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(client),
            Some(&CefString::from(url)),
            Some(&BrowserSettings::default()),
            None,
            None,
        )?;
        let hwnd: HWND = native_handle(browser.host()?.window_handle());
        if hwnd.is_null() {
            return None;
        }
        Some(ChildBrowser { browser, hwnd })
    }

    pub fn id(&self) -> i32 {
        self.browser.identifier()
    }

    /// Places the page at `x, y, width, height` in GPUI's window coordinates
    /// (logical pixels, origin top left of the client area).
    pub fn set_frame(&self, x: f64, y: f64, width: f64, height: f64) {
        unsafe {
            let parent = GetParent(self.hwnd);
            if parent.is_null() {
                return;
            }
            // Child windows are placed in physical pixels.
            let scale = f64::from(GetDpiForWindow(parent)) / 96.;
            let left = (x * scale).round() as i32;
            let top = (y * scale).round() as i32;
            let right = ((x + width) * scale).round() as i32;
            let bottom = ((y + height) * scale).round() as i32;

            let mut current = RECT::default();
            GetWindowRect(self.hwnd, &mut current);
            MapWindowPoints(
                std::ptr::null_mut(),
                parent,
                (&raw mut current).cast::<POINT>(),
                2,
            );
            let wanted = (left, top, right, bottom);
            if (current.left, current.top, current.right, current.bottom) != wanted {
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    left,
                    top,
                    right - left,
                    bottom - top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe { ShowWindow(self.hwnd, if visible { SW_SHOWNA } else { SW_HIDE }) };
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
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        if let Some(host) = self.browser.host() {
            host.close_browser(1);
        }
    }
}
