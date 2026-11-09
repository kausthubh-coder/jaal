//! Posted NSEvents for the variants' scripted demos (`JAAL_DEMO=1`), since
//! nobody sits at the Mac mini to click. Events go into the app's own queue
//! with `[NSApp postEvent:]`, so they take the same path a user's do from
//! there on. Not part of the bake-off features.

use objc2::encode::{Encoding, RefEncode};
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSPoint, NSRect};
use std::ffi::{CString, c_void};

const NS_EVENT_KEY_DOWN: usize = 10;
const NS_EVENT_KEY_UP: usize = 11;
const NS_EVENT_LEFT_MOUSE_DOWN: usize = 1;
const NS_EVENT_LEFT_MOUSE_UP: usize = 2;
const NS_COMMAND_KEY_MASK: usize = 1 << 20;

/// # Safety
/// Main thread.
pub unsafe fn key_window() -> *mut AnyObject {
    unsafe {
        let app: *mut AnyObject =
            msg_send![AnyClass::get(c"NSApplication").unwrap(), sharedApplication];
        msg_send![app, keyWindow]
    }
}

/// The content view's size in points.
///
/// # Safety
/// Main thread; `window` is a live NSWindow.
pub unsafe fn content_size(window: *mut AnyObject) -> (f64, f64) {
    unsafe {
        let view: *mut AnyObject = msg_send![window, contentView];
        let bounds: NSRect = msg_send![view, bounds];
        (bounds.size.width, bounds.size.height)
    }
}

/// Moves or resizes `window`: `f` gets its frame and returns the new one.
///
/// # Safety
/// Main thread; `window` is a live NSWindow.
pub unsafe fn set_frame(window: *mut AnyObject, f: impl FnOnce(NSRect) -> NSRect) {
    unsafe {
        let frame: NSRect = msg_send![window, frame];
        let _: () = msg_send![window, setFrame: f(frame), display: Bool::YES];
    }
}

unsafe fn post(event: *mut AnyObject) {
    unsafe {
        let app: *mut AnyObject =
            msg_send![AnyClass::get(c"NSApplication").unwrap(), sharedApplication];
        let _: () = msg_send![app, postEvent: event, atStart: Bool::NO];
    }
}

unsafe fn ns_string(s: &str) -> *mut AnyObject {
    let s = CString::new(s).unwrap();
    unsafe { msg_send![AnyClass::get(c"NSString").unwrap(), stringWithUTF8String: s.as_ptr()] }
}

/// Posts a key down and up to `window`.
///
/// # Safety
/// Main thread; `window` (if any) is a live NSWindow.
pub unsafe fn post_key(window: *mut AnyObject, c: char, key_code: u16, command: bool) {
    if window.is_null() {
        eprintln!("DEMO no key window for key {c:?}");
        return;
    }
    let flags = if command { NS_COMMAND_KEY_MASK } else { 0 };
    unsafe {
        let number: isize = msg_send![window, windowNumber];
        let chars = ns_string(&c.to_string());
        for kind in [NS_EVENT_KEY_DOWN, NS_EVENT_KEY_UP] {
            let event: *mut AnyObject = msg_send![
                AnyClass::get(c"NSEvent").unwrap(),
                keyEventWithType: kind,
                location: NSPoint::new(0., 0.),
                modifierFlags: flags,
                timestamp: 0f64,
                windowNumber: number,
                context: std::ptr::null_mut::<AnyObject>(),
                characters: chars,
                charactersIgnoringModifiers: chars,
                isARepeat: Bool::NO,
                keyCode: key_code
            ];
            post(event);
        }
    }
}

/// Posts a left click at `x, y` in the window's content, origin top left.
///
/// # Safety
/// Main thread; `window` (if any) is a live NSWindow.
pub unsafe fn post_click(window: *mut AnyObject, x: f64, y: f64) {
    unsafe {
        let number: isize = msg_send![window, windowNumber];
        let (_, height) = content_size(window);
        let location = NSPoint::new(x, height - y);
        for kind in [NS_EVENT_LEFT_MOUSE_DOWN, NS_EVENT_LEFT_MOUSE_UP] {
            let event: *mut AnyObject = msg_send![
                AnyClass::get(c"NSEvent").unwrap(),
                mouseEventWithType: kind,
                location: location,
                modifierFlags: 0usize,
                timestamp: 0f64,
                windowNumber: number,
                context: std::ptr::null_mut::<AnyObject>(),
                eventNumber: 0isize,
                clickCount: 1isize,
                pressure: 1f32
            ];
            post(event);
        }
    }
}

