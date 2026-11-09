//! Code both web-ui processes need: the messages the HTML UI sends to the
//! browser process, and the renderer side of CEF's message router, which the
//! helper process runs.

use cef::wrapper::message_router::{
    MessageRouterConfig, MessageRouterRendererSide, MessageRouterRendererSideHandlerCallbacks,
    RendererSideRouter,
};
use cef::*;
use jaal_bakeoff_shared::TabId;
use std::sync::Arc;

/// Both sides must use the same config. The default puts `window.cefQuery`
/// in every frame.
pub fn router_config() -> MessageRouterConfig {
    MessageRouterConfig::default()
}

/// A message from the HTML UI. JS sends it through `window.cefQuery` as
/// lines: the verb, then its arguments. URLs can't hold a newline, so lines
/// are enough and we need no JSON parser. Pass `onSuccess` and `onFailure`
/// every time: cef-rs's router silently drops a query without them.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The sidebar asked to show a tab. `clicked_ms` is the click handler's
    /// epoch time, which `tab_switch_start` records.
    Activate { tab: TabId, clicked_ms: f64 },
    /// The URL field asked the active tab to load what the user typed.
    Navigate(String),
    /// The command bar was dismissed with Escape.
    HideCommandBar,
    /// The command bar's field was submitted with Enter.
    CommandBarSubmit(String),
    /// A page reported its second frame after becoming visible.
    TabShown { ms: f64 },
    /// One batch of requestAnimationFrame times, sent when a bench run ends.
    Frames { run: BenchRun, ms: Vec<f64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchRun {
    Scroll,
    Resize,
}

pub fn parse_command(text: &str) -> Option<Command> {
    let mut lines = text.split('\n');
    let verb = lines.next()?;
    let mut arg = || lines.next();
    Some(match verb {
        "activate" => Command::Activate {
            tab: arg()?.parse().ok()?,
            clicked_ms: arg()?.parse().ok()?,
        },
        "navigate" => Command::Navigate(arg()?.to_string()),
        "hide_command_bar" => Command::HideCommandBar,
        "command_bar_submit" => Command::CommandBarSubmit(arg()?.to_string()),
        "tab_shown" => Command::TabShown {
            ms: arg()?.parse().ok()?,
        },
        "frames" => {
            let run = match arg()? {
                "scroll" => BenchRun::Scroll,
                "resize" => BenchRun::Resize,
                _ => return None,
            };
            let ms = arg()
                .unwrap_or("")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            Command::Frames { run, ms }
        }
        _ => return None,
    })
}

/// Turns what the user typed into a URL to load, or None for blank input.
/// No search engine: the bake-off never leaves local pages.
pub fn normalize_url(typed: &str) -> Option<String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    if typed.contains("://") || typed.starts_with("about:") || typed.starts_with("data:") {
        return Some(typed.to_string());
    }
    if typed.starts_with('/') {
        return Some(format!("file://{typed}"));
    }
    Some(format!("https://{typed}"))
}

/// `s` as a JS string literal, for building `execute_java_script` calls.
pub fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            // `<` keeps a title like "</script>" harmless; U+2028/9 end a
            // line in older JS parsers.
            '<' => out.push_str("\\u003c"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// The helper's app. It only adds `window.cefQuery` to each page.
wrap_app! {
    pub struct HelperApp {
        router: Arc<RendererSideRouter>,
    }

    impl App {
        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(HelperRenderProcessHandler::new(self.router.clone()))
        }
    }
}

pub fn helper_app() -> App {
    HelperApp::new(RendererSideRouter::new(router_config()))
}

wrap_render_process_handler! {
    struct HelperRenderProcessHandler {
        router: Arc<RendererSideRouter>,
    }

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            self.router
                .on_context_created(browser.cloned(), frame.cloned(), context.cloned());
        }

        fn on_context_released(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            self.router
                .on_context_released(browser.cloned(), frame.cloned(), context.cloned());
        }

        fn on_process_message_received(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> i32 {
            self.router.on_process_message_received(
                browser.cloned(),
                frame.cloned(),
                Some(source_process),
                message.cloned(),
            ) as i32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sidebar_click_parses_with_its_time() {
        assert_eq!(
            parse_command("activate\n3\n1731000000123.5"),
            Some(Command::Activate {
                tab: 3,
                clicked_ms: 1731000000123.5
            })
        );
    }

    #[test]
    fn a_frame_batch_parses_in_order() {
        assert_eq!(
            parse_command("frames\nscroll\n10.5,27.25,44"),
            Some(Command::Frames {
                run: BenchRun::Scroll,
                ms: vec![10.5, 27.25, 44.0]
            })
        );
        assert_eq!(
            parse_command("frames\nresize\n"),
            Some(Command::Frames {
                run: BenchRun::Resize,
                ms: vec![]
            })
        );
    }

    #[test]
    fn bad_messages_are_rejected() {
        assert_eq!(parse_command("activate\nnot-a-number\n1"), None);
        assert_eq!(parse_command("activate\n3"), None);
        assert_eq!(parse_command("frames\nwiggle\n1"), None);
        assert_eq!(parse_command("frames\nscroll\n1,x"), None);
        assert_eq!(parse_command("launch_missiles"), None);
    }

    #[test]
    fn typed_urls_get_a_scheme() {
        assert_eq!(normalize_url("  "), None);
        assert_eq!(
            normalize_url(" example.com/a "),
            Some("https://example.com/a".into())
        );
        assert_eq!(
            normalize_url("file:///tmp/x.html"),
            Some("file:///tmp/x.html".into())
        );
        assert_eq!(
            normalize_url("/tmp/x.html"),
            Some("file:///tmp/x.html".into())
        );
        assert_eq!(normalize_url("about:blank"), Some("about:blank".into()));
    }

    #[test]
    fn js_strings_cannot_break_out_of_the_script() {
        assert_eq!(
            js_string("a\"b\\c\n</script>\u{2028}\u{1}"),
            "\"a\\\"b\\\\c\\n\\u003c/script>\\u2028\\u0001\""
        );
    }
}
