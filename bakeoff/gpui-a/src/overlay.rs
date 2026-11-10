//! Fix A: the command bar is its own transparent GPUI window, glued over the
//! page. GPUI can't draw over the CEF page in the main window, because the
//! page's native view sits above GPUI's.
//!
//! macOS: the bar is an AppKit child window. AppKit moves a child window with
//! its parent, in the same WindowServer transaction, so window moves need
//! nothing from us.
//!
//! Windows: the bar is a window owned by the main window, which keeps it
//! above the main window but doesn't move it along, so the main window calls
//! `place` whenever it moves.
//!
//! Resizes change where the page is, so on both the main window calls
//! `place` on every frame the bar is open.

use gpui::{Bounds, Pixels};

#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(target_os = "windows")]
pub use win::*;

/// The bar's size and its distance below the page's top edge, in points.
pub const BAR_WIDTH: f32 = 620.;
pub const BAR_HEIGHT: f32 = 340.;
const BAR_TOP: f32 = 72.;
const BAR_MARGIN: f32 = 24.;

/// Where the bar goes, in the main window's GPUI coordinates.
pub fn bar_bounds(page: Bounds<Pixels>) -> (f64, f64, f64, f64) {
    let page_x = f64::from(f32::from(page.origin.x));
    let page_y = f64::from(f32::from(page.origin.y));
    let page_w = f64::from(f32::from(page.size.width));
    let page_h = f64::from(f32::from(page.size.height));
    let margin = f64::from(BAR_MARGIN);
    let width = f64::from(BAR_WIDTH).min(page_w - 2. * margin).max(1.);
    let height = f64::from(BAR_HEIGHT).min(page_h - 2. * margin).max(1.);
    let x = page_x + (page_w - width) / 2.;
    let y = page_y + f64::from(BAR_TOP).min((page_h - height) / 2.);
    (x, y, width, height)
}

#[cfg(target_os = "macos")]
mod mac {
    use super::bar_bounds;
    use gpui::{Bounds, Pixels};
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::ffi::c_void;

    const NS_WINDOW_ABOVE: isize = 1;
    // NSWindowCloseButton, NSWindowMiniaturizeButton, NSWindowZoomButton.
    const TRAFFIC_LIGHTS: [usize; 3] = [0, 1, 2];

    /// Turns a fresh GPUI pop-up window into the overlay: a child of `parent`
    /// at the parent's level (GPUI puts pop-ups above every app), without
    /// traffic lights.
    ///
    /// # Safety
    /// Both are live NSViews of GPUI windows, on the main thread.
    pub unsafe fn attach(parent_view: *mut c_void, bar_view: *mut c_void) {
        unsafe {
            let parent = jaal_gpui_cef::ns_window_of(parent_view);
            let bar = jaal_gpui_cef::ns_window_of(bar_view);
            let level: isize = msg_send![parent, level];
            let _: () = msg_send![bar, setLevel: level];
            let _: () = msg_send![bar, setMovable: Bool::NO];
            let _: () = msg_send![bar, setHasShadow: Bool::YES];
            for button in TRAFFIC_LIGHTS {
                let view: *mut AnyObject = msg_send![bar, standardWindowButton: button];
                if !view.is_null() {
                    let _: () = msg_send![view, setHidden: Bool::YES];
                }
            }
            let _: () = msg_send![parent, addChildWindow: bar, ordered: NS_WINDOW_ABOVE];
        }
    }

    /// Puts the bar over `page` (GPUI coordinates of the main window).
    ///
    /// # Safety
    /// Both are live NSViews of GPUI windows, on the main thread.
    pub unsafe fn place(parent_view: *mut c_void, bar_view: *mut c_void, page: Bounds<Pixels>) {
        let (x, y, width, height) = bar_bounds(page);
        unsafe {
            let parent = jaal_gpui_cef::ns_window_of(parent_view);
            let bar = jaal_gpui_cef::ns_window_of(bar_view);
            // The main window has a full-size content view, so its frame is its
            // content and GPUI's (0, 0) is the frame's top left. Screen
            // coordinates have their origin bottom left.
            let parent_frame: NSRect = msg_send![parent, frame];
            let frame = NSRect::new(
                NSPoint::new(
                    parent_frame.origin.x + x,
                    parent_frame.origin.y + parent_frame.size.height - y - height,
                ),
                NSSize::new(width, height),
            );
            let current: NSRect = msg_send![bar, frame];
            if current != frame {
                let _: () = msg_send![bar, setFrame: frame, display: Bool::YES];
            }
        }
    }

