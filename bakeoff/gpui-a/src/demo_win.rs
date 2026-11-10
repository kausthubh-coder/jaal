//! `JAAL_DEMO=1` on Windows: the same scripted session as demo.rs on macOS,
//! for screenshots and checks. Keys and clicks go through `SendInput`, so
//! they take the path a user's do: the OS input queue, then whichever window
//! is in front and has focus. That needs the window in front, so the demo
//! brings it there, and it moves the real mouse pointer (put back at the
//! end). It sends nothing while another app's window is in front, so it
//! can't type into someone's other work. It prints `DEMO <step> ...` lines on stderr for the script that
//! takes the screenshots (scripts/demo-windows.ts). Not part of the
//! bake-off features; it only runs when the variable is set.

use crate::browser::BrowserWindow;
use gpui::{AsyncWindowContext, WeakEntity};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput, VIRTUAL_KEY, VK_CONTROL,
    VK_ESCAPE, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    SWP_NOACTIVATE, SWP_NOZORDER, SetCursorPos, SetWindowPos,
};

pub fn enabled() -> bool {
    std::env::var("JAAL_DEMO").is_ok_and(|v| v == "1")
}

pub async fn run(this: WeakEntity<BrowserWindow>, cx: &mut AsyncWindowContext) {
    let executor = cx.background_executor().clone();
    let wait = move |ms: u64| executor.timer(Duration::from_millis(ms));
    wait(1500).await;
    let Ok(main) = this.update(cx, |b, _| b.gpui_view()) else {
        return;
    };
    let mut pointer = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut pointer) };
    bring_to_front(cx).await;
    report(&this, cx, "ready").await;

    // Two new tabs with Ctrl+T. Activation put focus on GPUI's window, so
    // the first takes GPUI's key binding; the new tab's page then has focus,
    // so the second takes the CEF keyboard-handler path.
    for _ in 0..2 {
        bring_to_front(cx).await;
        shortcut(b'T');
        wait(700).await;
    }
    wait(1000).await;
    report(&this, cx, "new_tabs").await;

    // Click the first tab row in the sidebar: GPUI's click path.
    retry(
        &this,
        cx,
        "click tab 1",
        || unsafe { click(main, 130., 106.) },
        |b| active_index(b) == Some(0),
    )
    .await;
    report(&this, cx, "clicked_tab_1").await;

    // Type a URL in the sidebar field and press Enter. The click must take
    // keyboard focus off the page for the keys to reach the field.
    bring_to_front(cx).await;
    unsafe { click(main, 130., 61.) };
    wait(300).await;
    shortcut(b'A');
    type_text("about:blank");
    key(VK_RETURN, false);
    key(VK_RETURN, true);
    wait(1200).await;
    report(&this, cx, "typed_url").await;

    // Back to a test page so the screenshots show content.
    retry(
        &this,
        cx,
        "click tab 2",
        || unsafe { click(main, 130., 140.) },
        |b| active_index(b) == Some(1),
    )
    .await;

    // A fixed place and size, so every run's shots match.
    unsafe {
        set_window(main, |_, scale| {
            (120, 80, px(1100., scale), px(700., scale))
        })
    };
    wait(800).await;
    report(&this, cx, "placed").await;

    // Ctrl+K, then type into the bar: focus must be in the bar window.
    retry(
        &this,
        cx,
        "open the bar",
        || shortcut(b'K'),
        |b| b.command_bar_is_open(),
    )
    .await;
    wait(300).await;
    type_text("docs");
    wait(1200).await;
    report(&this, cx, "bar_open").await;

    // Move the main window; the bar must follow (Windows doesn't carry an
    // owned window along, gpui-a moves it).
    unsafe { set_window(main, |(x, y, w, h), scale| (x + px(250., scale), y, w, h)) };
    wait(1200).await;
    report(&this, cx, "moved").await;

    // Resize it; the bar re-centers over the page on the next frame.
    unsafe {
        set_window(main, |(x, y, w, h), scale| {
            (
                x,
                y + px(60., scale),
                w - px(180., scale),
                h - px(60., scale),
            )
        })
    };
    wait(1200).await;
    report(&this, cx, "resized").await;

    // A click on the page, outside the bar, must close it.
    let (w, h) = unsafe { client_size(main) };
    retry(
        &this,
        cx,
        "click outside the bar",
        || unsafe { click(main, w - 120., h - 60.) },
        |b| !b.command_bar_is_open(),
    )
    .await;
    report(&this, cx, "clicked_outside").await;

    // Ctrl+K again, then Escape closes it.
    retry(
        &this,
        cx,
        "reopen the bar",
        || shortcut(b'K'),
        |b| b.command_bar_is_open(),
    )
    .await;
    report(&this, cx, "bar_reopened").await;
    retry(
        &this,
        cx,
        "escape",
        || {
            key(VK_ESCAPE, false);
            key(VK_ESCAPE, true);
        },
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

    // Ctrl+W with focus back on the page (Escape returned it to the main
    // window): the page's keyboard-handler path again.
    retry(
        &this,
        cx,
        "close tab",
        || shortcut(b'W'),
        |b| b.tabs().list().len() == 2,
    )
    .await;
    report(&this, cx, "closed_tab").await;

    unsafe { SetCursorPos(pointer.x, pointer.y) };
    bring_to_front(cx).await;
    shortcut(b'Q');
    wait(5000).await;
    eprintln!("DEMO quit: still running 5 s after Ctrl+Q");
}

