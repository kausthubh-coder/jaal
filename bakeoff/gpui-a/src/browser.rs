//! The main window: GPUI draws the sidebar and the frame around the page;
//! each tab is a CEF browser whose native view (NSView, or HWND on Windows)
//! sits over the page area. Only the active tab's view is visible.

use crate::command_bar::CommandBar;
use crate::overlay;
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, Pixels, SharedString,
    Subscription, UniformListScrollHandle, Window, WindowHandle, actions, canvas, div, point,
    prelude::*, px, rgb, rgba, size, uniform_list,
};
use jaal_bakeoff_shared::bench::{mark, mark_at};
use jaal_bakeoff_shared::bench_modes::{self as bench, BenchConfig};
use jaal_bakeoff_shared::{TabId, Tabs, fake_rows, normalize_url, test_page_url};
use jaal_gpui_cef::text_input::{TextInput, TextInputEvent};
use jaal_gpui_cef::{BrowserEvent, ChildBrowser};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

pub const SIDEBAR_WIDTH: f32 = 264.;
const PAGE_INSET: f32 = 8.;
const ROW_HEIGHT: f32 = 28.;
const FAKE_ROWS: usize = 5000;

pub const SIDEBAR_BG: u32 = 0xe9e6e1;
const TEXT: u32 = 0x24211d;
const TEXT_MUTED: u32 = 0x77716a;

