//! The main window: GPUI draws the sidebar, the page texture of the active
//! tab and, over it, the command bar, all in one window (fix B). Each tab is
//! a windowless CEF browser (`page.rs`); only the active one renders.

use crate::command_bar::{
    BAR_HEIGHT, BAR_MARGIN, BAR_RADIUS, BAR_TOP, BAR_WIDTH, CommandBar, CommandBarEvent,
};
use crate::page::{Page, PageView};
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, Bounds, BoxShadow, Context, Corners, Entity, FocusHandle, Focusable, KeyBinding,
    KeyDownEvent, KeyUpEvent, MouseButton, Pixels, SharedString, Subscription,
    UniformListScrollHandle, Window, WindowHandle, actions, canvas, div, hsla, point, prelude::*,
    px, rgb, rgba, size, uniform_list,
};
use jaal_bakeoff_shared::bench::{mark, mark_at};
use jaal_bakeoff_shared::bench_modes::{self as bench, BenchConfig};
use jaal_bakeoff_shared::{TabId, Tabs, fake_rows, normalize_url, test_page_url};
use jaal_gpui_cef::BrowserEvent;
use jaal_gpui_cef::text_input::{TextInput, TextInputEvent};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

pub const SIDEBAR_WIDTH: f32 = 264.;
const PAGE_INSET: f32 = 8.;
const PAGE_RADIUS: f32 = 6.;
const ROW_HEIGHT: f32 = 28.;
const FAKE_ROWS: usize = 5000;
/// How far the page behind the command bar is blurred, in points.
const BAR_BLUR_RADIUS: f32 = 24.;

pub const SIDEBAR_BG: u32 = 0xe9e6e1;
const TEXT: u32 = 0x24211d;
const TEXT_MUTED: u32 = 0x77716a;

actions!(gpui_b, [NewTab, CloseTab, ToggleCommandBar, Quit]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-k", ToggleCommandBar, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
}

const TAB_SHOWN_PREFIX: &str = "jaal:tab_shown:";
const TAB_SHOWN_JS: &str = "requestAnimationFrame(() => requestAnimationFrame(() => \
     console.log('jaal:tab_shown:' + (performance.timeOrigin + performance.now()))))";
/// Console lines the demo's pages write, printed on stderr.
const DEMO_PREFIX: &str = "jaal:demo:";

/// What the CEF callbacks queue for the main window.
pub enum PageEvent {
    Browser(BrowserEvent),
    FirstPaint,
    TabsLoaded,
    /// A page painted a new frame.
    Frame,
}

pub type BrowserHandle = WindowHandle<BrowserWindow>;

