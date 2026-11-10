//! CEF's external message pump, driven from GPUI's main thread.
//!
//! GPUI owns the main loop, so CEF runs with `external_message_pump = 1` and
//! asks us, from any thread, to call `cef::do_message_loop_work()` on the
//! main thread after a delay. This is a port of cefclient's
//! `MainMessageLoopExternalPump`: do the work when asked, never re-enter it,
//! and keep a 33 ms fallback tick running so CEF never stalls if a request
//! is lost.
//!
//! How a request reaches the main thread is per platform, see `platform`.
//! Either way it bypasses GPUI: it works from CEF's threads, needs no GPUI
//! context, and keeps running while the OS runs its own loop during a live
//! window move or resize.

use std::cell::Cell;

/// cefclient's `kMaxTimerDelay`: the longest we wait between pumps.
pub const MAX_DELAY_MS: i64 = 1000 / 30;

thread_local! {
    static PUMP: Pump = const {
        Pump {
            running: Cell::new(false),
            in_work: Cell::new(false),
            reentered: Cell::new(false),
            timer_pending: Cell::new(false),
            timer_generation: Cell::new(0),
        }
    };
}

/// Main-thread state. Only touched from main-thread callbacks.
struct Pump {
    running: Cell<bool>,
    in_work: Cell<bool>,
    reentered: Cell<bool>,
    timer_pending: Cell<bool>,
    // A timer may fire after a newer one replaced it (dispatch_after can't be
    // cancelled), so a timer carries the generation it was set in and does
    // nothing if it is stale.
    timer_generation: Cell<usize>,
}

/// Starts pumping. Call on the main thread right after `cef::initialize`.
pub fn start() {
    platform::init();
    PUMP.with(|p| p.running.set(true));
    set_timer(MAX_DELAY_MS);
}

/// Stops pumping before `cef::shutdown`, so no queued callback calls into CEF
/// after it is gone.
pub fn stop() {
    PUMP.with(|p| {
        p.running.set(false);
        p.timer_pending.set(false);
        p.timer_generation.set(p.timer_generation.get() + 1);
    });
}

/// `BrowserProcessHandler::on_schedule_message_pump_work`. Called on any
/// thread.
pub fn schedule(delay_ms: i64) {
    platform::post_schedule(delay_ms.clamp(0, MAX_DELAY_MS));
}

/// A schedule request, now on the main thread.
fn handle_schedule(delay_ms: i64) {
    if delay_ms == 0 {
        do_work();
    } else {
        set_timer(delay_ms);
    }
}

fn set_timer(delay_ms: i64) {
    let generation = PUMP.with(|p| {
        let next = p.timer_generation.get() + 1;
        p.timer_generation.set(next);
        p.timer_pending.set(true);
        next
    });
    platform::set_timer(delay_ms, generation);
}

fn timer_fired(generation: usize) {
    let current = PUMP.with(|p| p.timer_pending.get() && p.timer_generation.get() == generation);
    if current {
        do_work();
    }
}

fn do_work() {
    let entered = PUMP.with(|p| {
        if !p.running.get() {
            return false;
        }
        if p.in_work.get() {
            // CEF asked for work from inside do_message_loop_work (a nested
            // run loop, a modal dialog). Run it once the outer call returns.
            p.reentered.set(true);
            return false;
        }
        p.timer_pending.set(false);
        p.timer_generation.set(p.timer_generation.get() + 1);
        p.in_work.set(true);
        true
    });
    if !entered {
        return;
    }
    cef::do_message_loop_work();
    let (reentered, timer_pending) = PUMP.with(|p| {
        p.in_work.set(false);
        (p.reentered.replace(false), p.timer_pending.get())
    });
    if reentered {
        schedule(0);
    } else if !timer_pending {
        set_timer(MAX_DELAY_MS);
    }
}

