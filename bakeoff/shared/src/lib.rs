//! Code every bake-off variant shares, so the three browsers show the same
//! tabs and the same sidebar rows and their numbers compare like for like.

use std::path::{Path, PathBuf};

pub mod bench;
pub mod bench_modes;

/// One row in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub title: String,
    pub url: String,
}

/// The fake sidebar rows used by the scroll test. The same `n` always gives
/// the same rows, in every variant.
pub fn fake_rows(n: usize) -> Vec<Row> {
    const SITES: [&str; 8] = [
        "github.com",
        "news.ycombinator.com",
        "docs.rs",
        "en.wikipedia.org",
        "developer.apple.com",
        "learn.microsoft.com",
        "youtube.com",
        "figma.com",
    ];
    (0..n)
        .map(|i| {
            let site = SITES[i % SITES.len()];
            Row {
                title: format!("{site} page {i}"),
                url: format!("https://{site}/page/{i}"),
            }
        })
        .collect()
}

pub type TabId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
}

/// The open tabs and which one is showing. Variants keep their own browser
/// objects keyed by `TabId`.
#[derive(Debug, Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: TabId,
}

impl Tabs {
    pub fn list(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    /// Opens a tab after the active one and makes it active.
    pub fn open(&mut self, url: impl Into<String>) -> TabId {
        let id = self.next_id;
        self.next_id += 1;
        let at = self
            .active
            .and_then(|a| self.index_of(a))
            .map_or(self.tabs.len(), |i| i + 1);
        self.tabs.insert(
            at,
            Tab {
                id,
                url: url.into(),
            },
        );
        self.active = Some(id);
        id
    }

    /// Returns false when `id` isn't open.
    pub fn activate(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        self.active = Some(id);
        true
    }

    /// Closes a tab. Closing the active tab activates the one below it, or
    /// the one above when it was last, as Chrome does.
    pub fn close(&mut self, id: TabId) -> bool {
        let Some(i) = self.index_of(id) else {
            return false;
        };
        self.tabs.remove(i);
        if self.active == Some(id) {
            self.active = self.tabs.get(i).or_else(|| self.tabs.last()).map(|t| t.id);
        }
        true
    }

    pub fn set_url(&mut self, id: TabId, url: impl Into<String>) {
        if let Some(i) = self.index_of(id) {
            self.tabs[i].url = url.into();
        }
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }
}

/// `bakeoff/pages`, the local test pages. Benchmarks load only these, so
/// network speed never shows up in the numbers.
pub fn pages_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../pages")
}

/// `file://` URL of test page `n` (1 to 10).
pub fn test_page_url(n: usize) -> String {
    let path = pages_dir().join(format!("page-{n}.html"));
    let path = path.canonicalize().unwrap_or(path);
    format!("file://{}", path.display())
}

/// What the URL field and command bar do with typed text: URLs and bare
/// hosts load, anything else is a search. Every variant uses it.
pub fn normalize_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.contains("://") || text.starts_with("about:") || text.starts_with("data:") {
        return Some(text.to_string());
    }
    if !text.contains(' ') && (text.contains('.') || text.starts_with("localhost")) {
        return Some(format!("https://{text}"));
    }
    let query: String = text
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    Some(format!("https://duckduckgo.com/?q={query}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_rows_are_the_same_every_time() {
        let rows = fake_rows(5000);
        assert_eq!(rows.len(), 5000);
        assert_eq!(
            rows[9],
            Row {
                title: "news.ycombinator.com page 9".into(),
                url: "https://news.ycombinator.com/page/9".into(),
            }
        );
        assert_eq!(rows, fake_rows(5000));
    }

    #[test]
    fn new_tab_opens_after_the_active_one() {
        let mut tabs = Tabs::default();
        let a = tabs.open("a");
        let b = tabs.open("b");
        tabs.activate(a);
        let c = tabs.open("c");
        let order: Vec<_> = tabs.list().iter().map(|t| t.id).collect();
        assert_eq!(order, vec![a, c, b]);
        assert_eq!(tabs.active(), Some(c));
    }

    #[test]
    fn closing_the_active_tab_activates_the_next_then_the_previous() {
        let mut tabs = Tabs::default();
        let a = tabs.open("a");
        let b = tabs.open("b");
        let c = tabs.open("c");
        tabs.activate(b);
        assert!(tabs.close(b));
        assert_eq!(tabs.active(), Some(c));
        assert!(tabs.close(c));
        assert_eq!(tabs.active(), Some(a));
        assert!(tabs.close(a));
        assert_eq!(tabs.active(), None);
        assert!(!tabs.close(a));
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let mut tabs = Tabs::default();
        let a = tabs.open("a");
        let b = tabs.open("b");
        tabs.close(a);
        assert_eq!(tabs.active(), Some(b));
    }

    #[test]
    fn every_test_page_exists() {
        for n in 1..=10 {
            let url = test_page_url(n);
            let path = url.trim_start_matches("file://");
            assert!(Path::new(path).is_file(), "missing {path}");
        }
    }

    #[test]
    fn full_urls_load_as_typed() {
        assert_eq!(
            normalize_url(" file:///tmp/a.html "),
            Some("file:///tmp/a.html".into())
        );
        assert_eq!(normalize_url("about:blank"), Some("about:blank".into()));
    }

    #[test]
    fn bare_hosts_get_https() {
        assert_eq!(
            normalize_url("docs.rs/gpui"),
            Some("https://docs.rs/gpui".into())
        );
    }

    #[test]
    fn other_text_is_a_search() {
        assert_eq!(
            normalize_url("rust gpui & cef"),
            Some("https://duckduckgo.com/?q=rust+gpui+%26+cef".into())
        );
        assert_eq!(normalize_url("   "), None);
    }
}