pub struct BrowserWindow {
    tabs: Tabs,
    pages: HashMap<TabId, Page>,
    titles: HashMap<TabId, SharedString>,
    tab_of_browser: HashMap<i32, TabId>,
    /// Browsers asked to close that haven't sent `BeforeClose` yet.
    closing: HashMap<i32, Page>,
    handler: jaal_gpui_cef::EventHandler,
    events: mpsc::UnboundedSender<PageEvent>,
    /// A `Frame` event is queued and not yet handled; more paints before it
    /// is handled need no second one.
    redraw_pending: Rc<Cell<bool>>,
    /// The first tab's browser, whose main-frame load is `first_paint`.
    first_browser: Rc<Cell<i32>>,
    gpui_view: *mut c_void,
    rows: Rc<[SharedString]>,
    scroll: UniformListScrollHandle,
    url_input: Entity<TextInput>,
    /// Keyboard focus for the page: while it has it, keys go to CEF.
    page_focus: FocusHandle,
    /// Where the active page's texture was drawn in the last frame.
    page_drawn_at: Rc<Cell<Option<Bounds<Pixels>>>>,
    next_test_page: usize,
    command_bar: Option<Entity<CommandBar>>,
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
        let gpui_view = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::AppKit(handle)) => handle.ns_view.as_ptr(),
            _ => panic!("gpui-b runs on macOS only"),
        };
        eprintln!(
            "gpui-b: pid {} main window CGWindowID {}",
            std::process::id(),
            unsafe { jaal_gpui_cef::window_number(gpui_view) }
        );

        let (events, mut event_rx) = mpsc::unbounded::<PageEvent>();
        let first_browser = Rc::new(Cell::new(-1));
        let handler = Self::event_handler(events.clone(), first_browser.clone(), bench.tabs);
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
        let page_focus = cx.focus_handle();
        let subscriptions = vec![
            cx.subscribe_in(
                &url_input,
                window,
                |this, _input, event: &TextInputEvent, window, cx| match event {
                    TextInputEvent::Submit(text) => this.navigate(text, window, cx),
                    TextInputEvent::Cancel => this.focus_page(window, cx),
                },
            ),
            cx.on_focus(&page_focus, window, |this, _, _| {
                if let Some(page) = this.active_page() {
                    page.set_focus(true);
                }
            }),
            cx.on_blur(&page_focus, window, |this, _, _| {
                if let Some(page) = this.active_page() {
                    page.set_focus(false);
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
            handler,
            events,
            redraw_pending: Rc::new(Cell::new(false)),
            first_browser,
            gpui_view,
            rows,
            scroll: UniformListScrollHandle::new(),
            url_input,
            page_focus,
            page_drawn_at: Rc::new(Cell::new(None)),
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
        this.focus_page(window, cx);
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
                    if let Some(line) = message.strip_prefix(DEMO_PREFIX) {
                        eprintln!("DEMO page says {line}");
                        return true;
                    }
                }
                // A windowless browser has no view to set a cursor on; the
                // page view shows it.
                BrowserEvent::CursorChange { .. } => handled = true,
                _ => {}
            }
            let _ = events.unbounded_send(PageEvent::Browser(event));
            handled
        })
    }

    fn on_page_event(&mut self, event: PageEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            PageEvent::Frame => {
                self.redraw_pending.set(false);
                cx.notify();
            }
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
                        self.sync_url_field(cx);
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
            PageEvent::Browser(BrowserEvent::CursorChange { browser_id, cursor }) => {
                if let Some(page) = self
                    .tab_of_browser
                    .get(&browser_id)
                    .and_then(|tab| self.pages.get(tab))
                {
                    page.set_cursor(cursor);
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
            PageEvent::Browser(BrowserEvent::LoadEnd { .. } | BrowserEvent::Console { .. }) => {}
        }
    }

    pub fn tabs(&self) -> &Tabs {
        &self.tabs
    }

    fn active_page(&self) -> Option<&Page> {
        self.tabs.active().and_then(|id| self.pages.get(&id))
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
        // The page's real size arrives with the first layout; until then
        // start at what the layout will give, so the first frame is right.
        let viewport = window.viewport_size();
        let view_size = size(
            viewport.width - px(SIDEBAR_WIDTH + PAGE_INSET),
            viewport.height - px(2. * PAGE_INSET),
        );
        let events = self.events.clone();
        let pending = self.redraw_pending.clone();
        let on_frame = Box::new(move || {
            if !pending.replace(true) {
                let _ = events.unbounded_send(PageEvent::Frame);
            }
        });
        match Page::create(
            self.gpui_view,
            &url,
            view_size,
            window.scale_factor(),
            self.handler.clone(),
            on_frame,
        ) {
            Some(page) => {
                if self.first_browser.get() < 0 {
                    self.first_browser.set(page.id());
                }
                self.tab_of_browser.insert(page.id(), id);
                self.pages.insert(id, page);
            }
            None => eprintln!("gpui-b: CEF could not create a browser for {url}"),
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
        // visible, as in gpui-a and web-ui. The page reports the time
        // through the console, so the IPC delay doesn't count.
        if let Some(page) = self.pages.get(&id) {
            page.run_js(TAB_SHOWN_JS);
        }
    }

    /// Only the active tab renders: the others are told they're hidden.
    fn show_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.tabs.active();
        let page_focused = self.page_focus.is_focused(window);
        for (id, page) in &self.pages {
            let is_active = Some(*id) == active;
            page.set_hidden(!is_active);
            page.set_focus(is_active && page_focused);
        }
        self.sync_url_field(cx);
        cx.notify();
    }

    fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.page_focus);
        cx.notify();
    }

    fn sync_url_field(&mut self, cx: &mut Context<Self>) {
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
                }
                self.sync_url_field(cx);
                self.focus_page(window, cx);
            }
            None => self.open_tab(url, window, cx),
        }
        cx.notify();
    }

    /// A key while the page has focus. App shortcuts never get here: GPUI
    /// matches its key bindings first.
    fn page_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(page) = self.active_page() else {
            return;
        };
        let keystroke = &event.keystroke;
        if keystroke.modifiers.platform
            && page.edit_command(&keystroke.key, keystroke.modifiers.shift)
        {
            cx.stop_propagation();
            return;
        }
        if page.forward_current_key_event() {
            cx.stop_propagation();
        }
    }

    fn page_key_up(&mut self, _event: &KeyUpEvent, cx: &mut Context<Self>) {
        if let Some(page) = self.active_page()
            && page.forward_current_key_event()
        {
            cx.stop_propagation();
        }
    }

    pub fn toggle_command_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar.is_some() {
            self.close_command_bar(window, cx);
            return;
        }
        let active = self.tabs.active();
        let tabs = self
            .tabs
            .list()
            .iter()
            .map(|t| (t.id, self.tab_label(t.id), Some(t.id) == active))
            .collect();
        let bar = cx.new(|cx| CommandBar::new(tabs, window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &bar,
            window,
            |this, _, event: &CommandBarEvent, window, cx| match event {
                CommandBarEvent::Navigate(text) => {
                    let text = text.clone();
                    this.close_command_bar(window, cx);
                    this.navigate(&text, window, cx);
                }
                CommandBarEvent::ActivateTab(id) => {
                    let id = *id;
                    this.close_command_bar(window, cx);
                    this.activate_tab(id, window, cx);
                }
                CommandBarEvent::Close => this.close_command_bar(window, cx),
            },
        ));
        self.command_bar = Some(bar);
        cx.notify();
    }

    pub fn close_command_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar.take().is_some() {
            self.focus_page(window, cx);
        }
    }

    pub fn command_bar_is_open(&self) -> bool {
        self.command_bar.is_some()
    }

    /// What's typed in the command bar, if it's open.
    pub fn command_bar_query(&self, cx: &App) -> Option<String> {
        Some(self.command_bar.as_ref()?.read(cx).query(cx))
    }

    pub fn quit(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        self.command_bar = None;
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
            eprintln!("gpui-b: browsers didn't close in 3 s, exiting anyway");
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

    /// How far the rows are scrolled, in points (positive is down).
    pub fn sidebar_scroll_offset(&self) -> f32 {
        -f32::from(self.scroll.0.borrow().base_handle.offset().y)
    }

    pub fn url_field(&self) -> &Entity<TextInput> {
        &self.url_input
    }

    /// Runs `code` in the active page (the demo reads results back through
    /// `console.log('jaal:demo:...')`).
    pub fn run_js_in_active_page(&self, code: &str) {
        if let Some(page) = self.active_page() {
            page.run_js(code);
        }
    }

    pub fn active_page_paints(&self) -> u64 {
        self.active_page().map_or(0, Page::paint_count)
    }

    /// Where the page area is, from the last frame (window points).
    pub fn page_drawn_at(&self) -> Option<Bounds<Pixels>> {
        self.page_drawn_at.get()
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(
                // Title bar strip: traffic lights on the left, drag anywhere.
                div()
                    .h(px(44.))
                    .w_full()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
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

    /// The command bar's box over the page: a scrim that closes the bar on a
    /// click outside it, the bar's shadow, a blurred copy of the page behind
    /// it, then the bar itself.
    fn render_command_bar(
        &self,
        bar: Entity<CommandBar>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let backdrop = self
            .active_page()
            .and_then(Page::frame)
            .map(|(buffer, _)| buffer);
        let page_drawn_at = self.page_drawn_at.clone();
        div()
            .id("bar-scrim")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_command_bar(window, cx)),
            )
            .flex()
            .flex_col()
            .items_center()
            .pt(px(BAR_TOP))
            .px(px(BAR_MARGIN))
            .pb(px(BAR_MARGIN))
            .child(
                div()
                    .id("bar")
                    .relative()
                    .w(px(BAR_WIDTH))
                    .max_w_full()
                    .h(px(BAR_HEIGHT))
                    .max_h_full()
                    .rounded(px(BAR_RADIUS))
                    .shadow(vec![
                        BoxShadow {
                            color: hsla(0., 0., 0., 0.22),
                            offset: point(px(0.), px(16.)),
                            blur_radius: px(48.),
                            spread_radius: px(0.),
                        },
                        BoxShadow {
                            color: hsla(0., 0., 0., 0.12),
                            offset: point(px(0.), px(2.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                        },
                    ])
                    // Clicks inside the bar stay in the bar.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                if let (Some(buffer), Some(drawn_at)) =
                                    (backdrop.clone(), page_drawn_at.get())
                                {
                                    window.paint_surface_clipped(
                                        drawn_at,
                                        buffer,
                                        bounds,
                                        Corners::all(px(BAR_RADIUS)),
                                        px(BAR_BLUR_RADIUS),
                                    );
                                }
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(div().absolute().top_0().left_0().size_full().child(bar)),
            )
    }
}

fn finish_quit(cx: &mut Context<BrowserWindow>) {
    // Out of the current update and any CEF callback.
    cx.defer(|_| {
        jaal_gpui_cef::shutdown();
        std::process::exit(0);
    });
}

impl Render for BrowserWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.bench_running && jaal_bakeoff_shared::bench::frames_enabled() {
            mark("frame");
        }
        let page = self.active_page().cloned();
        let bar = self.command_bar.clone();

        div()
            .id("browser")
            .track_focus(&self.focus_handle)
            // On the root, so they fire wherever focus is in this window.
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.new_tab(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| this.close_active_tab(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleCommandBar, window, cx| {
                this.toggle_command_bar(window, cx)
            }))
            .on_action(cx.listener(|this, _: &Quit, _, cx| this.quit(cx)))
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(SIDEBAR_BG))
            .font_family(".SystemUIFont")
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .py(px(PAGE_INSET))
                    .pr(px(PAGE_INSET))
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .rounded(px(PAGE_RADIUS))
                            .bg(rgb(0xffffff))
                            .shadow_sm()
                            .child(
                                div()
                                    .id("page")
                                    .size_full()
                                    .track_focus(&self.page_focus)
                                    .on_key_down(
                                        cx.listener(|this, event, _, cx| this.page_key(event, cx)),
                                    )
                                    .on_key_up(
                                        cx.listener(|this, event, _, cx| {
                                            this.page_key_up(event, cx)
                                        }),
                                    )
                                    .child(PageView::new(
                                        page,
                                        self.page_focus.clone(),
                                        px(PAGE_RADIUS),
                                        self.page_drawn_at.clone(),
                                    )),
                            )
                            .when_some(bar, |d, bar| d.child(self.render_command_bar(bar, cx))),
                    ),
            )
    }
}

impl Focusable for BrowserWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
