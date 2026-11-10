//! The command bar: a see-through GPUI window the engine keeps over the
//! page, as a child of its own window.
//!
//! GPUI can't draw over the page inside the sidebar's window, and it can't
//! read the page's pixels to blur them. So this is a second window, larger
//! than the bar by `MARGIN` on every side for its shadow, and the engine
//! blurs the page under the bar itself with a layer in Chrome's compositor.
//! Windows' own blur was tried first: it does nothing on current Windows 11
//! (bakeoff/results, M2).

use crate::icons;
use crate::log;
use crate::shell::Shell;
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, PAGE_FRAME, SIDEBAR_WIDTH, Theme};
use gpui::{
    App, Bounds, Context, Entity, Focusable, FontWeight, KeyBinding, SharedString, Subscription,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, actions, div, point, prelude::*, px, size, svg,
};
use jaal_engine::{Engine, Placement, Tab, ViewOptions};
use std::ffi::c_void;

/// The bar's width, and the room around it for its shadow, in DIPs.
pub const BAR_WIDTH: i32 = 540;
// The shadow is 64 DIPs of blur dropped 28 DIPs down; anything less than
// this clips it at the window's edge.
const MARGIN: i32 = 128;
/// Where the bar's top edge sits below the window's, as in the design.
const BAR_TOP: i32 = 104;
const BAR_RADIUS: f32 = 12.;
/// The blur the design's glass uses: `backdrop-filter: blur(28px)`.
const BLUR: f32 = 28.;

const INPUT_HEIGHT: i32 = 54;
const RESULT_HEIGHT: i32 = 40;
const FOOTER_HEIGHT: i32 = 36;
const PADDING: i32 = 8;
const MAX_RESULTS: usize = 6;

actions!(command_bar, [SelectNext, SelectPrevious]);

const CONTEXT: &str = "CommandBar";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
    ]);
}

/// What Enter does with typed text that matches no tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarMode {
    /// Ctrl+T: open it in a new tab.
    NewTab,
    /// Ctrl+K: load it in the current tab.
    Navigate,
}

/// One row of results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarResult {
    /// An open tab whose title or address contains the query.
    SwitchTo {
        tab: Tab,
        title: SharedString,
        host: String,
    },
    /// What was typed, as a URL to load or a search to run.
    Open { text: String, is_search: bool },
}

/// The results for `query`: matching tabs first, then the typed text itself.
pub fn results_for(query: &str, tabs: &[(Tab, SharedString, String)]) -> Vec<BarResult> {
    let query = query.trim();
    let needle = query.to_lowercase();
    let mut results: Vec<BarResult> = tabs
        .iter()
        .filter(|(_, title, url)| {
            needle.is_empty()
                || title.to_lowercase().contains(&needle)
                || url.to_lowercase().contains(&needle)
        })
        .take(MAX_RESULTS - 1)
        .map(|(tab, title, url)| BarResult::SwitchTo {
            tab: *tab,
            title: title.clone(),
            host: crate::shell::split_url(url).0,
        })
        .collect();
    if let Some(url) = jaal_bakeoff_shared::normalize_url(query) {
        results.push(BarResult::Open {
            text: query.to_string(),
            is_search: url.starts_with("https://duckduckgo.com/?q="),
        });
    }
    results
}

/// The bar's height for a number of results, in DIPs.
pub fn bar_height(results: usize) -> i32 {
    let list = if results == 0 {
        0
    } else {
        PADDING + results as i32 * RESULT_HEIGHT
    };
    INPUT_HEIGHT + list + FOOTER_HEIGHT + PADDING
}

/// Where the bar's window goes: centered over the page, `BAR_TOP` below the
/// window's top, with room for the shadow all around.
fn view_options(results: usize) -> ViewOptions {
    ViewOptions {
        placement: Placement {
            // Both edges and a width: centered between the sidebar and the
            // frame at the right, which is where the page is.
            left: Some(SIDEBAR_WIDTH),
            right: Some(PAGE_FRAME),
            width: Some(BAR_WIDTH + 2 * MARGIN),
            top: Some(BAR_TOP - MARGIN),
            height: Some(bar_height(results) + 2 * MARGIN),
            bottom: None,
        },
        visible: true,
        backdrop_blur: BLUR,
        backdrop_inset: MARGIN,
        backdrop_corner_radius: BAR_RADIUS,
    }
}

