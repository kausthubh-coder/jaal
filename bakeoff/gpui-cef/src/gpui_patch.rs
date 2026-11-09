//! A run-time fix for a GPUI 0.2.2 deadlock (macOS `window.rs`,
//! `window_did_change_key_status`).
//!
//! When the app isn't active and a pop-up window is open, AppKit sends a
//! window a spurious `windowDidBecomeKey:` although it isn't key. GPUI undoes
//! it by calling `resignKeyWindow` while it still holds the window's state
//! mutex; `resignKeyWindow` synchronously sends `windowDidResignKey:` to the
//! same handler, which takes the same non-reentrant mutex, and the main
//! thread hangs forever. The command bar is such a pop-up, so clicking the
//! main window with the bar open, while another app is frontmost, froze the
//! browser.
//!
//! We wrap GPUI's `windowDidBecomeKey:` so the spurious case calls
//! `resignKeyWindow` before GPUI's handler runs, with no lock held, and skip
//! GPUI's handler for it. Real key changes go to GPUI unchanged.

use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{ffi, msg_send, sel};
use std::sync::OnceLock;

type KeyStatusFn = unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);

static GPUI_DID_BECOME_KEY: OnceLock<KeyStatusFn> = OnceLock::new();

/// Call once on the main thread, before GPUI opens windows. GPUI registers
/// both window classes from a `#[ctor]`, so they exist by `main`.
pub fn fix_key_status_deadlock() {
    for name in [c"GPUIWindow", c"GPUIPanel"] {
        let class = AnyClass::get(name).expect("gpui registers its window classes at load");
        let class_ptr = class as *const AnyClass as *mut AnyClass;
        unsafe {
            let wrapper: Imp = std::mem::transmute(did_become_key as KeyStatusFn);
            let previous = ffi::class_replaceMethod(
                class_ptr,
                sel!(windowDidBecomeKey:),
                wrapper,
                c"v@:@".as_ptr(),
            );
            let previous = previous.expect("gpui implements windowDidBecomeKey:");
            // Both classes get the same GPUI function; keep the first.
            let _ = GPUI_DID_BECOME_KEY.set(std::mem::transmute::<Imp, KeyStatusFn>(previous));
        }
    }
}

unsafe extern "C-unwind" fn did_become_key(
    this: &AnyObject,
    cmd: Sel,
    notification: *mut AnyObject,
) {
    let is_key: Bool = unsafe { msg_send![this, isKeyWindow] };
    if !is_key.as_bool() {
        unsafe {
            let _: () = msg_send![this, resignKeyWindow];
        }
        return;
    }
    let gpui = GPUI_DID_BECOME_KEY.get().expect("installed");
    unsafe { gpui(this, cmd, notification) };
}