actions!(gpui_a, [NewTab, CloseTab, ToggleCommandBar, Quit]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        // secondary: Cmd on macOS, Ctrl on Windows.
        KeyBinding::new("secondary-t", NewTab, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("secondary-k", ToggleCommandBar, None),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
}

const TAB_SHOWN_PREFIX: &str = "jaal:tab_shown:";
const TAB_SHOWN_JS: &str = "requestAnimationFrame(() => requestAnimationFrame(() => \
     console.log('jaal:tab_shown:' + (performance.timeOrigin + performance.now()))))";

/// What the CEF callbacks queue for the main window.
pub enum PageEvent {
    Browser(BrowserEvent),
    FirstPaint,
    TabsLoaded,
}

/// Every view in the app reaches the main window through this.
pub type BrowserHandle = WindowHandle<BrowserWindow>;

pub struct BrowserWindow {
    tabs: Tabs,
    pages: HashMap<TabId, ChildBrowser>,
    titles: HashMap<TabId, SharedString>,
    tab_of_browser: HashMap<i32, TabId>,
    /// Browsers asked to close that haven't sent `BeforeClose` yet.
    closing: HashMap<i32, ChildBrowser>,
    client: jaal_gpui_cef::cef::Client,
    /// The first tab's browser, whose main-frame load is `first_paint`.
    first_browser: Rc<Cell<i32>>,
    gpui_view: *mut c_void,
    rows: Rc<[SharedString]>,
    scroll: UniformListScrollHandle,
    url_input: Entity<TextInput>,
    /// The page area from the last frame's layout, in window coordinates.
    page_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    next_test_page: usize,
    command_bar: Option<WindowHandle<CommandBar>>,
    quitting: bool,
    bench: BenchConfig,
    /// A scroll or resize run is going: `render` marks frames.
    bench_running: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl BrowserWindow {
    pub fn new(
        urls: Vec<String>,
        bench: BenchConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let gpui_view = jaal_gpui_cef::native_view(window);
        #[cfg(target_os = "macos")]
        eprintln!("gpui-a: main window CGWindowID {}", unsafe {
            jaal_gpui_cef::window_number(gpui_view)
        });

        let (events, mut event_rx) = mpsc::unbounded::<PageEvent>();
        let first_browser = Rc::new(Cell::new(-1));
        let client = jaal_gpui_cef::client(Self::event_handler(
            events,
            first_browser.clone(),
            bench.tabs,
        ));
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = event_rx.next().await {
                let alive =
                    this.update_in(cx, |this, window, cx| this.on_page_event(event, window, cx));
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        let url_input = cx.new(|cx| {
            TextInput::new(
                "Search or enter URL",
                rgb(TEXT).into(),
                rgba(0x24211d66).into(),
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &url_input,
                window,
                |this, _input, event: &TextInputEvent, window, cx| match event {
                    TextInputEvent::Submit(text) => this.navigate(text, window, cx),
                    TextInputEvent::Cancel => this.show_active(window, cx),
                },
            ),
            // Windows doesn't carry an owned window along when its owner
            // moves (see overlay.rs); AppKit does, and a resize re-renders.
            cx.observe_window_bounds(window, |this, _window, cx| {
                if cfg!(target_os = "windows") {
                    this.place_command_bar(cx);
                }
            }),
        ];

        let rows: Rc<[SharedString]> = fake_rows(FAKE_ROWS)
            .into_iter()
            .map(|row| SharedString::from(row.title))
            .collect();

        let mut this = Self {
            tabs: Tabs::default(),
            pages: HashMap::new(),
            titles: HashMap::new(),
            tab_of_browser: HashMap::new(),
            closing: HashMap::new(),
            client,
            first_browser,
            gpui_view,
            rows,
            scroll: UniformListScrollHandle::new(),
            url_input,
            page_bounds: Rc::new(Cell::new(None)),
            next_test_page: urls.len() + 1,
            command_bar: None,
            quitting: false,
            bench,
            bench_running: false,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        for url in urls {
            this.open_tab(url, window, cx);
        }
        if let Some(first) = this.tabs.list().first().map(|t| t.id) {
            this.tabs.activate(first);
            this.show_active(window, cx);
        }
        this
    }

    /// Runs inside CEF callbacks: marks what must be timed exactly there,
    /// queues the rest for GPUI.
    fn event_handler(
        events: mpsc::UnboundedSender<PageEvent>,
        first_browser: Rc<Cell<i32>>,
        bench_tabs: Option<usize>,
    ) -> jaal_gpui_cef::EventHandler {
        let painted = Cell::new(false);
        let loaded = RefCell::new(HashSet::new());
        Rc::new(move |event: BrowserEvent| {
            let mut handled = false;
            match &event {
                BrowserEvent::LoadEnd {
                    browser_id,
                    main_frame: true,
                } => {
                    if *browser_id == first_browser.get() && !painted.replace(true) {
                        mark("first_paint");
                        let _ = events.unbounded_send(PageEvent::FirstPaint);
                    }
                    if let Some(n) = bench_tabs {
                        let mut loaded = loaded.borrow_mut();
                        if loaded.len() < n && loaded.insert(*browser_id) && loaded.len() == n {
                            mark("tabs_loaded");
                            let _ = events.unbounded_send(PageEvent::TabsLoaded);
                        }
                    }
                }
                BrowserEvent::CommandKey { key, .. } => {
                    handled = matches!(key, 't' | 'w' | 'k' | 'q')
                }
                BrowserEvent::Console { message, .. } => {
                    if let Some(ms) = message.strip_prefix(TAB_SHOWN_PREFIX) {
                        if let Ok(ms) = ms.parse::<f64>() {
                            mark_at("tab_switch_done", ms);
                        }
                        return true;
                    }
                }
                _ => {}
            }
            let _ = events.unbounded_send(PageEvent::Browser(event));
            handled
        })
    }

    fn on_page_event(&mut self, event: PageEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            PageEvent::FirstPaint => {
                self.start_frame_bench(window, cx);
                if crate::demo::enabled() {
                    cx.spawn_in(window, crate::demo::run).detach();
                }
            }
            PageEvent::TabsLoaded => self.start_switch_bench(window, cx),
            PageEvent::Browser(BrowserEvent::AddressChange { browser_id, url }) => {
                if let Some(&tab) = self.tab_of_browser.get(&browser_id) {
                    self.tabs.set_url(tab, url);
                    if self.tabs.active() == Some(tab) {
                        self.sync_url_field(window, cx);
                    }
                    cx.notify();
                }
            }
            PageEvent::Browser(BrowserEvent::TitleChange { browser_id, title }) => {
                if let Some(&tab) = self.tab_of_browser.get(&browser_id) {
                    self.titles.insert(tab, title.into());
                    cx.notify();
                }
            }
            PageEvent::Browser(BrowserEvent::BeforeClose { browser_id }) => {
                self.closing.remove(&browser_id);
                if self.quitting && self.closing.is_empty() {
                    finish_quit(cx);
                }
            }
            PageEvent::Browser(BrowserEvent::CommandKey { key, .. }) => match key {
                't' => self.new_tab(window, cx),
                'w' => self.close_active_tab(window, cx),
                'k' => self.toggle_command_bar(window, cx),
                'q' => self.quit(cx),
                _ => {}
            },
            PageEvent::Browser(
                BrowserEvent::LoadEnd { .. }
                | BrowserEvent::Console { .. }
                | BrowserEvent::CursorChange { .. },
            ) => {}
        }
    }

    pub fn tabs(&self) -> &Tabs {
        &self.tabs
    }

    pub fn tab_label(&self, id: TabId) -> SharedString {
        self.titles.get(&id).cloned().unwrap_or_else(|| {
            let url = self
                .tabs
                .list()
                .iter()
                .find(|t| t.id == id)
                .map_or("", |t| t.url.as_str());
            SharedString::from(url.to_string())
        })
    }

    fn open_tab(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.tabs.open(url.clone());
        match ChildBrowser::create(self.gpui_view, &url, &mut self.client) {
            Some(page) => {
                if self.first_browser.get() < 0 {
                    self.first_browser.set(page.id());
                }
                self.tab_of_browser.insert(page.id(), id);
                self.pages.insert(id, page);
            }
            None => eprintln!("gpui-a: CEF could not create a browser for {url}"),
        }
        self.show_active(window, cx);
    }

    pub fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let n = (self.next_test_page - 1) % 10 + 1;
        self.next_test_page += 1;
        self.open_tab(test_page_url(n), window, cx);
    }

    pub fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.tabs.active() else { return };
        self.tabs.close(id);
        self.titles.remove(&id);
        if let Some(page) = self.pages.remove(&id) {
            self.tab_of_browser.remove(&page.id());
            page.close();
            self.closing.insert(page.id(), page);
        }
        self.show_active(window, cx);
    }

    /// The one path every tab switch takes: a sidebar click, the command bar
    /// and the switch bench.
    pub fn activate_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.active() == Some(id) || !self.tabs.list().iter().any(|t| t.id == id) {
            return;
        }
        mark("tab_switch_start");
        self.tabs.activate(id);
        self.show_active(window, cx);
        // Done is the page's own second animation frame after it became
        // visible: the first runs as its first frame begins, the second once
        // that frame is out. Same as web-ui. The page reports the time
        // through the console, so the IPC delay doesn't count.
        if let Some(page) = self.pages.get(&id) {
            page.run_js(TAB_SHOWN_JS);
        }
    }

    /// Shows only the active tab's page and puts its URL in the field.
    fn show_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.tabs.active();
        if let Some(page) = active.and_then(|id| self.pages.get(&id))
            && let Some(bounds) = self.page_bounds.get()
        {
            set_page_frame(page, bounds);
        }
        for (id, page) in &self.pages {
            page.set_visible(Some(*id) == active);
        }
        self.sync_url_field(window, cx);
        cx.notify();
    }

    fn sync_url_field(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let url = self
            .tabs
            .active()
            .and_then(|id| self.tabs.list().iter().find(|t| t.id == id))
            .map(|t| t.url.clone())
            .unwrap_or_default();
        self.url_input
            .update(cx, |input, cx| input.set_text(url, cx));
    }

    pub fn navigate(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = normalize_url(text) else {
            return;
        };
        match self.tabs.active() {
            Some(id) => {
                self.tabs.set_url(id, url.clone());
                if let Some(page) = self.pages.get(&id) {
                    page.load_url(&url);
                    page.set_visible(true);
                }
                self.sync_url_field(window, cx);
            }
            None => self.open_tab(url, window, cx),
        }
        cx.notify();
    }

    pub fn toggle_command_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar.is_some() {
            self.close_command_bar(cx);
            return;
        }
        let Some(page) = self.page_bounds.get() else {
            return;
        };
        let handle: BrowserHandle = Window::window_handle(window).downcast().unwrap();
        let active = self.tabs.active();
        let tabs = self
            .tabs
            .list()
            .iter()
            .map(|t| (t.id, self.tab_label(t.id), Some(t.id) == active))
            .collect();
        let bar = CommandBar::open(handle, self.gpui_view, window.bounds(), page, tabs, cx);
        self.command_bar = bar;
        cx.notify();
    }

    pub fn close_command_bar(&mut self, cx: &mut Context<Self>) {
        let Some(bar) = self.command_bar.take() else {
            return;
        };
        let gpui_view = self.gpui_view;
        // Removing the window from inside our own update would re-enter it;
        // defer to a clean stack.
        cx.defer(move |cx| {
            let _ = bar.update(cx, |bar, window, _| {
                unsafe { overlay::detach(gpui_view, bar.gpui_view()) };
                window.remove_window();
            });
        });
        cx.notify();
    }

    /// Puts an open command bar back over the page, after the main window
    /// moved.
    fn place_command_bar(&self, cx: &App) {
        let Some(page) = self.page_bounds.get() else {
            return;
        };
        let Some(bar_view) = self
            .command_bar
            .and_then(|bar| bar.read(cx).ok().map(|bar| bar.gpui_view()))
        else {
            return;
        };
        unsafe { overlay::place(self.gpui_view, bar_view, page) };
    }

    pub fn command_bar_is_open(&self) -> bool {
        self.command_bar.is_some()
    }

    /// What's typed in the command bar, if it's open.
    pub fn command_bar_query(&self, cx: &App) -> Option<String> {
        let bar = self.command_bar?;
        bar.read(cx).ok().map(|bar| bar.query(cx))
    }

    pub fn quit(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        self.close_command_bar(cx);
        for (_, page) in self.pages.drain() {
            page.close();
            self.closing.insert(page.id(), page);
        }
        if self.closing.is_empty() {
            finish_quit(cx);
            return;
        }
        // Don't hang on a browser that never reports BeforeClose.
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(3))
                .await;
            eprintln!("gpui-a: browsers didn't close in 3 s, exiting anyway");
            std::process::exit(0);
        })
        .detach();
    }

    fn start_frame_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.bench.scroll && !self.bench.resize {
            return;
        }
        let started = Instant::now();
        let height = window.viewport_size().height;
        self.bench_tick(started, height, window, cx);
    }

    /// One frame of the scroll or resize run: move things, then ask for the
    /// next frame. `render` writes the `frame` marks.
    fn bench_tick(
        &mut self,
        started: Instant,
        height: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let elapsed = started.elapsed();
        if elapsed >= bench::RUN_TIME {
            self.bench_running = false;
            self.quit(cx);
            return;
        }
        // Frame marks cover the run only, like web-ui's.
        self.bench_running = true;
        if self.bench.scroll {
            let handle = self.scroll.0.borrow().base_handle.clone();
            let offset = handle.offset();
            let max = handle.max_offset().height;
            let mut y = offset.y - px(bench::SCROLL_PX_PER_FRAME);
            if -y > max {
                y = px(0.);
            }
            handle.set_offset(point(offset.x, y));
        }
        if self.bench.resize {
            let width = bench::resize_width(elapsed.as_secs_f64());
            window.resize(size(px(width), height));
        }
        cx.notify();
        let this = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = this.update(cx, |this, cx| this.bench_tick(started, height, window, cx));
        });
    }

    fn start_switch_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.bench.switch {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(bench::SWITCH_DELAY).await;
            for step in 0..bench::SWITCH_COUNT {
                let switched = this.update_in(cx, |this, window, cx| {
                    let ids: Vec<TabId> = this.tabs.list().iter().map(|t| t.id).collect();
                    if ids.is_empty() {
                        return;
                    }
                    let id = ids[bench::switch_target(step, ids.len())];
                    this.activate_tab(id, window, cx);
                });
                if switched.is_err() {
                    return;
                }
                cx.background_executor().timer(bench::SWITCH_INTERVAL).await;
            }
            let _ = this.update(cx, |this, cx| this.quit(cx));
        })
        .detach();
    }

    pub fn gpui_view(&self) -> *mut c_void {
        self.gpui_view
    }

    pub fn scroll_rows_to(&self, index: usize) {
        self.scroll
            .scroll_to_item_strict(index, gpui::ScrollStrategy::Top);
    }

    /// How far the rows are scrolled, in points (positive is down).
    pub fn sidebar_scroll_offset(&self) -> f32 {
        -f32::from(self.scroll.0.borrow().base_handle.offset().y)
    }

    pub fn url_field(&self) -> &Entity<TextInput> {
        &self.url_input
    }

    fn render_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tabs.active();
        let tabs = self.tabs.list().iter().map(|tab| {
            let id = tab.id;
            let is_active = Some(id) == active;
            div()
                .id(("tab", id as usize))
                .h(px(32.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .text_sm()
                .text_color(rgb(TEXT))
                .when(is_active, |d| d.bg(rgb(0xfbfaf8)).shadow_sm())
                .when(!is_active, |d| d.hover(|d| d.bg(rgba(0x0000000d))))
                .child(div().size(px(14.)).rounded_sm().bg(rgb(0xc9c3ba)))
                .child(div().flex_1().truncate().child(self.tab_label(id)))
                .on_click(cx.listener(move |this, _, window, cx| this.activate_tab(id, window, cx)))
        });

        let rows = self.rows.clone();
        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .px(px(10.))
            .child(title_strip(window))
            .child(
                div()
                    .h(px(34.))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_lg()
                    .bg(rgba(0x0000000f))
                    .text_sm()
                    .line_height(px(18.))
                    .overflow_hidden()
                    .child(self.url_input.clone()),
            )
            .child(
                div()
                    .pt_3()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(tabs),
            )
            .child(div().h(px(1.)).mx_1().my_1().bg(rgba(0x0000001a)))
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child("History"),
            )
            .child(
                uniform_list("rows", rows.len(), move |range, _window, _cx| {
                    range
                        .map(|ix| {
                            div()
                                .id(ix)
                                .h(px(ROW_HEIGHT))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded_md()
                                .text_xs()
                                .text_color(rgb(TEXT_MUTED))
                                .hover(|d| d.bg(rgba(0x0000000d)))
                                .child(rows[ix].clone())
                        })
                        .collect()
                })
                .track_scroll(self.scroll.clone())
                .flex_1()
                .pb_2(),
            )
    }
}