pub struct CommandBar {
    shell: WeakEntity<Shell>,
    engine: Engine,
    engine_window: jaal_engine::Window,
    view: *mut c_void,
    mode: BarMode,
    tabs: Vec<(Tab, SharedString, String)>,
    input: Entity<TextInput>,
    results: Vec<BarResult>,
    selected: usize,
    theme: Theme,
    /// Whether this window has had keyboard focus yet. It opens without it.
    was_active: bool,
    _subscriptions: Vec<Subscription>,
}

impl CommandBar {
    /// Opens the bar and gives it keyboard focus. Returns its window and
    /// native view.
    pub fn open(
        shell: WeakEntity<Shell>,
        engine: Engine,
        engine_window: jaal_engine::Window,
        mode: BarMode,
        tabs: Vec<(Tab, SharedString, String)>,
        theme: Theme,
        cx: &mut App,
    ) -> Option<(WindowHandle<CommandBar>, *mut c_void)> {
        let results = results_for("", &tabs);
        let options = view_options(results.len());
        let bar = cx
            .open_window(
                child_window_options(
                    options.placement.width.unwrap_or(0),
                    options.placement.height.unwrap_or(0),
                ),
                |window, cx| {
                    cx.new(|cx| {
                        CommandBar::new(
                            shell,
                            engine,
                            engine_window,
                            mode,
                            tabs,
                            results,
                            theme,
                            window,
                            cx,
                        )
                    })
                },
            )
            .map_err(|error| log(&format!("ui: command bar window: {error}")))
            .ok()?;
        let view = bar
            .update(cx, |bar, window, _| {
                // Focus goes into the bar as it opens. The window takes
                // keyboard focus from the page once the engine has shown it.
                window.activate_window();
                bar.view
            })
            .ok()?;
        engine.attach_view(engine_window, view, options).ok()?;
        Some((bar, view))
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        shell: WeakEntity<Shell>,
        engine: Engine,
        engine_window: jaal_engine::Window,
        mode: BarMode,
        tabs: Vec<(Tab, SharedString, String)>,
        results: Vec<BarResult>,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                match mode {
                    BarMode::NewTab => "Search or enter address for a new tab",
                    BarMode::Navigate => "Search, enter an address, or pick a tab",
                },
                theme.ink.into(),
                theme.ink_3.into(),
                cx,
            )
        });
        window.focus(&input.read(cx).focus_handle(cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &input,
                window,
                |this, _, event: &TextInputEvent, _, cx| match event {
                    TextInputEvent::Changed(text) => this.set_query(text, cx),
                    TextInputEvent::Submit(_) => this.accept(cx),
                    TextInputEvent::Cancel => this.close(true, cx),
                },
            ),
            // Clicking anywhere else takes keyboard focus away, which closes
            // the bar. Focus is already where the user put it.
            cx.observe_window_activation(window, |this, window, cx| {
                if this.was_active && !window.is_window_active() {
                    this.close(false, cx);
                }
                this.was_active = window.is_window_active();
            }),
        ];
        Self {
            shell,
            engine,
            engine_window,
            view: crate::native_view(window),
            mode,
            tabs,
            input,
            results,
            selected: 0,
            theme,
            was_active: false,
            _subscriptions: subscriptions,
        }
    }

    fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let before = self.results.len();
        self.results = results_for(query, &self.tabs);
        self.selected = 0;
        if self.results.len() != before {
            // The engine resizes the window and the blur under it together.
            let _ = self.engine.update_view(
                self.engine_window,
                self.view,
                view_options(self.results.len()),
            );
        }
        cx.notify();
    }

    fn select(&mut self, step: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let count = self.results.len() as isize;
        self.selected = (self.selected as isize + step).rem_euclid(count) as usize;
        cx.notify();
    }

    fn accept(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self.results.get(self.selected).cloned() else {
            return self.close(true, cx);
        };
        let mode = self.mode;
        let _ = self.shell.update(cx, |shell, cx| {
            match result {
                BarResult::SwitchTo { tab, .. } => shell.activate_tab(tab),
                BarResult::Open { text, .. } => match mode {
                    BarMode::NewTab => shell.open_in_new_tab(&text),
                    BarMode::Navigate => shell.navigate(&text, cx),
                },
            }
            shell.close_command_bar(true, cx);
        });
    }

    fn close(&mut self, refocus_page: bool, cx: &mut Context<Self>) {
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.close_command_bar(refocus_page, cx));
    }

    fn render_result(&self, index: usize, result: &BarResult) -> impl IntoElement + use<> {
        let theme = &self.theme;
        let selected = index == self.selected;
        let (lead, title, detail, kind) = match result {
            BarResult::SwitchTo { title, host, .. } => {
                (icons::LINK, title.clone(), host.clone(), "Switch to tab")
            }
            BarResult::Open { text, is_search } => (
                if *is_search {
                    icons::SEARCH
                } else {
                    icons::LINK
                },
                SharedString::from(if *is_search {
                    format!("Search the web for \u{201c}{text}\u{201d}")
                } else {
                    text.clone()
                }),
                String::new(),
                match (self.mode, *is_search) {
                    (_, true) => "Search",
                    (BarMode::NewTab, false) => "Open in new tab",
                    (BarMode::Navigate, false) => "Go",
                },
            ),
        };
        div()
            .id(index)
            .h(px(RESULT_HEIGHT as f32))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(11.))
            .rounded(px(10.))
            .when(selected, |row| row.bg(theme.accent_fill))
            .when(!selected, |row| {
                row.hover(|style| style.bg(theme.fill_hover))
            })
            .child(
                svg()
                    .path(lead)
                    .size(px(16.))
                    .flex_none()
                    .text_color(theme.ink_2),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .overflow_hidden()
                    .child(div().truncate().child(title))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(theme.ink_3)
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .when(selected, |kind| {
                        kind.text_color(theme.accent_ink)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!selected, |kind| kind.text_color(theme.ink_3))
                    .child(kind),
            )
    }
}

