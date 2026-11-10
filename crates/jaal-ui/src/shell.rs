//! The sidebar, and the state of one browser window.
//!
//! The engine owns the window and the page. This view is a GPUI window that
//! the engine keeps as a child of its own, down the left edge. The command
//! bar is a second GPUI window the engine keeps over the page
//! (`command_bar.rs`). Tabs live in the engine: this view only mirrors what
//! the engine's events say.

use crate::command_bar::{BarMode, CommandBar};
use crate::icons;
use crate::log;
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, PAGE_FRAME, PAGE_RADIUS, SIDEBAR_WIDTH, Theme};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, KeyBinding, MouseButton,
    MouseDownEvent, SharedString, Subscription, UniformListScrollHandle, Window, WindowHandle,
    actions, div, linear_color_stop, linear_gradient, prelude::*, px, svg, uniform_list,
};
use jaal_bakeoff_shared::bench::{frames_enabled, mark};
use jaal_bakeoff_shared::bench_modes::{self as bench, BenchConfig};
use jaal_engine::NativeView;
use jaal_engine::{
    Engine, Event, Key, NavAction, Placement, Tab, TabState, ViewOptions, WindowInfo, WindowState,
    WindowStyle,
};
use std::rc::Rc;

actions!(
    jaal,
    [
        NewTab,
        CloseTab,
        FocusAddress,
        ToggleCommandBar,
        ToggleFullscreen
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-t", NewTab, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-l", FocusAddress, None),
        KeyBinding::new("ctrl-k", ToggleCommandBar, None),
        KeyBinding::new("f11", ToggleFullscreen, None),
    ]);
}

// Windows virtual-key codes, as the engine reports keys.
/// The height of a tab row and of a test row, in DIPs.
const ROW_HEIGHT: f32 = 30.;

const VK_K: u32 = 0x4b;
const VK_L: u32 = 0x4c;
const VK_T: u32 = 0x54;
const VK_W: u32 = 0x57;
const VK_F11: u32 = 0x7a;

fn ctrl(code: u32) -> Key {
    Key {
        code,
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    }
}

/// The shortcuts a page must never get, because the user expects them to
/// work whatever the page does. The engine asks this from the middle of
/// Chrome's key handling, so it only looks the key up.
pub fn is_reserved_shortcut(key: Key) -> bool {
    key == ctrl(VK_T) || key == ctrl(VK_W) || key == ctrl(VK_L)
}

/// What a key that came back from the engine does. Reserved keys arrive
/// before the page saw them; the rest only after the page left them alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    NewTab,
    CloseTab,
    FocusAddress,
    ToggleCommandBar,
    ToggleFullscreen,
}

pub fn shortcut_for(key: Key) -> Option<Shortcut> {
    if key == ctrl(VK_T) {
        Some(Shortcut::NewTab)
    } else if key == ctrl(VK_W) {
        Some(Shortcut::CloseTab)
    } else if key == ctrl(VK_L) {
        Some(Shortcut::FocusAddress)
    } else if key == ctrl(VK_K) {
        Some(Shortcut::ToggleCommandBar)
    } else if key.code == VK_F11 && !key.ctrl && !key.alt && !key.shift && !key.meta {
        Some(Shortcut::ToggleFullscreen)
    } else {
        None
    }
}

