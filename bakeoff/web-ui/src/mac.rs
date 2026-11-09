//! The macOS glue CEF needs: an NSApplication that speaks CEF's protocols,
//! and the window number for `screencapture -l`.

use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObjectProtocol};
use objc2::{ClassType, DefinedClass, MainThreadMarker, define_class, extern_methods, msg_send};
use objc2_app_kit::{NSApp, NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSPoint, NSString};
use std::cell::Cell;
use std::ffi::c_void;

#[derive(Default)]
pub struct JaalApplicationIvars {
    handling_send_event: Cell<Bool>,
}

define_class!(
    // Chromium checks that NSApp tracks whether it is inside sendEvent:, and
    // CEF asserts NSApp conforms to CefAppProtocol before it initializes.
    #[unsafe(super(NSApplication))]
    #[ivars = JaalApplicationIvars]
    pub struct JaalApplication;

    impl JaalApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let was_sending = self.ivars().handling_send_event.get();
            self.ivars().handling_send_event.set(Bool::YES);
            let _: () = msg_send![super(self), sendEvent: event];
            self.ivars().handling_send_event.set(was_sending);
        }

        // The default terminate: calls exit() and never leaves the run loop,
        // which skips Chromium's shutdown. Close the browsers instead; the
        // run loop ends when the last one is gone.
        #[unsafe(method(terminate:))]
        unsafe fn terminate(&self, _sender: Option<&AnyObject>) {
            crate::app::quit();
        }
    }

    unsafe impl CrAppControlProtocol for JaalApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_handling_send_event(&self, handling: Bool) {
            self.ivars().handling_send_event.set(handling);
        }
    }

    unsafe impl CrAppProtocol for JaalApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for JaalApplication {}
);

impl JaalApplication {
    extern_methods! {
        #[unsafe(method(sharedApplication))]
        fn shared_application() -> objc2::rc::Retained<Self>;
    }
}

/// Makes NSApp a `JaalApplication`. Must run before anything touches NSApp.
pub fn install_application() {
    let _ = JaalApplication::shared_application();
    let mtm = MainThreadMarker::new().expect("not on the main thread");
    assert!(
        NSApp(mtm).isKindOfClass(JaalApplication::class()),
        "NSApp was created before install_application"
    );
}

/// The NSWindow behind a CEF window handle, which on macOS is an NSView (or,
/// for some CEF versions, the NSWindow itself).
fn ns_window<'a>(handle: *mut c_void) -> Option<&'a AnyObject> {
    // SAFETY: CEF hands out a live NSView or NSWindow pointer, and AppKit
    // keeps the window alive while it's on screen.
    unsafe {
        let object = handle.cast::<AnyObject>().as_ref()?;
        let ns_window_class = AnyClass::get(c"NSWindow")?;
        let is_window: bool = msg_send![object, isKindOfClass: ns_window_class];
        if is_window {
            return Some(object);
        }
        let window: *mut AnyObject = msg_send![object, window];
        window.as_ref()
    }
}

/// The CGWindowID of the window behind a CEF window handle.
pub fn window_number(handle: *mut c_void) -> Option<isize> {
    let window = ns_window(handle)?;
    Some(unsafe { msg_send![window, windowNumber] })
}

/// Posts a key down and up to our own NSApp, aimed at the key window or else
/// at `handle`'s window, the same path a real keyboard
/// takes through AppKit into Chromium. `key` is one unshifted character
/// (a-z, 0-9, punctuation such as / . -), "enter" or "esc", optionally
/// prefixed with "cmd+". Returns false for a key we can't name.
pub fn press_key(handle: *mut c_void, key: &str) -> bool {
    let (command, name) = match key.strip_prefix("cmd+") {
        Some(rest) => (true, rest),
        None => (false, key),
    };
    let Some((key_code, chars)) = mac_key(name) else {
        return false;
    };
    let mtm = MainThreadMarker::new().expect("not on the main thread");
    let app = NSApp(mtm);
    // A real key goes to the key window. With another app active (a system
    // prompt on the shared screen) we have none, so aim at the window that
    // would be key: `handle`'s.
    let key_window: *mut AnyObject = unsafe { msg_send![&*app, keyWindow] };
    let Some(target) = unsafe { key_window.as_ref() }.or_else(|| ns_window(handle)) else {
        return false;
    };
    let window_number: isize = unsafe { msg_send![target, windowNumber] };
    let flags = if command {
        NSEventModifierFlags::Command
    } else {
        NSEventModifierFlags::empty()
    };
    let chars = NSString::from_str(&chars);
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let Some(event) = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            window_number,
            None,
            &chars,
            &chars,
            false,
            key_code,
        ) else {
            continue;
        };
        if command {
            // Key equivalents are matched by NSApp before any window.
            app.sendEvent(&event);
        } else {
            // NSApp sends plain keys to its main window when no window is
            // key, whatever window the event names; go to the target direct.
            let _: () = unsafe { msg_send![target, sendEvent: &*event] };
        }
    }
    true
}

/// macOS virtual key code (kVK_ANSI_*) and the character it types.
fn mac_key(name: &str) -> Option<(u16, String)> {
    // kVK_ANSI_* codes, indexed by code. '\0' marks codes that aren't a
    // plain character (36 is Return).
    const ANSI: &str = "asdfhgzxcv\0bqweryt123465=97-80]ou[ip\0lj'k;\\,/nm.";
    match name {
        "enter" => return Some((0x24, "\r".into())),
        "esc" => return Some((0x35, "\u{1b}".into())),
        _ => {}
    }
    let mut chars = name.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let code = ANSI.chars().position(|a| a == c && a != '\0')?;
    Some((code as u16, c.to_string()))
}

/// Rounds the command bar and gives it a shadow over the page, natively.
///
/// CEF can't make a windowed browser transparent (alpha 0 falls back to
/// opaque, see `background_color` in cef_types.h), so the HTML can't draw a
/// shadow outside its own rectangle or blur the page. On macOS a CEF overlay
/// is its own borderless child NSWindow, so we let the window server do it:
/// clear window background, rounded content layer, window shadow. AppKit
/// derives the shadow from the rounded shape.
pub fn round_overlay_with_shadow(handle: *mut c_void, radius: f64) {
    // SAFETY: `handle` is the overlay browser's live content NSView.
    unsafe {
        let Some(view) = handle.cast::<AnyObject>().as_ref() else {
            return;
        };
        let window: *mut AnyObject = msg_send![view, window];
        let Some(window) = window.as_ref() else {
            return;
        };
        let Some(ns_color) = AnyClass::get(c"NSColor") else {
            return;
        };
        let clear: *mut AnyObject = msg_send![ns_color, clearColor];
        let _: () = msg_send![window, setOpaque: false];
        let _: () = msg_send![window, setBackgroundColor: clear];
        let _: () = msg_send![view, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![view, layer];
        if let Some(layer) = layer.as_ref() {
            let _: () = msg_send![layer, setCornerRadius: radius];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
        let _: () = msg_send![window, setHasShadow: true];
        let _: () = msg_send![window, invalidateShadow];
    }
}

/// Makes the NSWindow behind a CEF window handle the key window, so typing
/// goes to it. CEF's `request_focus` on the overlay's view doesn't do this:
/// the overlay is its own child window and the main window stays key.
pub fn make_key_window(handle: *mut c_void) {
    if let Some(window) = ns_window(handle) {
        let _: () = unsafe { msg_send![window, makeKeyWindow] };
    }
}