/// macOS: libdispatch's main queue, which every run loop mode drains,
/// including the one AppKit runs during a live window resize.
#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;

    #[repr(C)]
    struct DispatchQueue {
        _private: [u8; 0],
    }

    type DispatchFn = extern "C" fn(*mut c_void);

    unsafe extern "C" {
        // `dispatch_get_main_queue()` is a C macro for `&_dispatch_main_q`.
        static _dispatch_main_q: DispatchQueue;
        fn dispatch_async_f(queue: *const DispatchQueue, context: *mut c_void, work: DispatchFn);
        fn dispatch_after_f(
            when: u64,
            queue: *const DispatchQueue,
            context: *mut c_void,
            work: DispatchFn,
        );
        fn dispatch_time(when: u64, delta_ns: i64) -> u64;
    }

    const DISPATCH_TIME_NOW: u64 = 0;

    fn main_queue() -> *const DispatchQueue {
        &raw const _dispatch_main_q
    }

    pub fn init() {}

    pub fn post_schedule(delay_ms: i64) {
        let context = delay_ms as usize as *mut c_void;
        unsafe { dispatch_async_f(main_queue(), context, handle_schedule) };
    }

    extern "C" fn handle_schedule(context: *mut c_void) {
        super::handle_schedule(context as usize as i64);
    }

    pub fn set_timer(delay_ms: i64, generation: usize) {
        unsafe {
            let when = dispatch_time(DISPATCH_TIME_NOW, delay_ms * 1_000_000);
            dispatch_after_f(when, main_queue(), generation as *mut c_void, timer_fired);
        }
    }

    extern "C" fn timer_fired(context: *mut c_void) {
        super::timer_fired(context as usize);
    }
}

/// Runs `f` on the main thread from the OS message loop, once the current
/// GPUI update has returned. Call on the main thread.
///
/// For native calls that make Windows send a message straight back into a
/// GPUI window: moving or resizing one does, and GPUI answers WM_MOVE and
/// WM_SIZE by borrowing its `App`, which panics while an update holds it.
#[cfg(target_os = "windows")]
pub fn run_after_gpui(f: impl FnOnce() + 'static) {
    platform::run_after_gpui(Box::new(f));
}

/// Windows: a hidden message-only window on the main thread, like cefclient's
/// `MainMessageLoopExternalPumpWin`. Requests arrive as posted messages and
/// delays run on a Win32 timer. GPUI's loop dispatches both, and so does the
/// modal loop Windows runs while the user drags or resizes the window.
#[cfg(target_os = "windows")]
mod platform {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicPtr, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, HWND_MESSAGE, KillTimer, PostMessageW, RegisterClassW,
        SetTimer, WM_APP, WM_TIMER, WNDCLASSW,
    };

    const WM_SCHEDULE_WORK: u32 = WM_APP + 0x4a41;
    const WM_RUN_QUEUED: u32 = WM_APP + 0x4a42;
    const TIMER_ID: usize = 1;

    static WINDOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

    thread_local! {
        // SetTimer replaces the pending timer, so only the newest generation
        // can fire.
        static TIMER_GENERATION: Cell<usize> = const { Cell::new(0) };
        static QUEUED: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    }

    pub fn run_after_gpui(f: Box<dyn FnOnce()>) {
        let first = QUEUED.with_borrow_mut(|queued| {
            queued.push(f);
            queued.len() == 1
        });
        if first {
            let window = WINDOW.load(Ordering::Acquire);
            unsafe { PostMessageW(window, WM_RUN_QUEUED, 0, 0) };
        }
    }

    pub fn init() {
        let class_name: Vec<u16> = "JaalCefPump\0".encode_utf16().collect();
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class_name.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&class);
            let window = CreateWindowExW(
                0,
                class_name.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            assert!(!window.is_null(), "could not create the CEF pump window");
            WINDOW.store(window, Ordering::Release);
        }
    }

    pub fn post_schedule(delay_ms: i64) {
        let window = WINDOW.load(Ordering::Acquire);
        if !window.is_null() {
            unsafe { PostMessageW(window, WM_SCHEDULE_WORK, delay_ms as WPARAM, 0) };
        }
    }

    pub fn set_timer(delay_ms: i64, generation: usize) {
        TIMER_GENERATION.set(generation);
        let window = WINDOW.load(Ordering::Acquire);
        unsafe { SetTimer(window, TIMER_ID, delay_ms as u32, None) };
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_SCHEDULE_WORK => {
                super::handle_schedule(wparam as i64);
                0
            }
            WM_RUN_QUEUED => {
                for f in QUEUED.take() {
                    f();
                }
                0
            }
            WM_TIMER if wparam == TIMER_ID => {
                unsafe { KillTimer(window, TIMER_ID) };
                super::timer_fired(TIMER_GENERATION.get());
                0
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }
}