/// A URL as the address field shows it at rest: the host, then the rest.
pub fn split_url(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    if url.starts_with("file:") {
        // A local file has no host worth showing; its name is what matters.
        let name = rest.rsplit('/').next().unwrap_or(rest);
        return ("file".into(), format!("/{name}"));
    }
    let (host, path) = match rest.find(['/', '?', '#']) {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let host = host.strip_prefix("www.").unwrap_or(host);
    (
        host.into(),
        if path == "/" {
            String::new()
        } else {
            path.into()
        },
    )
}

/// A tab's row: its title, or the address while the page has none.
pub fn tab_label(title: &str, url: &str) -> String {
    if !title.trim().is_empty() {
        return title.into();
    }
    let (host, path) = split_url(url);
    if host.is_empty() {
        "New tab".into()
    } else {
        format!("{host}{path}")
    }
}

pub struct TabRow {
    pub tab: Tab,
    pub title: SharedString,
    pub url: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// What a launch asked for, from the environment.
pub struct Config {
    pub urls: Vec<String>,
    /// How many of the bake-off's fake rows to list under the tabs. The
    /// scroll test uses 5,000; a normal launch shows none.
    pub fake_rows: usize,
    pub dark: Option<bool>,
    pub frameless: bool,
    /// The bake-off's bench modes (`JAAL_BENCH_*`), for engine/scripts/bench.ts.
    pub bench: BenchConfig,
}

pub struct Shell {
    engine: Engine,
    engine_window: jaal_engine::Window,
    /// This sidebar's own native view.
    view: NativeView,
    tabs: Vec<TabRow>,
    active: Option<Tab>,
    rows: Rc<[SharedString]>,
    scroll: UniformListScrollHandle,
    last_logged_scroll: usize,
    address: Entity<TextInput>,
    editing_address: bool,
    command_bar: Option<(WindowHandle<CommandBar>, NativeView)>,
    info: Option<WindowInfo>,
    /// The layout last given to the engine: with the sidebar, or the page
    /// alone (fullscreen).
    page_alone: Option<bool>,
    theme: Theme,
    bench: BenchConfig,
    /// The first tab has finished loading once.
    first_load_marked: bool,
    /// A scroll run is going: `render` marks frames.
    bench_running: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(
        engine: Engine,
        engine_window: jaal_engine::Window,
        config: &Config,
        mut events: UnboundedReceiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let dark = config.dark.unwrap_or_else(|| {
            matches!(
                window.appearance(),
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
            )
        });
        let theme = Theme::jaal(dark);
        let view = crate::native_view(window);

        let address = cx.new(|cx| {
            TextInput::new(
                "Search or enter address",
                theme.ink.into(),
                theme.ink_3.into(),
                theme.accent.into(),
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &address,
                window,
                |this, _, event: &TextInputEvent, window, cx| match event {
                    TextInputEvent::Submit(text) => {
                        let text = text.clone();
                        this.navigate(&text, cx);
                        this.stop_editing_address(window, cx);
                    }
                    TextInputEvent::Cancel => this.stop_editing_address(window, cx),
                    TextInputEvent::Changed(_) => {}
                },
            ),
            // The sidebar is "active" while it has keyboard focus. When the
            // user clicks the page, the address stops being edited.
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() && this.editing_address {
                    this.editing_address = false;
                    this.show_active_url(cx);
                    cx.notify();
                }
            }),
        ];

        // Engine events arrive from Chromium's loop, outside any GPUI update,
        // so they cross over on a channel.
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                let alive = this.update_in(cx, |this, window, cx| {
                    this.on_engine_event(event, window, cx)
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        log(&format!("ui: {} test rows", config.fake_rows));
        let this = Self {
            engine,
            engine_window,
            view,
            tabs: Vec::new(),
            active: None,
            rows: jaal_bakeoff_shared::fake_rows(config.fake_rows)
                .into_iter()
                .map(|row| row.title.into())
                .collect(),
            scroll: UniformListScrollHandle::new(),
            last_logged_scroll: 0,
            address,
            editing_address: false,
            command_bar: None,
            info: None,
            page_alone: None,
            theme,
            bench: config.bench.clone(),
            first_load_marked: false,
            bench_running: false,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.engine
            .attach_view(
                engine_window,
                view,
                ViewOptions::opaque(Placement {
                    left: Some(0),
                    top: Some(0),
                    bottom: Some(0),
                    width: Some(SIDEBAR_WIDTH),
                    ..Placement::default()
                }),
            )
            .expect("attach the sidebar");
        this.engine
            .set_window_style(
                engine_window,
                WindowStyle {
                    background_top: theme::argb(this.theme.chrome_top),
                    background_bottom: theme::argb(this.theme.chrome_bottom),
                    page_corner_radius: PAGE_RADIUS,
                    page_shadow_color: theme::argb(this.theme.card_shadow),
                    page_shadow_blur: 10.,
                    page_shadow_offset_y: 2.,
                    page_outline_color: theme::argb(this.theme.card_outline),
                },
            )
            .expect("style the window");
        this
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn tabs(&self) -> &[TabRow] {
        &self.tabs
    }

    fn active_row(&self) -> Option<&TabRow> {
        self.tabs.iter().find(|row| Some(row.tab) == self.active)
    }

    fn on_engine_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::TabInserted(state) | Event::TabState(state) => {
                let loaded = !state.loading && !state.url.is_empty();
                self.on_tab_state(state, cx);
                if loaded && !self.first_load_marked {
                    self.first_load_marked = true;
                    mark("first_load");
                    self.start_frame_bench(window, cx);
                }
            }
            Event::TabRemoved(tab) => {
                self.tabs.retain(|row| row.tab != tab);
                if self.active == Some(tab) {
                    self.active = None;
                }
                cx.notify();
            }
            Event::Key { key, .. } => {
                if let Some(shortcut) = shortcut_for(key) {
                    self.run_shortcut(shortcut, window, cx);
                }
            }
            Event::WindowState(info) if info.window == self.engine_window => {
                self.info = Some(info);
                self.apply_layout(cx);
                cx.notify();
            }
            _ => {}
        }
    }

    fn on_tab_state(&mut self, state: TabState, cx: &mut Context<Self>) {
        let title: SharedString = tab_label(&state.title, &state.url).into();
        match self.tabs.iter_mut().find(|row| row.tab == state.tab) {
            Some(row) => {
                row.title = title;
                row.url = state.url;
                row.loading = state.loading;
                row.can_go_back = state.can_go_back;
                row.can_go_forward = state.can_go_forward;
            }
            None => self.tabs.push(TabRow {
                tab: state.tab,
                title,
                url: state.url,
                loading: state.loading,
                can_go_back: state.can_go_back,
                can_go_forward: state.can_go_forward,
            }),
        }
        if state.active {
            self.active = Some(state.tab);
        }
        if !self.editing_address {
            self.show_active_url(cx);
        }
        cx.notify();
    }

    fn show_active_url(&mut self, cx: &mut Context<Self>) {
        let url = self
            .active_row()
            .map(|row| row.url.clone())
            .unwrap_or_default();
        self.address.update(cx, |input, cx| input.set_text(url, cx));
    }

    /// With the sidebar: the page is a card inside a frame of chrome. In
    /// fullscreen the page takes the whole window and the sidebar hides.
    fn apply_layout(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.info else { return };
        let page_alone = info.state == WindowState::Fullscreen;
        if self.page_alone == Some(page_alone) {
            return;
        }
        self.page_alone = Some(page_alone);
        log(&format!(
            "ui: layout {}",
            if page_alone {
                "page alone"
            } else {
                "sidebar and page"
            }
        ));
        let sidebar = Placement {
            left: Some(0),
            top: Some(0),
            bottom: Some(0),
            width: Some(SIDEBAR_WIDTH),
            ..Placement::default()
        };
        let _ = self.engine.update_view(
            self.engine_window,
            self.view,
            ViewOptions {
                visible: !page_alone,
                ..ViewOptions::opaque(sidebar)
            },
        );
        let _ = self.engine.set_page_placement(
            self.engine_window,
            if page_alone {
                Placement::inset(0, 0, 0, 0)
            } else {
                Placement::inset(SIDEBAR_WIDTH, PAGE_FRAME, PAGE_FRAME, PAGE_FRAME)
            },
        );
        let _ = self.engine.set_window_style(
            self.engine_window,
            WindowStyle {
                background_top: theme::argb(self.theme.chrome_top),
                background_bottom: theme::argb(self.theme.chrome_bottom),
                page_corner_radius: if page_alone { 0. } else { PAGE_RADIUS },
                page_shadow_color: theme::argb(self.theme.card_shadow),
                page_shadow_blur: 10.,
                page_shadow_offset_y: 2.,
                page_outline_color: theme::argb(self.theme.card_outline),
            },
        );
        if page_alone {
            self.close_command_bar(false, cx);
        }
    }

    fn run_shortcut(&mut self, shortcut: Shortcut, window: &mut Window, cx: &mut Context<Self>) {
        log(&format!("ui: shortcut {shortcut:?}"));
        match shortcut {
            Shortcut::NewTab => self.open_command_bar(BarMode::NewTab, cx),
            Shortcut::CloseTab => self.close_active_tab(),
            Shortcut::FocusAddress => self.edit_address(window, cx),
            Shortcut::ToggleCommandBar => {
                if self.command_bar.is_some() {
                    self.close_command_bar(true, cx);
                } else {
                    self.open_command_bar(BarMode::Navigate, cx);
                }
            }
            Shortcut::ToggleFullscreen => {
                let fullscreen = self
                    .info
                    .is_some_and(|info| info.state == WindowState::Fullscreen);
                let _ = self.engine.set_window_state(
                    self.engine_window,
                    if fullscreen {
                        WindowState::Normal
                    } else {
                        WindowState::Fullscreen
                    },
                );
            }
        }
    }

    fn close_active_tab(&mut self) {
        if let Some(tab) = self.active {
            let _ = self.engine.close_tab(tab);
        }
    }

    /// Loads what was typed in the active tab: a URL, a bare host, or a
    /// search.
    pub fn navigate(&mut self, text: &str, _cx: &mut Context<Self>) {
        let Some(url) = jaal_bakeoff_shared::normalize_url(text) else {
            return;
        };
        log(&format!("ui: navigate {url}"));
        match self.active {
            Some(tab) => {
                let _ = self.engine.load(tab, &url);
            }
            None => {
                let _ = self.engine.open_tab(self.engine_window, &url, true);
            }
        }
    }

    pub fn open_in_new_tab(&mut self, text: &str) {
        if let Some(url) = jaal_bakeoff_shared::normalize_url(text) {
            log(&format!("ui: new tab {url}"));
            let _ = self.engine.open_tab(self.engine_window, &url, true);
        }
    }

    pub fn activate_tab(&mut self, tab: Tab) {
        let _ = self.engine.activate_tab(tab);
        let _ = self.engine.focus_page(self.engine_window);
    }

    fn edit_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_command_bar(false, cx);
        self.editing_address = true;
        self.show_active_url(cx);
        self.address
            .update(cx, |input, cx| input.select_all_text(cx));
        // Keyboard focus moves from the page to this sidebar's view, then to
        // the field inside it.
        window.activate_window();
        window.focus(&self.address.read(cx).focus_handle(cx));
        log("ui: address focused");
        cx.notify();
    }

    fn stop_editing_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_address = false;
        self.show_active_url(cx);
        window.focus(&self.focus_handle);
        let _ = self.engine.focus_page(self.engine_window);
        cx.notify();
    }

    fn open_command_bar(&mut self, mode: BarMode, cx: &mut Context<Self>) {
        if self.page_alone == Some(true) {
            return;
        }
        self.close_command_bar(false, cx);
        let shell = cx.entity().downgrade();
        let tabs = self
            .tabs
            .iter()
            .map(|row| (row.tab, row.title.clone(), row.url.clone()))
            .collect();
        match CommandBar::open(
            shell,
            self.engine,
            self.engine_window,
            mode,
            tabs,
            self.theme.clone(),
            cx,
        ) {
            Some(bar) => {
                log(&format!("ui: command bar opened ({mode:?})"));
                self.command_bar = Some(bar);
            }
            None => log("ui: could not open the command bar"),
        }
    }

    /// Closes the command bar. `refocus_page` hands keyboard focus back to
    /// the page; pass false when focus already went somewhere on purpose.
    pub fn close_command_bar(&mut self, refocus_page: bool, cx: &mut Context<Self>) {
        let Some((bar, view)) = self.command_bar.take() else {
            return;
        };
        log("ui: command bar closed");
        let _ = self.engine.detach_view(self.engine_window, view);
        let _ = bar.update(cx, |_, window, _| window.remove_window());
        if refocus_page {
            let _ = self.engine.focus_page(self.engine_window);
        }
    }

    fn start_frame_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bench.scroll {
            self.bench_tick(std::time::Instant::now(), window, cx);
        }
    }

    /// One frame of the bake-off's scroll run: move the list, then ask for
    /// the next frame. `render` writes the `frame` marks. The same steps as
    /// gpui-a's, so the two compare.
    fn bench_tick(
        &mut self,
        started: std::time::Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if started.elapsed() >= bench::RUN_TIME {
            self.bench_running = false;
            let _ = self.engine.quit();
            return;
        }
        self.bench_running = true;
        let handle = self.scroll.0.borrow().base_handle.clone();
        let offset = handle.offset();
        let mut y = offset.y - px(bench::SCROLL_PX_PER_FRAME);
        if -y > handle.max_offset().height {
            y = px(0.);
        }
        handle.set_offset(gpui::point(offset.x, y));
        cx.notify();
        let this = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = this.update(cx, |this, cx| this.bench_tick(started, window, cx));
        });
    }

    fn navigate_active(&mut self, action: NavAction) {
        if let Some(tab) = self.active {
            let _ = self.engine.navigate(tab, action);
            let _ = self.engine.focus_page(self.engine_window);
        }
    }

    fn set_window_state(&mut self, state: WindowState) {
        let _ = self.engine.set_window_state(self.engine_window, state);
    }

    fn render_top_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let maximized = self
            .info
            .is_some_and(|info| info.state == WindowState::Maximized);
        let caption_button = |id: &'static str, icon: &'static str| {
            div()
                .id(id)
                .w(px(27.))
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .hover(|style| style.bg(theme.fill))
                .active(|style| style.bg(theme.line))
                // The row under the buttons drags the window.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(svg().path(icon).size(px(10.)).text_color(theme.ink))
        };
        let active = self.active_row();
        let has_tab = active.is_some();
        let can_go_back = active.is_some_and(|row| row.can_go_back);
        let can_go_forward = active.is_some_and(|row| row.can_go_forward);
        let loading = active.is_some_and(|row| row.loading);
        // A 16 px icon in a 26 px target. A button with nothing to do keeps
        // its place and goes quiet.
        let nav_button = |id: &'static str, icon: &'static str, enabled: bool| {
            div()
                .id(id)
                .size(px(26.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .when(enabled, |button| {
                    button
                        .hover(|style| style.bg(theme.fill))
                        .active(|style| style.bg(theme.line))
                })
                .child(svg().path(icon).size(px(16.)).text_color(if enabled {
                    theme.ink_2
                } else {
                    theme.ink_3
                }))
        };
        div()
            .id("top")
            .h(px(42.))
            .flex_none()
            .flex()
            .items_center()
            // The window has no system title bar, so this row is one: drag
            // it to move the window, double-click it to maximize.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    if event.click_count == 2 {
                        let maximized = this
                            .info
                            .is_some_and(|info| info.state == WindowState::Maximized);
                        this.set_window_state(if maximized {
                            WindowState::Normal
                        } else {
                            WindowState::Maximized
                        });
                    } else {
                        let _ = this.engine.begin_window_drag(this.engine_window);
                    }
                }),
            )
            .child(
                nav_button("back", icons::BACK, can_go_back)
                    .on_click(cx.listener(|this, _, _, _| this.navigate_active(NavAction::Back))),
            )
            .child(
                nav_button("forward", icons::FORWARD, can_go_forward).on_click(
                    cx.listener(|this, _, _, _| this.navigate_active(NavAction::Forward)),
                ),
            )
            .child(
                // While the page loads, reload becomes stop.
                nav_button(
                    "reload",
                    if loading { icons::CLOSE } else { icons::RELOAD },
                    has_tab,
                )
                .on_click(cx.listener(move |this, _, _, _| {
                    this.navigate_active(if loading {
                        NavAction::Stop
                    } else {
                        NavAction::Reload
                    })
                })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .mr(px(-4.))
                    .child(caption_button("minimize", icons::WINDOW_MINIMIZE).on_click(
                        cx.listener(|this, _, _, _| this.set_window_state(WindowState::Minimized)),
                    ))
                    .child(
                        caption_button(
                            "maximize",
                            if maximized {
                                icons::WINDOW_RESTORE
                            } else {
                                icons::WINDOW_MAXIMIZE
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.set_window_state(if maximized {
                                WindowState::Normal
                            } else {
                                WindowState::Maximized
                            })
                        })),
                    )
                    .child(
                        caption_button("close", icons::WINDOW_CLOSE).on_click(cx.listener(
                            |this, _, _, _| {
                                let _ = this.engine.close_window(this.engine_window);
                            },
                        )),
                    ),
            )
    }

    fn render_address(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let field = div()
            .id("address")
            .h(px(32.))
            .flex_none()
            .px(px(9.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(9.))
            .text_size(px(12.5))
            .line_height(px(18.))
            .overflow_hidden();
        if self.editing_address {
            // The focused field is raised, with the accent as its ring.
            return field
                .bg(theme.raised)
                .border_2()
                .border_color(theme.accent)
                .px(px(7.))
                .child(self.address.clone());
        }
        let (host, path) = self
            .active_row()
            .map(|row| split_url(&row.url))
            .unwrap_or_default();
        let loading = self.active_row().is_some_and(|row| row.loading);
        field
            .relative()
            .when(loading, |field| {
                // The design's loading mark: an accent line along the
                // field's bottom edge.
                field.child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .h(px(2.))
                        .w(gpui::relative(0.55))
                        .bg(theme.accent),
                )
            })
            .bg(theme.fill)
            .hover(|style| style.bg(theme.line))
            .on_click(cx.listener(|this, _, window, cx| this.edit_address(window, cx)))
            .child(
                svg()
                    .path(icons::SITE)
                    .size(px(16.))
                    .flex_none()
                    .text_color(theme.ink_2),
            )
            .child(if host.is_empty() {
                div()
                    .flex_1()
                    .truncate()
                    .text_color(theme.ink_3)
                    .child("Search or enter address")
            } else {
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.ink)
                            .child(host),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.ink_2)
                            .child(path),
                    )
            })
    }

    fn render_tab(&self, row: &TabRow, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let tab = row.tab;
        let active = Some(tab) == self.active;
        let (host, _) = split_url(&row.url);
        div()
            .id(("tab", tab_element_id(tab)))
            .group("tab")
            .h(px(30.))
            .flex_none()
            .px(px(9.))
            .flex()
            .items_center()
            .gap(px(9.))
            .rounded(px(8.))
            .when(active, |row| {
                row.bg(theme.raised).shadow(theme.raised_shadow.clone())
            })
            .when(!active, |row| row.hover(|style| style.bg(theme.fill_hover)))
            .on_click(cx.listener(move |this, _, _, _| this.activate_tab(tab)))
            .child(favicon(&host, theme))
            .child(div().flex_1().truncate().child(row.title.clone()))
            .child(
                // The close button shows on the active row and on hover. Its
                // hit area is larger than the cross.
                div()
                    .id(("close", tab_element_id(tab)))
                    .size(px(22.))
                    .mr(px(-4.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .when(!active, |close| {
                        close
                            .invisible()
                            .group_hover("tab", |style| style.visible())
                    })
                    .hover(|style| style.bg(theme.fill))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        let _ = this.engine.close_tab(tab);
                    }))
                    .child(
                        svg()
                            .path(icons::CLOSE)
                            .size(px(16.))
                            .text_color(theme.ink_3),
                    ),
            )
    }
}