    /// # Safety
    /// Both are live NSViews of GPUI windows, on the main thread.
    pub unsafe fn detach(parent_view: *mut c_void, bar_view: *mut c_void) {
        unsafe {
            let parent = jaal_gpui_cef::ns_window_of(parent_view);
            let bar = jaal_gpui_cef::ns_window_of(bar_view);
            let _: () = msg_send![parent, removeChildWindow: bar];
        }
    }
}

/// Every call here runs through `run_after_gpui`: moving a GPUI window makes
/// Windows send it WM_MOVE and WM_SIZE at once, and GPUI handles those by
/// borrowing its `App`, which the caller's update still holds.
#[cfg(target_os = "windows")]
mod win {
    use super::bar_bounds;
    use gpui::{Bounds, Pixels};
    use jaal_gpui_cef::run_after_gpui;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWLP_HWNDPARENT, GetClientRect, GetWindowRect, IsWindow, SWP_NOACTIVATE, SWP_NOZORDER,
        SetWindowLongPtrW, SetWindowPos,
    };

    /// Turns a fresh GPUI pop-up window into the overlay: owned by the main
    /// window, so it stays above it, minimizes with it and has no taskbar
    /// button, with Windows 11's rounded corners. DWM draws the corners and
    /// the shadow.
    ///
    /// # Safety
    /// Both are HWNDs of GPUI windows, on the main thread.
    pub unsafe fn attach(parent_view: *mut c_void, bar_view: *mut c_void) {
        let (parent, bar): (HWND, HWND) = (parent_view, bar_view);
        run_after_gpui(move || unsafe {
            if IsWindow(bar) == 0 {
                return;
            }
            SetWindowLongPtrW(bar, GWLP_HWNDPARENT, parent as isize);
            let round = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                bar,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                (&raw const round).cast(),
                size_of_val(&round) as u32,
            );
        });
    }

    /// Puts the bar over `page` (GPUI coordinates of the main window).
    ///
    /// # Safety
    /// Both are HWNDs of GPUI windows, on the main thread.
    pub unsafe fn place(parent_view: *mut c_void, bar_view: *mut c_void, page: Bounds<Pixels>) {
        let (x, y, width, height) = bar_bounds(page);
        let (parent, bar): (HWND, HWND) = (parent_view, bar_view);
        run_after_gpui(move || unsafe {
            if IsWindow(parent) == 0 || IsWindow(bar) == 0 {
                return;
            }
            // GPUI's coordinates are the client area's, in logical pixels;
            // windows move in physical screen pixels.
            let scale = f64::from(GetDpiForWindow(parent)) / 96.;
            let mut origin = POINT { x: 0, y: 0 };
            ClientToScreen(parent, &mut origin);
            let left = origin.x + (x * scale).round() as i32;
            let top = origin.y + (y * scale).round() as i32;
            let right = origin.x + ((x + width) * scale).round() as i32;
            let bottom = origin.y + ((y + height) * scale).round() as i32;

            // The bar's window rect can be larger than its client area (an
            // invisible resize border); place it so the client area lands
            // on the spot.
            let mut window = RECT::default();
            GetWindowRect(bar, &mut window);
            let mut client = RECT::default();
            GetClientRect(bar, &mut client);
            let mut client_origin = POINT { x: 0, y: 0 };
            ClientToScreen(bar, &mut client_origin);
            let border_x = (window.right - window.left) - client.right;
            let border_y = (window.bottom - window.top) - client.bottom;
            let wanted = (
                left - (client_origin.x - window.left),
                top - (client_origin.y - window.top),
                right - left + border_x,
                bottom - top + border_y,
            );
            let current = (
                window.left,
                window.top,
                window.right - window.left,
                window.bottom - window.top,
            );
            if wanted != current {
                SetWindowPos(
                    bar,
                    std::ptr::null_mut(),
                    wanted.0,
                    wanted.1,
                    wanted.2,
                    wanted.3,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        });
    }

    /// Nothing to undo: ownership ends when the bar's window is removed.
    ///
    /// # Safety
    /// Both are HWNDs of GPUI windows, on the main thread.
    pub unsafe fn detach(_parent_view: *mut c_void, _bar_view: *mut c_void) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn the_bar_sits_centered_near_the_top_of_the_page() {
        let page = Bounds::new(point(px(272.), px(8.)), size(px(920.), px(784.)));
        assert_eq!(bar_bounds(page), (422., 80., 620., 340.));
    }

    #[test]
    fn the_bar_shrinks_to_fit_a_small_page() {
        let page = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(300.)));
        assert_eq!(bar_bounds(page), (24., 24., 352., 252.));
    }
}
