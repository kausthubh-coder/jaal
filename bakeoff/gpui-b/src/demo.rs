//! `JAAL_DEMO=1`: a scripted session for screenshots and checks, since nobody
//! sits at the Mac mini. It posts real NSEvents into the app's queue
//! (`jaal_gpui_cef::posted_events`), so clicks, keys and wheel ticks reach
//! the page the way a user's do: AppKit, then GPUI, then our forwarding to
//! CEF. It prints `DEMO <step> ...` lines on stderr for the script that
//! takes the screenshots. Not part of the bake-off features.

use crate::browser::BrowserWindow;
use gpui::{AsyncWindowContext, WeakEntity};
use jaal_gpui_cef::posted_events::{
    ScrollPhase, content_size, key_window, post_click, post_key, post_scroll,
};
use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_foundation::NSRect;
use std::time::{Duration, Instant};

const NS_FLOATING_WINDOW_LEVEL: isize = 3;

/// A page with a link and a text field at fixed spots, so the demo knows
/// where to click. Test pages have no links. `#` is `%23` in a data: URL.
const LINK_PAGE: &str = "data:text/html,<title>Link test</title>\
<style>body{font:20px -apple-system;margin:0;background:%23fdfcfa}\
a{position:fixed;left:40px;top:40px;font-size:30px}\
input{position:fixed;left:40px;top:200px;font-size:24px;width:320px;padding:8px}\
%23out{position:fixed;left:40px;top:110px;font-size:26px;color:%23888}\
%23out.done{color:%230a7d32;font-weight:600}</style>\
<a href='%23clicked' onclick=\"out.textContent='Link clicked through forwarded input';out.className='done'\">Click this link</a>\
<p id=out>Not clicked yet</p><input id=field placeholder='Keys go here'>";

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
    // mid-shot. It leaves floating-level windows alone.
    unsafe {
        let _: () = msg_send![main, setLevel: NS_FLOATING_WINDOW_LEVEL];
    }
    report(&this, cx, "ready").await;

    // Cmd+K, then type into the bar. One window: the bar is GPUI drawing
    // over the page, focus moves to its text field.
    retry(
        &this,
        cx,
        "open the bar",
        || unsafe { post_key(main, 'k', 40, true) },
        |b| b.command_bar_is_open(),
    )
    .await;
    for (c, code) in [('d', 2), ('o', 31), ('c', 8), ('s', 1)] {
        unsafe { post_key(main, c, code, false) };
    }
    wait(1000).await;
    report(&this, cx, "bar_open").await;

    retry(
        &this,
        cx,
        "escape",
        || unsafe { post_key(main, '\u{1b}', 53, false) },
        |b| !b.command_bar_is_open(),
    )
    .await;
    report(&this, cx, "bar_closed").await;

    // A trackpad flick on the page: fingers down, 12 moves, lift, then a
    // decaying momentum tail, 8 ms apart. Each tick goes AppKit -> GPUI ->
    // CEF's send_mouse_wheel_event.
    let Some(page) = this.update(cx, |b, _| b.page_drawn_at()).ok().flatten() else {
        eprintln!("DEMO no page drawn yet");
        return;
    };
    let center_x = f64::from(f32::from(page.origin.x + page.size.width / 2.));
    let center_y = f64::from(f32::from(page.origin.y + page.size.height / 2.));
    let paints_before = this.update(cx, |b, _| b.active_page_paints()).unwrap_or(0);
    let started = Instant::now();
    let mut ticks = vec![(ScrollPhase::Began, -6)];
    ticks.extend((0..12).map(|_| (ScrollPhase::Changed, -24)));
    ticks.push((ScrollPhase::Ended, 0));
    ticks.push((ScrollPhase::MomentumBegan, -20));
    ticks.extend((0..30).map(|i| (ScrollPhase::Momentum, -(20 - i * 2 / 3).max(1))));
    ticks.push((ScrollPhase::MomentumEnded, 0));
    for (i, (phase, dy)) in ticks.iter().enumerate() {
        unsafe { post_scroll(main, center_x, center_y, *dy, *phase) };
        wait(8).await;
        if i == 20 {
            // Frames the page painted while the wheel was moving, before
            // the screenshot pause.
            let paints =
                this.update(cx, |b, _| b.active_page_paints()).unwrap_or(0) - paints_before;
            let seconds = started.elapsed().as_secs_f64();
            eprintln!(
                "DEMO scroll: {paints} page frames in {seconds:.2} s of scrolling ({:.0} fps)",
                paints as f64 / seconds
            );
            ask_page(&this, cx, "'scrollY=' + Math.round(scrollY)");
            report(&this, cx, "scrolling").await;
        }
    }
    wait(600).await;
    ask_page(&this, cx, "'scrollY=' + Math.round(scrollY)");
    wait(300).await;
    report(&this, cx, "scrolled").await;

    // The link page, through the same code the URL field's Enter runs.
    let _ = this.update_in(cx, |b, window, cx| b.navigate(LINK_PAGE, window, cx));
    wait(1500).await;
    report(&this, cx, "link_page").await;

    // Click the link and the field, type into it: all forwarded input.
    let Some(page) = this.update(cx, |b, _| b.page_drawn_at()).ok().flatten() else {
        return;
    };
    let page_x = f64::from(f32::from(page.origin.x));
    let page_y = f64::from(f32::from(page.origin.y));
    unsafe { post_click(main, page_x + 90., page_y + 60.) };
    wait(500).await;
    unsafe { post_click(main, page_x + 120., page_y + 225.) };
    wait(300).await;
    for (c, code) in [('h', 4), ('i', 34), ('!', 18)] {
        unsafe { post_key(main, c, code, false) };
    }
    wait(800).await;
    ask_page(
        &this,
        cx,
        "'out=' + out.textContent + ' field=' + field.value",
    );
    wait(300).await;
    report(&this, cx, "clicked").await;

    // Tabs: two new ones with Cmd+T, click the first row, close one.
    for _ in 0..2 {
        unsafe { post_key(main, 't', 17, true) };
        wait(700).await;
    }
    retry(
        &this,
        cx,
        "click tab 1",
        || unsafe { post_click(main, 130., 106.) },
        |b| active_index(b) == Some(0),
    )
    .await;
    report(&this, cx, "clicked_tab_1").await;
    unsafe { post_key(main, 'w', 13, true) };
    wait(1000).await;
    report(&this, cx, "closed_tab").await;

    unsafe { post_key(main, 'q', 12, true) };
}

/// Has the page log `jaal:demo:<expression>`; the console handler prints it.
fn ask_page(this: &WeakEntity<BrowserWindow>, cx: &mut AsyncWindowContext, expression: &str) {
    let code = format!("console.log('jaal:demo:' + ({expression}))");
    let _ = this.update(cx, |b, _| b.run_js_in_active_page(&code));
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
        } else if key == main.cast::<AnyObject>() {
            "main"
        } else {
            "other"
        };
        let (width, height) = unsafe { content_size(main) };
        let frame: NSRect = unsafe { msg_send![main, frame] };
        eprintln!(
            "DEMO {step} tabs={} active={:?} url={:?} bar_open={} bar_query={:?} key={key_is} main_frame={:.0},{:.0},{width:.0}x{height:.0} sidebar_scroll={:.0}",
            b.tabs().list().len(),
            active_index(b),
            b.url_field().read(cx).text(),
            b.command_bar_is_open(),
            b.command_bar_query(cx),
            frame.origin.x,
            frame.origin.y,
            b.sidebar_scroll_offset(),
        );
    });
}
