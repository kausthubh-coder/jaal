//! CEF's external message pump, driven from the main dispatch queue.
//!
//! GPUI owns the main loop (`[NSApp run]`), so CEF runs with
//! `external_message_pump = 1` and asks us, from any thread, to call
//! `cef::do_message_loop_work()` on the main thread after a delay. This is a
//! port of cefclient's `MainMessageLoopExternalPump`: do the work when asked,
//! never re-enter it, and keep a 33 ms fallback tick running so CEF never
//! stalls if a request is lost.
//!
//! We post to libdispatch's main queue directly rather than through GPUI: it
//! works from CEF's threads, needs no GPUI context, and is drained by every
//! run loop mode, including the one AppKit runs during a live window resize.

use std::cell::Cell;
use std::ffi::c_void;

/// cefclient's `kMaxTimerDelay`: the longest we wait between pumps.
pub const MAX_DELAY_MS: i64 = 1000 / 30;

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

/// Main-thread state. Only touched from main-queue callbacks.
struct Pump {
    running: Cell<bool>,
    in_work: Cell<bool>,
    reentered: Cell<bool>,
    timer_pending: Cell<bool>,
    // dispatch_after can't be cancelled, so a timer carries the generation it
    // was set in and does nothing if a newer timer replaced it.
    timer_generation: Cell<usize>,
}

/// Starts pumping. Call on the main thread right after `cef::initialize`.
pub fn start() {
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
    let delay = delay_ms.clamp(0, MAX_DELAY_MS) as usize;
    unsafe { dispatch_async_f(main_queue(), delay as *mut c_void, handle_schedule) };
}

extern "C" fn handle_schedule(context: *mut c_void) {
    let delay_ms = context as usize as i64;
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
    unsafe {
        let when = dispatch_time(DISPATCH_TIME_NOW, delay_ms * 1_000_000);
        dispatch_after_f(when, main_queue(), generation as *mut c_void, timer_fired);
    }
}

extern "C" fn timer_fired(context: *mut c_void) {
    let generation = context as usize;
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