/// Title bar strip: traffic lights on the left, drag anywhere.
#[cfg(target_os = "macos")]
fn title_strip(_window: &Window) -> impl IntoElement {
    div()
        .h(px(44.))
        .w_full()
        .flex_none()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
            window.start_window_move()
        })
}

/// Title bar strip: drag anywhere, minimize, maximize and close on the right.
/// The window has no system title bar, so GPUI draws the buttons and tells
/// Windows where they are, which keeps snap layouts and double-click to
/// maximize working.
#[cfg(target_os = "windows")]
fn title_strip(window: &Window) -> impl IntoElement {
    use gpui::WindowControlArea;

    // Segoe Fluent Icons' caption glyphs, what Windows 11's own title bars use.
    let maximize = if window.is_maximized() {
        "\u{E923}"
    } else {
        "\u{E922}"
    };
    let button = |id: &'static str, glyph: &'static str, area: WindowControlArea| {
        div()
            .id(id)
            .w(px(34.))
            .h(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .window_control_area(area)
            .child(glyph)
    };
    div()
        .h(px(44.))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .window_control_area(WindowControlArea::Drag)
        .font_family("Segoe Fluent Icons")
        .text_size(px(10.))
        .text_color(rgb(TEXT))
        .child(
            button("minimize", "\u{E921}", WindowControlArea::Min)
                .hover(|d| d.bg(rgba(0x0000000f))),
        )
        .child(
            button("maximize", maximize, WindowControlArea::Max).hover(|d| d.bg(rgba(0x0000000f))),
        )
        .child(
            button("close", "\u{E8BB}", WindowControlArea::Close)
                .hover(|d| d.bg(rgb(0xc42b1c)).text_color(rgb(0xffffff))),
        )
}

fn set_page_frame(page: &ChildBrowser, bounds: Bounds<Pixels>) {
    page.set_frame(
        f64::from(f32::from(bounds.origin.x)),
        f64::from(f32::from(bounds.origin.y)),
        f64::from(f32::from(bounds.size.width)),
        f64::from(f32::from(bounds.size.height)),
    );
}

fn finish_quit(cx: &mut Context<BrowserWindow>) {
    // Out of the current update and any CEF callback.
    cx.defer(|_| {
        jaal_gpui_cef::shutdown();
        std::process::exit(0);
    });
}

impl Render for BrowserWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.bench_running && jaal_bakeoff_shared::bench::frames_enabled() {
            mark("frame");
        }
        let page_bounds = self.page_bounds.clone();
        let active_page = self
            .tabs
            .active()
            .and_then(|id| self.pages.get(&id))
            .cloned();
        let bar_view = self
            .command_bar
            .and_then(|bar| bar.read(cx).ok().map(|bar| bar.gpui_view()));
        let gpui_view = self.gpui_view;

        div()
            .id("browser")
            .track_focus(&self.focus_handle)
            // On the root, so they fire wherever focus is in this window,
            // the CEF page included (GPUI sees key equivalents first).
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.new_tab(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| this.close_active_tab(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleCommandBar, window, cx| {
                this.toggle_command_bar(window, cx)
            }))
            .on_action(cx.listener(|this, _: &Quit, _, cx| this.quit(cx)))
            // A click on GPUI's part of the window takes keyboard focus back
            // from the page. AppKit does this by itself; Windows doesn't.
            .capture_any_mouse_down(move |_, _, _| unsafe {
                jaal_gpui_cef::focus_gpui_view(gpui_view)
            })
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(SIDEBAR_BG))
            .font_family(".SystemUIFont")
            .child(self.render_sidebar(window, cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .py(px(PAGE_INSET))
                    .pr(px(PAGE_INSET))
                    .child(
                        div()
                            .size_full()
                            .rounded_md()
                            .bg(rgb(0xffffff))
                            .shadow_sm()
                            .child(
                                // Lays the CEF view and the command bar over
                                // this box in the same frame GPUI lays it out.
                                canvas(
                                    move |bounds, _window, _cx| {
                                        page_bounds.set(Some(bounds));
                                        if let Some(page) = &active_page {
                                            set_page_frame(page, bounds);
                                        }
                                        if let Some(bar_view) = bar_view {
                                            unsafe { overlay::place(gpui_view, bar_view, bounds) };
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .size_full(),
                            ),
                    ),
            )
    }
}

impl Focusable for BrowserWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