/// Options for a GPUI window the engine will adopt as a child of its own.
/// It starts hidden: the engine parents, places and shows it.
pub fn child_window_options(width: i32, height: i32) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(width as f32), px(height as f32)),
        ))),
        // With no title bar asked for, GPUI treats the top edge as a caption
        // it draws itself and takes the frame out of the client area.
        titlebar: Some(gpui::TitlebarOptions {
            title: None,
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        // Even for a see-through window. A child window's surface already
        // blends over the page by its own alpha; GPUI's "transparent" mode
        // adds a DWM accent on top, which paints a pale wash over it.
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    }
}

impl Render for CommandBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let results: Vec<_> = self
            .results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                (
                    self.render_result(index, result).into_any_element(),
                    index,
                    result.clone(),
                )
            })
            .collect();
        let hint = |key: &'static str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().text_color(theme.ink).child(key))
                .child(label)
        };
        let enter_label = match (self.results.get(self.selected), self.mode) {
            (Some(BarResult::SwitchTo { .. }), _) => "Switch to tab",
            (
                Some(BarResult::Open {
                    is_search: true, ..
                }),
                _,
            ) => "Search",
            (_, BarMode::NewTab) => "Open in new tab",
            (_, BarMode::Navigate) => "Go",
        };

        div()
            .size_full()
            // Room for the shadow. A click here is a click outside the bar.
            .p(px(MARGIN as f32))
            .font_family(theme::FONT)
            .text_size(px(13.))
            .line_height(px(17.))
            .text_color(theme.ink)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close(true, cx)),
            )
            .child(
                div()
                    .key_context(CONTEXT)
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select(1, cx)))
                    .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select(-1, cx)))
                    .on_action(
                        cx.listener(|this, _: &crate::shell::ToggleCommandBar, _, cx| {
                            this.close(true, cx)
                        }),
                    )
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .size_full()
                    .flex()
                    .flex_col()
                    .px(px(PADDING as f32))
                    .pb(px(PADDING as f32))
                    .rounded(px(BAR_RADIUS))
                    // The glass: a tint over the page the engine blurred.
                    .bg(theme.glass)
                    .shadow(theme.glass_shadow.clone())
                    .child(
                        div()
                            .h(px(INPUT_HEIGHT as f32))
                            .flex_none()
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .text_size(px(17.))
                            .line_height(px(24.))
                            .child(
                                svg()
                                    .path(icons::SEARCH)
                                    .size(px(18.))
                                    .flex_none()
                                    .text_color(theme.ink_3),
                            )
                            .child(self.input.clone()),
                    )
                    .when(!results.is_empty(), |bar| {
                        bar.child(div().h(px(1.)).flex_none().bg(theme.line))
                            .child(div().h(px(PADDING as f32 - 1.)).flex_none())
                            .children(results.into_iter().map(|(row, index, result)| {
                                div()
                                    .id(("result", index))
                                    .flex_none()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.selected = this
                                            .results
                                            .iter()
                                            .position(|r| *r == result)
                                            .unwrap_or(index);
                                        this.accept(cx);
                                    }))
                                    .child(row)
                            }))
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .h(px(FOOTER_HEIGHT as f32))
                            .flex_none()
                            .px(px(10.))
                            .pt(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .border_t_1()
                            .border_color(theme.line)
                            .text_size(px(11.5))
                            .text_color(theme.ink_2)
                            .child(hint("\u{21b5}", enter_label))
                            .child(hint("\u{2191}\u{2193}", "Choose"))
                            .child(div().flex_1())
                            .child(hint("esc", "Close")),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaal_engine::sys;

    fn tab(slot: u32) -> Tab {
        Tab::from_raw(sys::jaal_handle {
            slot,
            generation: 1,
        })
    }

    fn tabs() -> Vec<(Tab, SharedString, String)> {
        vec![
            (
                tab(1),
                "How native apps draw".into(),
                "https://jaal.app/research".into(),
            ),
            (
                tab(2),
                "Speedometer 3.1".into(),
                "https://browserbench.org/Speedometer3.1/".into(),
            ),
        ]
    }

    #[test]
    fn an_empty_query_lists_every_open_tab_and_nothing_to_open() {
        assert_eq!(
            results_for("  ", &tabs()),
            vec![
                BarResult::SwitchTo {
                    tab: tab(1),
                    title: "How native apps draw".into(),
                    host: "jaal.app".into(),
                },
                BarResult::SwitchTo {
                    tab: tab(2),
                    title: "Speedometer 3.1".into(),
                    host: "browserbench.org".into(),
                },
            ]
        );
    }

    #[test]
    fn a_query_matches_titles_and_addresses_then_offers_the_text_itself() {
        // "speed" is in a title; "BROWSERBENCH" only in an address.
        for query in ["speed", "BROWSERBENCH"] {
            assert_eq!(
                results_for(query, &tabs()),
                vec![
                    BarResult::SwitchTo {
                        tab: tab(2),
                        title: "Speedometer 3.1".into(),
                        host: "browserbench.org".into(),
                    },
                    BarResult::Open {
                        text: query.into(),
                        is_search: true,
                    },
                ],
                "query {query}"
            );
        }
    }

    #[test]
    fn a_typed_address_opens_as_an_address_not_a_search() {
        assert_eq!(
            results_for("example.com/docs", &tabs()),
            vec![BarResult::Open {
                text: "example.com/docs".into(),
                is_search: false,
            }]
        );
    }

    #[test]
    fn the_bar_grows_by_one_row_per_result() {
        assert_eq!(bar_height(0), 98);
        assert_eq!(bar_height(1), 146);
        assert_eq!(bar_height(3), 226);
    }

    #[test]
    fn the_bars_window_is_centered_over_the_page_with_room_for_its_shadow() {
        let options = view_options(2);
        assert_eq!(
            options.placement,
            Placement {
                left: Some(236),
                right: Some(8),
                width: Some(796),
                top: Some(-24),
                height: Some(442),
                bottom: None,
            }
        );
        // The blurred area is the bar itself, inside the shadow's margin.
        assert_eq!(options.backdrop_inset, 128);
        assert_eq!(options.backdrop_blur, 28.);
    }
}
