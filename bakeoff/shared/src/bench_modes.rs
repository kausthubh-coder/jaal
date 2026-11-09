//! The bench modes from docs/bakeoff.md ("How marks are taken"), read from
//! the environment. Marks go through `bench`; this file only decides what to
//! do, the same way in every variant.

use crate::test_page_url;
use std::time::Duration;

/// How long the scroll and resize runs last.
pub const RUN_TIME: Duration = Duration::from_secs(5);
pub const SCROLL_PX_PER_FRAME: f32 = 40.;
pub const RESIZE_MIN_WIDTH: f32 = 900.;
pub const RESIZE_MAX_WIDTH: f32 = 1400.;
/// One full 900 -> 1400 -> 900 sweep.
const RESIZE_PERIOD_S: f64 = 2.;

pub const SWITCH_COUNT: usize = 20;
pub const SWITCH_DELAY: Duration = Duration::from_secs(2);
pub const SWITCH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BenchConfig {
    /// `JAAL_BENCH_TABS=N`: open test pages 1..N at start.
    pub tabs: Option<usize>,
    /// `JAAL_BENCH_SWITCH=1`: after `tabs_loaded`, switch tabs 20 times.
    pub switch: bool,
    /// `JAAL_BENCH_SCROLL=1`: scroll the sidebar rows for 5 s, then quit.
    pub scroll: bool,
    /// `JAAL_BENCH_RESIZE=1`: sweep the window width for 5 s, then quit.
    pub resize: bool,
}

impl BenchConfig {
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let on = |name: &str| var(name).is_some_and(|v| v == "1");
        Self {
            tabs: var("JAAL_BENCH_TABS")
                .and_then(|v| v.parse().ok())
                .filter(|&n| n > 0),
            switch: on("JAAL_BENCH_SWITCH"),
            scroll: on("JAAL_BENCH_SCROLL"),
            resize: on("JAAL_BENCH_RESIZE"),
        }
    }
}

/// The tabs to open at start: test pages 1..N (N = 1 unless
/// `JAAL_BENCH_TABS` says otherwise), with `first_url` replacing page 1.
pub fn startup_urls(tabs: Option<usize>, first_url: Option<String>) -> Vec<String> {
    let n = tabs.unwrap_or(1);
    (1..=n)
        .map(|i| match (&first_url, i) {
            (Some(url), 1) => url.clone(),
            // Only 10 test pages exist; more tabs reuse them in order.
            _ => test_page_url((i - 1) % 10 + 1),
        })
        .collect()
}

/// The first command-line argument, if it isn't a switch (macOS and CEF
/// pass some, like `-psn_...` or `--type=`).
pub fn url_argument(mut args: impl Iterator<Item = String>) -> Option<String> {
    args.nth(1).filter(|a| !a.starts_with('-'))
}

/// Index of the tab the `step`th switch (0-based) goes to: tab 2, 3, ... N,
/// 1, 2, ... round robin, starting from tab 1 active.
pub fn switch_target(step: usize, tab_count: usize) -> usize {
    (step + 1) % tab_count
}

/// Window width `elapsed_s` into the resize run: a triangle wave between
/// 900 and 1400 points.
pub fn resize_width(elapsed_s: f64) -> f32 {
    let phase = (elapsed_s / RESIZE_PERIOD_S).fract();
    let up = if phase < 0.5 {
        phase * 2.
    } else {
        2. - phase * 2.
    };
    RESIZE_MIN_WIDTH + (RESIZE_MAX_WIDTH - RESIZE_MIN_WIDTH) * up as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_vars_turn_modes_on_only_when_set_to_1() {
        let vars = |name: &str| match name {
            "JAAL_BENCH_TABS" => Some("10".to_string()),
            "JAAL_BENCH_SWITCH" => Some("1".to_string()),
            "JAAL_BENCH_SCROLL" => Some("0".to_string()),
            _ => None,
        };
        assert_eq!(
            BenchConfig::from_vars(vars),
            BenchConfig {
                tabs: Some(10),
                switch: true,
                scroll: false,
                resize: false,
            }
        );
        assert_eq!(BenchConfig::from_vars(|_| None), BenchConfig::default());
    }

    #[test]
    fn startup_opens_one_test_page_by_default() {
        assert_eq!(startup_urls(None, None), vec![test_page_url(1)]);
    }

    #[test]
    fn bench_tabs_open_pages_in_order_and_the_argument_replaces_page_1() {
        let urls = startup_urls(Some(3), Some("https://example.com/".into()));
        assert_eq!(
            urls,
            vec![
                "https://example.com/".to_string(),
                test_page_url(2),
                test_page_url(3)
            ]
        );
    }

    #[test]
    fn switches_go_round_robin_from_tab_2() {
        let order: Vec<usize> = (0..SWITCH_COUNT).map(|s| switch_target(s, 10)).collect();
        assert_eq!(
            order,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0]
        );
    }

    #[test]
    fn only_a_plain_first_argument_is_a_url() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            url_argument(args(&["gpui-a", "file:///p.html"]).into_iter()),
            Some("file:///p.html".into())
        );
        assert_eq!(
            url_argument(args(&["gpui-a", "-psn_0_1"]).into_iter()),
            None
        );
        assert_eq!(url_argument(args(&["gpui-a"]).into_iter()), None);
    }

    #[test]
    fn resize_sweeps_between_900_and_1400() {
        assert_eq!(resize_width(0.), 900.);
        assert_eq!(resize_width(0.5), 1150.);
        assert_eq!(resize_width(1.), 1400.);
        assert_eq!(resize_width(1.5), 1150.);
        assert_eq!(resize_width(2.), 900.);
    }
}