fn tab_element_id(tab: Tab) -> usize {
    // Unique per live tab: the handle's slot, and its generation so a reused
    // slot is a new element.
    let handle = tab.raw();
    (handle.slot as usize) << 32 | handle.generation as usize
}

/// A site's stand-in icon: its first letter on a tile whose color comes from
/// the host, until real favicons cross the engine API.
pub(crate) fn favicon(host: &str, theme: &Theme) -> impl IntoElement + use<> {
    let letter = host
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "·".into());
    let hue = host.bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u32)
    }) % 360;
    div()
        .size(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(theme::oklch(
            if theme.dark { 0.62 } else { 0.56 },
            0.13,
            hue as f32,
            1.,
        ))
        .text_size(px(9.))
        .font_weight(FontWeight::BOLD)
        .text_color(gpui::white())
        .child(letter)
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.bench_running && frames_enabled() {
            mark("frame");
        }
        // The acceptance run reads how far the list has scrolled.
        // Rows have one height, so the offset tells which row is at the top.
        let top = (-f32::from(self.scroll.0.borrow().base_handle.offset().y) / ROW_HEIGHT)
            .round()
            .max(0.) as usize;
        if top != self.last_logged_scroll {
            self.last_logged_scroll = top;
            log(&format!("ui: list top row {top}"));
        }

        let theme = self.theme.clone();
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .map(|row| self.render_tab(row, cx).into_any_element())
            .collect();
        let rows = self.rows.clone();
        let row_ink = theme.ink_2;
        let row_hover = theme.fill_hover;

        div()
            .size_full()
            .flex()
            .flex_col()
            .px(px(10.))
            // The same gradient the engine paints behind the page, so the
            // sidebar and the frame around the page read as one surface.
            .bg(linear_gradient(
                180.,
                linear_color_stop(theme.chrome_top, 0.),
                linear_color_stop(theme.chrome_bottom, 1.),
            ))
            .font_family(theme::FONT)
            .text_size(px(13.))
            .line_height(px(17.))
            .text_color(theme.ink)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                this.run_shortcut(Shortcut::NewTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.run_shortcut(Shortcut::CloseTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusAddress, window, cx| {
                this.run_shortcut(Shortcut::FocusAddress, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleCommandBar, window, cx| {
                this.run_shortcut(Shortcut::ToggleCommandBar, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleFullscreen, window, cx| {
                this.run_shortcut(Shortcut::ToggleFullscreen, window, cx)
            }))
            .child(self.render_top_row(cx))
            .child(self.render_address(cx))
            .child(
                div()
                    .mt(px(14.))
                    .h(px(24.))
                    .flex_none()
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ink_2)
                    .child(
                        svg()
                            .path(icons::SPACE)
                            .size(px(14.))
                            .text_color(theme.accent),
                    )
                    .child("Jaal"),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_none()
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(theme.ink_3)
                    .child("Today")
                    .child(div().flex_1().h(px(1.)).bg(theme.line)),
            )
            .child(
                div()
                    .id("new-tab")
                    .h(px(30.))
                    .flex_none()
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .rounded(px(8.))
                    .text_color(theme.ink_2)
                    .hover(|style| style.bg(theme.fill_hover))
                    .on_click(
                        cx.listener(|this, _, _, cx| this.open_command_bar(BarMode::NewTab, cx)),
                    )
                    .child(
                        svg()
                            .path(icons::PLUS)
                            .size(px(16.))
                            .text_color(theme.ink_3),
                    )
                    .child(div().flex_1().child("New tab"))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.ink_3)
                            .child("Ctrl+T"),
                    ),
            )
            .child(div().flex().flex_col().gap(px(2.)).children(tabs))
            .when(!rows.is_empty(), |sidebar| {
                sidebar
                    .child(
                        div()
                            .h(px(28.))
                            .flex_none()
                            .px(px(9.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(11.))
                            .text_color(theme.ink_3)
                            .child("Test rows")
                            .child(div().flex_1().h(px(1.)).bg(theme.line)),
                    )
                    .child(
                        uniform_list("rows", rows.len(), move |range, _window, _cx| {
                            range
                                .map(|ix| {
                                    div()
                                        .id(ix)
                                        .h(px(ROW_HEIGHT))
                                        .px(px(9.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(8.))
                                        .text_color(row_ink)
                                        .hover(|style| style.bg(row_hover))
                                        .child(rows[ix].clone())
                                })
                                .collect()
                        })
                        .track_scroll(self.scroll.clone())
                        .flex_1(),
                    )
            })
            .when(self.rows.is_empty(), |sidebar| {
                sidebar.child(div().flex_1())
            })
            .child(div().h(px(10.)).flex_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: u32, ctrl: bool, shift: bool) -> Key {
        Key {
            code,
            shift,
            ctrl,
            alt: false,
            meta: false,
        }
    }

    #[test]
    fn ctrl_t_w_and_l_are_reserved_and_nothing_else_is() {
        assert!(is_reserved_shortcut(key(VK_T, true, false)));
        assert!(is_reserved_shortcut(key(VK_W, true, false)));
        assert!(is_reserved_shortcut(key(VK_L, true, false)));
        // Ctrl+K opens the command bar, but only after the page left it.
        assert!(!is_reserved_shortcut(key(VK_K, true, false)));
        // A plain letter and a different chord belong to the page.
        assert!(!is_reserved_shortcut(key(VK_T, false, false)));
        assert!(!is_reserved_shortcut(key(VK_T, true, true)));
        assert!(!is_reserved_shortcut(key(VK_F11, false, false)));
    }

    #[test]
    fn keys_map_to_their_shortcuts() {
        assert_eq!(shortcut_for(key(VK_T, true, false)), Some(Shortcut::NewTab));
        assert_eq!(
            shortcut_for(key(VK_W, true, false)),
            Some(Shortcut::CloseTab)
        );
        assert_eq!(
            shortcut_for(key(VK_L, true, false)),
            Some(Shortcut::FocusAddress)
        );
        assert_eq!(
            shortcut_for(key(VK_K, true, false)),
            Some(Shortcut::ToggleCommandBar)
        );
        assert_eq!(
            shortcut_for(key(VK_F11, false, false)),
            Some(Shortcut::ToggleFullscreen)
        );
        assert_eq!(shortcut_for(key(VK_K, false, false)), None);
        assert_eq!(shortcut_for(key(0x42, true, false)), None);
    }

    #[test]
    fn a_url_splits_into_the_host_and_the_rest() {
        assert_eq!(
            split_url("https://www.example.com/a/b?c=d"),
            ("example.com".into(), "/a/b?c=d".into())
        );
        assert_eq!(
            split_url("https://example.com/"),
            ("example.com".into(), String::new())
        );
        assert_eq!(
            split_url("https://example.com"),
            ("example.com".into(), String::new())
        );
        assert_eq!(
            split_url("file:///C:/src/jaal/engine/test/m2-page.html"),
            ("file".into(), "/m2-page.html".into())
        );
        assert_eq!(split_url(""), (String::new(), String::new()));
    }

    #[test]
    fn a_tab_is_labelled_by_its_title_or_else_its_address() {
        assert_eq!(
            tab_label("Example Domain", "https://example.com/"),
            "Example Domain"
        );
        assert_eq!(
            tab_label("", "https://example.com/docs"),
            "example.com/docs"
        );
        assert_eq!(tab_label("  ", ""), "New tab");
    }
}
