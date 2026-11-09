//! Bench modes, driven by environment variables. Marks go through
//! `jaal_bakeoff_shared::bench`; how each one is taken is written up in
//! `docs/bakeoff.md` under "How marks are taken".

use crate::app;
use cef::*;
use jaal_bakeoff_shared::bench::{frames_enabled, now_ms};
use web_ui::BenchRun;

pub use jaal_bakeoff_shared::bench::{mark, mark_at};

const RUN_MS: f64 = 5000.0;
/// Startup work (font caches, GC) settles before a frame run starts, so the
/// run measures scrolling or resizing, not the end of startup.
const SETTLE_MS: i64 = 1000;
const SCROLL_PX_PER_FRAME: u32 = 40;
const RESIZE_MIN: f64 = 900.0;
const RESIZE_MAX: f64 = 1400.0;
/// One full 900 -> 1400 -> 900 sweep.
const RESIZE_PERIOD_MS: f64 = 2500.0;
const RESIZE_TICK_MS: i64 = 16;
const SWITCH_DELAY_MS: i64 = 2000;
const SWITCH_GAP_MS: u32 = 500;
const SWITCH_COUNT: usize = 20;

#[derive(Debug, Default)]
pub struct Bench {
    scroll: bool,
    resize: bool,
    /// `JAAL_BENCH_TABS`: open test pages 1..=tabs at startup. 0 when unset.
    pub tabs: usize,
    switch: bool,
    frame_runs_started: bool,
    tabs_loaded: bool,
    switches_seen: usize,
    switching: bool,
    /// Runs still going. The app quits when the last one ends.
    running: usize,
    keys_to_press: Vec<String>,
}

fn env_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

impl Bench {
    pub fn from_env() -> Self {
        Bench {
            scroll: env_on("JAAL_BENCH_SCROLL"),
            resize: env_on("JAAL_BENCH_RESIZE"),
            tabs: std::env::var("JAAL_BENCH_TABS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            switch: env_on("JAAL_BENCH_SWITCH"),
            keys_to_press: expand_keys(&std::env::var("JAAL_PRESS_KEYS").unwrap_or_default()),
            ..Default::default()
        }
    }
}

/// The sidebar and the first tab have both loaded.
pub fn on_ready() {
    start_pressing_keys();
    // Test aid for screenshots: `JAAL_SCROLL_TO_ROW=4000` jumps the sidebar
    // there, past what the 5 s scroll run reaches.
    if let Some(row) = std::env::var("JAAL_SCROLL_TO_ROW")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        app::run_in_sidebar(&format!("jaal.scrollToRow({row});"));
    }
    let start = app::with_bench(|b| {
        if b.frame_runs_started || !(b.scroll || b.resize) {
            return false;
        }
        b.frame_runs_started = true;
        b.running += b.scroll as usize + b.resize as usize;
        true
    });
    if start {
        let mut task = StartFrameRuns::new();
        post_delayed_task(ThreadId::UI, Some(&mut task), SETTLE_MS);
    }
}

/// Every tab opened by `JAAL_BENCH_TABS` has fired `on_load_end`.
pub fn on_tabs_loaded() {
    let switch = app::with_bench(|b| {
        if b.tabs_loaded {
            return false;
        }
        b.tabs_loaded = true;
        b.running += b.switch as usize;
        b.switch
    });
    mark("tabs_loaded");
    if switch {
        let mut task = StartSwitching::new();
        post_delayed_task(ThreadId::UI, Some(&mut task), SWITCH_DELAY_MS);
    }
}

pub fn on_tab_shown() {
    let finished = app::with_bench(|b| {
        if !b.switching {
            return false;
        }
        b.switches_seen += 1;
        if b.switches_seen < SWITCH_COUNT {
            return false;
        }
        b.switching = false;
        true
    });
    if finished {
        finish_run();
    }
}

pub fn on_frames(_run: BenchRun, times: &[f64]) {
    if frames_enabled() {
        for &ms in times {
            mark_at("frame", ms);
        }
    }
    finish_run();
}

fn finish_run() {
    let all_done = app::with_bench(|b| {
        b.running = b.running.saturating_sub(1);
        b.running == 0
    });
    if all_done {
        app::quit();
    }
}

wrap_task! {
    struct StartFrameRuns;

    impl Task {
        fn execute(&self) {
            let (scroll, resize) = app::with_bench(|b| (b.scroll, b.resize));
            let collect = frames_enabled();
            if scroll {
                app::run_in_sidebar(&format!(
                    "jaal.benchScroll({RUN_MS}, {SCROLL_PX_PER_FRAME}, {collect});"
                ));
            }
            if resize {
                // The page is the view that re-lays out on every step, so
                // its frames are the ones a slow resize drops.
                app::run_in_active_page(&page_frame_collector(collect));
                let mut tick = ResizeTick::new(now_ms());
                post_task(ThreadId::UI, Some(&mut tick));
            }
        }
    }
}