fn active_index(b: &BrowserWindow) -> Option<usize> {
    let id = b.tabs().active()?;
    b.tabs().list().iter().position(|t| t.id == id)
}

fn px(logical: f64, scale: f64) -> i32 {
    (logical * scale).round() as i32
}

/// True when the window in front is one of ours: the main window or the
/// command bar.
fn ours_in_front() -> bool {
    unsafe {
        let window = GetForegroundWindow();
        let mut process = 0;
        !window.is_null()
            && GetWindowThreadProcessId(window, &mut process) != 0
            && process == GetCurrentProcessId()
    }
}

/// SendInput types into whatever window is in front, so this activates
/// ours (GPUI's activation works even when another app was in front) and
/// waits until it is.
async fn bring_to_front(cx: &mut AsyncWindowContext) {
    for _ in 0..10 {
        if ours_in_front() {
            return;
        }
        let _ = cx.update(|window, _| window.activate_window());
        cx.background_executor()
            .timer(Duration::from_millis(300))
            .await;
    }
    eprintln!("DEMO could not bring the window to the front");
}

fn send(inputs: &[INPUT]) {
    if !ours_in_front() {
        eprintln!("DEMO skipped input: another app's window is in front");
        return;
    }
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
}

fn keyboard(vk: VIRTUAL_KEY, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) {
    send(&[keyboard(vk, 0, if up { KEYEVENTF_KEYUP } else { 0 })]);
}

/// Ctrl+`letter` (an upper-case ASCII letter, which is its virtual key).
fn shortcut(letter: u8) {
    let letter = VIRTUAL_KEY::from(letter);
    send(&[
        keyboard(VK_CONTROL, 0, 0),
        keyboard(letter, 0, 0),
        keyboard(letter, 0, KEYEVENTF_KEYUP),
        keyboard(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ]);
}

/// Types `text` as characters, whatever the keyboard layout.
fn type_text(text: &str) {
    for unit in text.encode_utf16() {
        send(&[
            keyboard(0, unit, KEYEVENTF_UNICODE),
            keyboard(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
        ]);
    }
}

/// Left click at `x, y` in the window's GPUI coordinates.
///
/// # Safety
/// `window` is a live HWND.
unsafe fn click(window: HWND, x: f64, y: f64) {
    unsafe {
        let scale = f64::from(GetDpiForWindow(window)) / 96.;
        let mut point = POINT {
            x: px(x, scale),
            y: px(y, scale),
        };
        ClientToScreen(window, &mut point);
        SetCursorPos(point.x, point.y);
    }
    let mouse = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[mouse(MOUSEEVENTF_LEFTDOWN), mouse(MOUSEEVENTF_LEFTUP)]);
}

/// The client area's size in logical pixels.
///
/// # Safety
/// `window` is a live HWND.
unsafe fn client_size(window: HWND) -> (f64, f64) {
    unsafe {
        let scale = f64::from(GetDpiForWindow(window)) / 96.;
        let mut rect = RECT::default();
        GetClientRect(window, &mut rect);
        (
            f64::from(rect.right) / scale,
            f64::from(rect.bottom) / scale,
        )
    }
}

/// Moves or resizes `window`: `f` gets its rect (x, y, width, height in
/// physical pixels) and the scale, and returns the new one. Called between
/// GPUI updates, so GPUI can handle the WM_MOVE and WM_SIZE this sends.
///
/// # Safety
/// `window` is a live HWND.
unsafe fn set_window(
    window: HWND,
    f: impl FnOnce((i32, i32, i32, i32), f64) -> (i32, i32, i32, i32),
) {
    unsafe {
        let scale = f64::from(GetDpiForWindow(window)) / 96.;
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect);
        let current = (
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        );
        let (x, y, w, h) = f(current, scale);
        SetWindowPos(
            window,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Sends input until `done` holds, up to 4 tries. The log says how many
/// tries it took.
async fn retry(
    this: &WeakEntity<BrowserWindow>,
    cx: &mut AsyncWindowContext,
    what: &str,
    post: impl Fn(),
    done: impl Fn(&BrowserWindow) -> bool,
) {
    for attempt in 1..=4 {
        bring_to_front(cx).await;
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
        let main = b.gpui_view();
        let foreground = unsafe { GetForegroundWindow() };
        let front = if foreground == main {
            "main"
        } else if foreground.is_null() {
            "none"
        } else {
            "other"
        };
        let active = b
            .tabs()
            .active()
            .and_then(|id| b.tabs().list().iter().position(|t| t.id == id));
        let mut rect = RECT::default();
        unsafe { GetWindowRect(main, &mut rect) };
        let bar_query = b.command_bar_query(cx);
        eprintln!(
            "DEMO {step} tabs={} active={:?} url={:?} bar_open={} bar_query={bar_query:?} front={front} main_rect={},{},{}x{} sidebar_scroll={:.0}",
            b.tabs().list().len(),
            active,
            b.url_field().read(cx).text(),
            b.command_bar_is_open(),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            b.sidebar_scroll_offset(),
        );
    });
}
