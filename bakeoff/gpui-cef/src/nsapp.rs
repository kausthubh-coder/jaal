//! CEF on macOS needs NSApp to implement `CefAppProtocol`: Chromium asks
//! `[NSApp isHandlingSendEvent]` to tell whether an event is being dispatched
//! right now (menus, nested run loops and the message pump depend on it).
//! GPUI already installs its own NSApplication subclass, `GPUIApplication`,
//! from a `#[ctor]` before `main` runs. Only one app class can exist, so
//! instead of forking gpui we add the three methods to GPUI's class at run
//! time, before anything creates NSApp.

use objc2::encode::Encode;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Imp, Sel};
use objc2::{ffi, msg_send, sel};
use std::cell::Cell;
use std::ffi::{CStr, CString};

const GPUI_APP_CLASS: &CStr = c"GPUIApplication";

thread_local! {
    // NSApp is a singleton used only on the main thread, so the flag lives in
    // a main-thread global instead of an ivar (ivars can't be added to a class
    // that is already registered).
    static HANDLING_SEND_EVENT: Cell<bool> = const { Cell::new(false) };
}

/// Makes `GPUIApplication` conform to Chromium's `CrAppControlProtocol` (what
/// CEF's `CefAppProtocol` asks for) and creates NSApp as an
/// instance of it. Call on the main thread after the CEF framework is loaded
/// (the protocols live in it) and before `gpui::Application::new()` and
/// `cef::initialize`, either of which could otherwise create a plain
/// NSApplication.
pub fn install_cef_app_protocol() {
    let class = AnyClass::get(GPUI_APP_CLASS).expect("gpui registers GPUIApplication at load");
    let class_ptr = class as *const AnyClass as *mut AnyClass;
    let bool_getter = CString::new(format!("{}@:", Bool::ENCODING)).unwrap();
    let bool_setter = CString::new(format!("v@:{}", Bool::ENCODING)).unwrap();
    unsafe {
        add_method(
            class_ptr,
            sel!(isHandlingSendEvent),
            is_handling_send_event as *const (),
            &bool_getter,
        );
        add_method(
            class_ptr,
            sel!(setHandlingSendEvent:),
            set_handling_send_event as *const (),
            &bool_setter,
        );
        // GPUIApplication doesn't override sendEvent:, so this adds an
        // override that calls NSApplication's.
        add_method(
            class_ptr,
            sel!(sendEvent:),
            send_event as *const (),
            c"v@:@",
        );
        // Chromium checks `conformsToProtocol:` for these two, so they must be
        // the framework's own protocol objects. CefAppProtocol is declared
        // only in CEF's client headers and nothing in the framework checks
        // it, so it isn't in the runtime and we skip it.
        for name in [c"CrAppProtocol", c"CrAppControlProtocol"] {
            let protocol = AnyProtocol::get(name)
                .unwrap_or_else(|| panic!("{name:?} missing; load the CEF framework first"));
            ffi::class_addProtocol(class_ptr, protocol);
        }

        let app: *mut AnyObject = msg_send![class, sharedApplication];
        let is_gpui: Bool = msg_send![app, isKindOfClass: class];
        assert!(
            is_gpui.as_bool(),
            "NSApp was created before install_cef_app_protocol, so it isn't a GPUIApplication"
        );
    }
}

unsafe fn add_method(class: *mut AnyClass, sel: Sel, imp: *const (), types: &CStr) {
    let imp: Imp = unsafe { std::mem::transmute(imp) };
    let added = unsafe { ffi::class_addMethod(class, sel, imp, types.as_ptr()) };
    assert!(added.as_bool(), "GPUIApplication already implements {sel}");
}

extern "C-unwind" fn is_handling_send_event(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::new(HANDLING_SEND_EVENT.get())
}

extern "C-unwind" fn set_handling_send_event(_this: &AnyObject, _cmd: Sel, value: Bool) {
    HANDLING_SEND_EVENT.set(value.as_bool());
}

/// Same as CEF's `CefScopedSendingEvent`: the flag is set for the duration of
/// the dispatch and restored after, also when sendEvent: nests.
extern "C-unwind" fn send_event(this: &AnyObject, _cmd: Sel, event: *mut AnyObject) {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            HANDLING_SEND_EVENT.set(self.0);
        }
    }
    let _restore = Restore(HANDLING_SEND_EVENT.replace(true));
    let ns_application = AnyClass::get(c"NSApplication").unwrap();
    unsafe {
        let _: () = msg_send![super(this, ns_application), sendEvent: event];
    }
}