wrap_task! {
    struct ResizeTick {
        start_ms: f64,
    }

    impl Task {
        fn execute(&self) {
            let elapsed = now_ms() - self.start_ms;
            if elapsed >= RUN_MS {
                return;
            }
            app::resize_window(resize_width(elapsed));
            let mut next = ResizeTick::new(self.start_ms);
            post_delayed_task(ThreadId::UI, Some(&mut next), RESIZE_TICK_MS);
        }
    }
}

wrap_task! {
    struct StartSwitching;

    impl Task {
        fn execute(&self) {
            app::with_bench(|b| b.switching = true);
            let ids: Vec<String> = app::tab_ids().iter().map(|id| id.to_string()).collect();
            // Driven from the sidebar's JS so each switch takes the same
            // path as a click: the sidebar's click function, then cefQuery.
            app::run_in_sidebar(&format!(
                "jaal.benchSwitch([{}], {SWITCH_COUNT}, {SWITCH_GAP_MS});",
                ids.join(",")
            ));
        }
    }
}

/// `JAAL_PRESS_KEYS=cmd+k,type:/tmp/a.html,enter`: presses each key once the
/// UI is ready, as real key events posted to our own NSApp, so shortcuts and
/// typing can be checked with nobody at the keyboard. `type:` presses each
/// character of the rest. Test aid only; no bench mark uses it.
fn start_pressing_keys() {
    let keys = app::with_bench(|b| std::mem::take(&mut b.keys_to_press));
    if keys.is_empty() {
        return;
    }
    let mut task = PressKeys::new(keys);
    post_delayed_task(ThreadId::UI, Some(&mut task), FIRST_KEY_DELAY_MS);
}

const FIRST_KEY_DELAY_MS: i64 = 700;
const KEY_GAP_MS: i64 = 150;

fn expand_keys(list: &str) -> Vec<String> {
    list.split(',')
        .filter(|k| !k.is_empty())
        .flat_map(|k| match k.strip_prefix("type:") {
            Some(text) => text.chars().map(String::from).collect(),
            None => vec![k.to_string()],
        })
        .collect()
}

wrap_task! {
    struct PressKeys {
        keys: Vec<String>,
    }

    impl Task {
        fn execute(&self) {
            let Some((key, rest)) = self.keys.split_first() else { return };
            app::press_key(key);
            if !rest.is_empty() {
                let mut next = PressKeys::new(rest.to_vec());
                post_delayed_task(ThreadId::UI, Some(&mut next), KEY_GAP_MS);
            }
        }
    }
}

/// Window width at `elapsed_ms` into the resize run: a triangle wave between
/// 900 and 1400 px, so the speed is the same every run.
fn resize_width(elapsed_ms: f64) -> i32 {
    let phase = (elapsed_ms % RESIZE_PERIOD_MS) / RESIZE_PERIOD_MS;
    let up = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    (RESIZE_MIN + (RESIZE_MAX - RESIZE_MIN) * up).round() as i32
}

fn page_frame_collector(collect: bool) -> String {
    format!(
        "(() => {{ const times = []; const end = performance.now() + {RUN_MS}; \
         const step = (t) => {{ if ({collect}) times.push(performance.timeOrigin + t); \
         if (performance.now() < end) requestAnimationFrame(step); \
         else window.cefQuery({{ request: 'frames\\nresize\\n' + times.map((x) => x.toFixed(3)).join(','), \
         onSuccess() {{}}, onFailure() {{}} }}); }}; \
         requestAnimationFrame(step); }})();"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_becomes_one_key_per_character() {
        assert_eq!(
            expand_keys("cmd+k,type:a/b,enter"),
            vec!["cmd+k", "a", "/", "b", "enter"]
        );
        assert_eq!(expand_keys(""), Vec::<String>::new());
    }

    #[test]
    fn resize_sweeps_between_900_and_1400() {
        assert_eq!(resize_width(0.0), 900);
        assert_eq!(resize_width(RESIZE_PERIOD_MS / 4.0), 1150);
        assert_eq!(resize_width(RESIZE_PERIOD_MS / 2.0), 1400);
        assert_eq!(resize_width(RESIZE_PERIOD_MS * 0.75), 1150);
        assert_eq!(resize_width(RESIZE_PERIOD_MS), 900);
    }
}
