//! `JAAL_DEMO=1`: a scripted session for screenshots and checks, since nobody
//! sits at the Mac mini to click. It posts real NSEvents into the app's event
//! queue (`[NSApp postEvent:]`), so keys and clicks take the same path a
//! user's do, and prints `DEMO <step> ...` lines on stderr for the script that
//! takes the screenshots. Not part of the bake-off features; it only runs
//! when the variable is set.

use crate::browser::BrowserWindow;
use gpui::{AsyncWindowContext, WeakEntity};
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::ffi::CString;

use std::time::Duration;

const NS_EVENT_KEY_DOWN: usize = 10;
const NS_EVENT_KEY_UP: usize = 11;
const NS_EVENT_LEFT_MOUSE_DOWN: usize = 1;
const NS_EVENT_LEFT_MOUSE_UP: usize = 2;
const NS_COMMAND_KEY_MASK: usize = 1 << 20;
const NS_FLOATING_WINDOW_LEVEL: isize = 3;

pub fn enabled() -> bool {
    std::env::var("JAAL_DEMO").is_ok_and(|v| v == "1")
}

pub async fn run(this: WeakEntity<BrowserWindow>, cx: &mut AsyncWindowContext) {
    let executor = cx.background_executor().clone();
    let wait = move |ms: u64| executor.timer(Duration::from_millis(ms));
    wait(1500).await;
    let Ok(view) = this.update(cx, |b, _| b.gpui_view()) else {
        return;
    };
    let main = unsafe { jaal_gpui_cef::ns_window_of(view) };
    // Stage Manager shuffles an inactive app's windows into its side strip,
    // mid-shot. It leaves floating-level windows alone. The command bar
    // takes its parent's level, so it still sits above the page.
    unsafe {
        let _: () = msg_send![main, setLevel: NS_FLOATING_WINDOW_LEVEL];
    }
    report(&this, cx, "ready").await;

    // Two new tabs with Cmd+T. Focus is on the page, so this is the CEF
    // keyboard-handler path.
    for _ in 0..2 {
        unsafe { post_key(main, 't', 17, true) };
        wait(700).await;
    }
    wait(1000).await;
    report(&this, cx, "new_tabs").await;

    // Click the first tab row in the sidebar: GPUI's click path.
    retry(
        &this,
        cx,
        "click tab 1",
        || unsafe { post_click(main, 130., 106.) },
        |b| active_index(b) == Some(0),
    )
    .await;
    report(&this, cx, "clicked_tab_1").await;

    // Type a URL in the sidebar field and press Enter. This needs the main
    // window to be key, which an inactive app's window can't be, so it is
    // expected to do nothing here; the log shows it.
    unsafe { post_click(main, 130., 61.) };
    wait(300).await;
    for (c, code) in [
        ('a', 0),
        ('b', 11),
        ('o', 31),
        ('u', 32),
        ('t', 17),
        (':', 41),
    ] {
        unsafe { post_key(main, c, code, false) };
    }
    unsafe { post_key(main, '\r', 36, false) };
    wait(1200).await;
    report(&this, cx, "typed_url").await;

    // Back to a test page so the screenshots show content.
    retry(
        &this,
        cx,
        "click tab 2",
        || unsafe { post_click(main, 130., 140.) },
        |b| active_index(b) == Some(1),
    )
    .await;

    // Into the band of the screen that the Mac mini's leftover system
    // dialogs and banners don't cover, so a screen-region capture shows the
    // window and the bar alone. AppKit's y is from the screen's bottom.
    unsafe {
        set_frame(main, |_| {
            NSRect::new(NSPoint::new(170., 90.), NSSize::new(1000., 520.))
        })
    };
    wait(800).await;
    report(&this, cx, "placed").await;

    // Cmd+K, then type into the bar: focus must be in the bar window.
    retry(
        &this,
        cx,
        "open the bar",
        || unsafe { post_key(main, 'k', 40, true) },
        |b| b.command_bar_is_open(),
    )
    .await;
    wait(300).await;
    for (c, code) in [('d', 2), ('o', 31), ('c', 8), ('s', 1)] {
        let key = unsafe { key_window() };
        unsafe { post_key(key, c, code, false) };
    }
    wait(1200).await;
    report(&this, cx, "bar_open").await;

    // Move the main window; AppKit carries the child window along.
    unsafe {
        set_frame(main, |f| {
            NSRect::new(NSPoint::new(f.origin.x + 350., f.origin.y), f.size)
        })
    };
    wait(1200).await;
    report(&this, cx, "moved").await;

    // Resize it; the bar re-centers over the page on the next frame.
    unsafe {
        set_frame(main, |f| {
            NSRect::new(
                NSPoint::new(f.origin.x, f.origin.y + 60.),
                NSSize::new(f.size.width - 180., f.size.height - 60.),
            )
        })
    };
    wait(1200).await;
    report(&this, cx, "resized").await;

    // A click on the page, outside the bar, must close it.
    let (w, h) = unsafe { content_size(main) };
    retry(
        &this,
        cx,
        "click outside the bar",
        || unsafe { post_click(main, w - 120., h - 60.) },
        |b| !b.command_bar_is_open(),
    )
    .await;
    report(&this, cx, "clicked_outside").await;

    // Cmd+K again, then Escape closes it.
    retry(
        &this,
        cx,
        "reopen the bar",
        || unsafe { post_key(main, 'k', 40, true) },
        |b| b.command_bar_is_open(),
    )
    .await;
    report(&this, cx, "bar_reopened").await;
    retry(
        &this,
        cx,
        "escape",
        || unsafe { post_key(key_window(), '\u{1b}', 53, false) },
        |b| !b.command_bar_is_open(),
    )
    .await;
    report(&this, cx, "escaped").await;

    let _ = this.update(cx, |b, cx| {
        b.scroll_rows_to(3200);
        cx.notify();
    });
    wait(1000).await;
    report(&this, cx, "scrolled").await;

    unsafe { post_key(main, 'w', 13, true) };
    wait(1000).await;
    report(&this, cx, "closed_tab").await;

    unsafe { post_key(main, 'q', 12, true) };
}

