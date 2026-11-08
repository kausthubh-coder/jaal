//! Benchmark marks. Every variant calls `mark` at the same moments, and the
//! bench scripts read the log, so all three are timed the same way.
//!
//! Marking is off unless `JAAL_BENCH_LOG` names a file. Each mark appends one
//! JSON line: `{"name":"first_paint","ms":1234.5,"pid":42}`, where `ms` is
//! milliseconds since the Unix epoch, so marks from different processes line
//! up.
//!
//! The marks every variant writes:
//! - `main`: first line of `main` in the browser process.
//! - `first_paint`: the first frame that shows the first tab's page.
//! - `tab_switch_start` / `tab_switch_done`: a sidebar click, and the first
//!   frame that shows the newly active page.
//! - `frame`: one per frame the UI draws while `JAAL_BENCH_FRAMES=1`, used by
//!   the scroll and resize tests to count dropped frames.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var_os("JAAL_BENCH_LOG").map(PathBuf::from))
        .as_ref()
}

/// True when frame marks are wanted. Off by default because writing a line
/// per frame is itself work.
pub fn frames_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("JAAL_BENCH_FRAMES").is_ok_and(|v| v == "1"))
}

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// Appends a mark to the log. Does nothing when `JAAL_BENCH_LOG` is unset.
pub fn mark(name: &str) {
    mark_at(name, now_ms());
}

/// Same as `mark`, with a time taken earlier (for example a frame's
/// presentation time reported by the renderer).
pub fn mark_at(name: &str, ms: f64) {
    let Some(path) = log_path() else { return };
    let line = format_mark(name, ms, std::process::id());
    // Opened per mark so several processes can append to one file without
    // sharing a handle. Marks are rare except frames, which are opt-in.
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn format_mark(name: &str, ms: f64, pid: u32) -> String {
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    format!("{{\"name\":\"{name}\",\"ms\":{ms:.3},\"pid\":{pid}}}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_is_one_json_line() {
        assert_eq!(
            format_mark("first_paint", 1234.5, 42),
            "{\"name\":\"first_paint\",\"ms\":1234.500,\"pid\":42}\n"
        );
    }

    #[test]
    fn a_mark_name_cannot_break_the_json() {
        assert_eq!(
            format_mark("bad\"name\n", 1.0, 1),
            "{\"name\":\"badname\",\"ms\":1.000,\"pid\":1}\n"
        );
    }
}
