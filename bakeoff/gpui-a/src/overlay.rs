//! Fix A: the command bar is its own transparent GPUI window, glued over the
//! page as an AppKit child window. GPUI can't draw over the CEF page view in
//! the main window, because the page's NSView sits above GPUI's.
//!
//! AppKit moves a child window with its parent, in the same WindowServer
//! transaction, so window moves need nothing from us. Resizes change where the
//! page is, so the main window calls `place` on every frame the bar is open.

use gpui::{Bounds, Pixels};
use objc2::msg_send;
use objc2::runtime::{AnyObject, Bool};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::ffi::c_void;

const NS_WINDOW_ABOVE: isize = 1;
// NSWindowCloseButton, NSWindowMiniaturizeButton, NSWindowZoomButton.
const TRAFFIC_LIGHTS: [usize; 3] = [0, 1, 2];

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