fn active_index(b: &BrowserWindow) -> Option<usize> {
    let id = b.tabs().active()?;
    b.tabs().list().iter().position(|t| t.id == id)
}

/// Posts an event until `done` holds, up to 4 tries. Events posted into an
/// app that isn't frontmost are sometimes dropped; the log says how many
/// tries it took.
async fn retry(
    this: &WeakEntity<BrowserWindow>,
    cx: &mut AsyncWindowContext,
    what: &str,
    post: impl Fn(),
    done: impl Fn(&BrowserWindow) -> bool,
) {
    for attempt in 1..=4 {
        post();
        cx.background_executor()
            .timer(Duration::from_millis(700))
            .await;
        if this.update(cx, |b, _| done(b)).unwrap_or(false) {
            eprintln!("DEMO {what}: ok after {attempt} tries");
            return;
        }
    }
    eprintln!("DEMO {what}: failed after 4 tries");
}

/// Prints the state after `step`, then waits (up to 8 s) for the screenshot
/// script to create `$JAAL_DEMO_ACK_DIR/<step>`, so each shot shows the step
/// it is named after.
async fn report(this: &WeakEntity<BrowserWindow>, cx: &mut AsyncWindowContext, step: &str) {
    print_state(this, cx, step);
    let Some(dir) = std::env::var_os("JAAL_DEMO_ACK_DIR") else {
        return;
    };
    let ack = std::path::Path::new(&dir).join(step);
    for _ in 0..80 {
        if ack.exists() {
            return;
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    eprintln!("DEMO no screenshot ack for {step}");
}

fn print_state(this: &WeakEntity<BrowserWindow>, cx: &mut AsyncWindowContext, step: &str) {
    let _ = this.update(cx, |b, cx| {
        let key = unsafe { key_window() };
        let main = unsafe { jaal_gpui_cef::ns_window_of(b.gpui_view()) };
        // Stage Manager parks an inactive app's window in its side strip,
        // where a capture is a thumbnail. Fronting it doesn't activate the
        // app, so focus stays as the step left it.
        unsafe {
            let _: () = msg_send![main, orderFrontRegardless];
        }
        let key_is = if key.is_null() {
            "none"
        } else if key == main {
            "main"
        } else {
            "other"
        };
        let active = b
            .tabs()
            .active()
            .and_then(|id| b.tabs().list().iter().position(|t| t.id == id));
        let frame: NSRect = unsafe { msg_send![main, frame] };
        let bar_query = b.command_bar_query(cx);
        eprintln!(
            "DEMO {step} tabs={} active={:?} url={:?} bar_open={} bar_query={bar_query:?} key={key_is} main_frame={:.0},{:.0},{:.0}x{:.0} sidebar_scroll={:.0}",
            b.tabs().list().len(),
            active,
            b.url_field().read(cx).text(),
            b.command_bar_is_open(),
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            b.sidebar_scroll_offset(),
        );
    });
}

unsafe fn key_window() -> *mut AnyObject {
    unsafe {
        let app: *mut AnyObject =
            msg_send![AnyClass::get(c"NSApplication").unwrap(), sharedApplication];
        msg_send![app, keyWindow]
    }
}

unsafe fn content_size(window: *mut AnyObject) -> (f64, f64) {
    unsafe {
        let view: *mut AnyObject = msg_send![window, contentView];
        let bounds: NSRect = msg_send![view, bounds];
        (bounds.size.width, bounds.size.height)
    }
}

unsafe fn set_frame(window: *mut AnyObject, f: impl FnOnce(NSRect) -> NSRect) {
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
unsafe fn post_key(window: *mut AnyObject, c: char, key_code: u16, command: bool) {
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
unsafe fn post_click(window: *mut AnyObject, x: f64, y: f64) {
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