/// Where a trackpad scroll event is in its gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollPhase {
    /// Fingers on the trackpad: began, then changed.
    Began,
    Changed,
    Ended,
    /// After the fingers lift: AppKit's momentum events.
    MomentumBegan,
    Momentum,
    MomentumEnded,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

/// Quartz's `CGEventRef` target, typed so objc2 checks the message send
/// against AppKit's signature.
#[repr(C)]
struct CGEvent {
    _private: [u8; 0],
}

unsafe impl RefEncode for CGEvent {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateScrollWheelEvent2(
        source: *const c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> *mut CGEvent;
    fn CGEventSetLocation(event: *mut CGEvent, location: CGPoint);
    fn CGEventSetIntegerValueField(event: *mut CGEvent, field: u32, value: i64);
    // Undocumented but exported. AppKit takes an event's locationInWindow
    // from it, and the public location only gives a screen point.
    fn CGEventSetWindowLocation(event: *mut CGEvent, location: CGPoint);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

const CG_SCROLL_EVENT_UNIT_PIXEL: u32 = 0;
/// Undocumented: the window number AppKit gives the wrapped NSEvent.
/// Without it the event has no window and AppKit drops it.
const CG_EVENT_WINDOW_NUMBER: u32 = 51;
const CG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS: u32 = 88;
const CG_SCROLL_WHEEL_EVENT_SCROLL_PHASE: u32 = 99;
const CG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE: u32 = 123;

/// Posts a precise (trackpad-style) scroll of `dy` pixels at `x, y` in the
/// window's content, origin top left. Positive `dy` scrolls toward the top,
/// as AppKit's `scrollingDeltaY` does.
///
/// AppKit has no constructor for scroll NSEvents, so this builds a Quartz
/// event and wraps it, using two undocumented Quartz hooks to aim it at
/// `window`. It is posted to this app's queue only, never to the system.
///
/// # Safety
/// Main thread; `window` is a live NSWindow.
pub unsafe fn post_scroll(window: *mut AnyObject, x: f64, y: f64, dy: i32, phase: ScrollPhase) {
    // CGScrollPhase and CGMomentumScrollPhase values.
    let (scroll_phase, momentum_phase) = match phase {
        ScrollPhase::Began => (1, 0),
        ScrollPhase::Changed => (2, 0),
        ScrollPhase::Ended => (4, 0),
        ScrollPhase::MomentumBegan => (0, 1),
        ScrollPhase::Momentum => (0, 2),
        ScrollPhase::MomentumEnded => (0, 3),
    };
    unsafe {
        let number: isize = msg_send![window, windowNumber];
        let frame: NSRect = msg_send![window, frame];
        let screen: *mut AnyObject = msg_send![window, screen];
        let screen_frame: NSRect = msg_send![screen, frame];
        // Quartz's global origin is the top left of the main screen; the
        // window's frame is in AppKit's, bottom left.
        let global = CGPoint {
            x: frame.origin.x + x,
            y: screen_frame.size.height - (frame.origin.y + frame.size.height) + y,
        };
        let event = CGEventCreateScrollWheelEvent2(
            std::ptr::null(),
            CG_SCROLL_EVENT_UNIT_PIXEL,
            1,
            dy,
            0,
            0,
        );
        if event.is_null() {
            eprintln!("DEMO could not create a scroll event");
            return;
        }
        CGEventSetLocation(event, global);
        CGEventSetIntegerValueField(event, CG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS, 1);
        CGEventSetIntegerValueField(event, CG_SCROLL_WHEEL_EVENT_SCROLL_PHASE, scroll_phase);
        CGEventSetIntegerValueField(event, CG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE, momentum_phase);
        CGEventSetIntegerValueField(event, CG_EVENT_WINDOW_NUMBER, number as i64);
        CGEventSetWindowLocation(event, CGPoint { x, y });
        let ns_event: *mut AnyObject =
            msg_send![AnyClass::get(c"NSEvent").unwrap(), eventWithCGEvent: event];
        CFRelease(event.cast());
        if ns_event.is_null() {
            eprintln!("DEMO could not wrap the scroll event");
            return;
        }
        post(ns_event);
    }
}
