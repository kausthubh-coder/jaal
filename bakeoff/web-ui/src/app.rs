//! The browser process: one CEF Views window with a sidebar browser view, a
//! page browser view per tab, and the command bar as an overlay browser view.
//!
//! Every callback used here runs on CEF's UI thread, so the state lives in a
//! thread-local `RefCell`. CEF can call back into us synchronously from
//! inside a CEF call (adding a view lays out the window, for example), so no
//! code holds the borrow across a CEF call: `ui()` borrows, copies out the
//! handles it needs, and returns before we touch CEF.

use crate::bench::{self, Bench};
use cef::wrapper::message_router::{
    BrowserSideCallback, BrowserSideHandler, BrowserSideRouter, MessageRouterBrowserSide,
    MessageRouterBrowserSideHandlerCallbacks,
};
use cef::*;
use jaal_bakeoff_shared::{TabId, Tabs, fake_rows, test_page_url};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use web_ui::{Command, js_string, normalize_url, parse_command};

const SIDEBAR_WIDTH: i32 = 280;
const INITIAL_BOUNDS: Rect = Rect {
    x: 80,
    y: 60,
    width: 1200,
    height: 800,
};
/// The overlay is exactly the card: CEF paints a windowed browser's
/// background opaque, so a transparent margin for the shadow can't work.
/// The shadow and rounded corners come from macOS, see
/// `mac::round_overlay_with_shadow`.
const COMMAND_BAR_WIDTH: i32 = 600;
const COMMAND_BAR_HEIGHT: i32 = 292;
const COMMAND_BAR_TOP: i32 = 80;
const COMMAND_BAR_RADIUS: f64 = 14.0;
/// Opaque, matching `--card` in `ui/command.html`.
const COMMAND_BAR_BACKGROUND: u32 = 0xFF202126;
/// Opaque, matching `--bg` in `ui/sidebar.css`, so nothing flashes white
/// before the sidebar's first paint.
const SIDEBAR_BACKGROUND: u32 = 0xFF1C1D21;
const COMMAND_EVENT_FLAG: u32 = 128; // EVENTFLAG_COMMAND_DOWN

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Sidebar,
    CommandBar,
    Tab(TabId),
}

struct Page {
    view: BrowserView,
    title: String,
    loaded: bool,
}

#[derive(Default)]
struct Ui {
    window: Option<Window>,
    sidebar: Option<BrowserView>,
    content: Option<Panel>,
    content_bounds: Option<Rect>,
    command_bar: Option<OverlayController>,
    command_view: Option<BrowserView>,
    client: Option<Client>,
    tabs: Tabs,
    pages: HashMap<TabId, Page>,
    /// Browser identifier to what that browser shows.
    roles: HashMap<i32, Role>,
    first_tab: Option<TabId>,
    next_test_page: usize,
    sidebar_loaded: bool,
    first_paint_marked: bool,
    open_browsers: usize,
    quitting: bool,
    bench: Bench,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    UI.with(|cell| {
        let mut ui = cell
            .try_borrow_mut()
            .expect("UI state re-entered: a CEF call ran while ui() held the borrow");
        f(&mut ui)
    })
}

/// `$JAAL_PROFILE_DIR`, or `target/profiles/web-ui` in the workspace. The
/// bench script wipes `$JAAL_PROFILE_DIR` before each run for cold starts.
fn profile_dir() -> PathBuf {
    let dir = std::env::var_os("JAAL_PROFILE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/profiles/web-ui")
        });
    std::fs::create_dir_all(&dir).expect("cannot create the profile dir");
    dir.canonicalize().unwrap_or(dir)
}

fn ui_url(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ui")
        .join(file);
    let path = path.canonicalize().unwrap_or(path);
    format!("file://{}", path.display())
}

/// The first command-line argument that isn't a switch replaces test page 1.
fn first_tab_url() -> String {
    std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .and_then(|a| normalize_url(&a))
        .unwrap_or_else(|| test_page_url(1))
}

pub fn run(args: &args::Args) {
    let profile = profile_dir();
    let profile = profile.to_string_lossy();
    let settings = Settings {
        no_sandbox: 0,
        root_cache_path: CefString::from(profile.as_ref()),
        cache_path: CefString::from(profile.as_ref()),
        background_color: SIDEBAR_BACKGROUND,
        ..Default::default()
    };
    let mut app = BrowserApp::new();
    assert_eq!(
        initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ),
        1,
        "CEF failed to initialize"
    );
    run_message_loop();
    shutdown();
}

wrap_app! {
    struct BrowserApp;

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(command_line) = command_line else { return };
            // Chromium asks the login keychain for its cookie key at start,
            // which can raise a dialog nobody is here to click.
            command_line.append_switch(Some(&CefString::from("use-mock-keychain")));
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(UiBrowserProcessHandler::new())
        }
    }
}

wrap_browser_process_handler! {
    struct UiBrowserProcessHandler;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let router = BrowserSideRouter::new(web_ui::router_config());
            router.add_handler(Arc::new(QueryHandler), true);
            let client = UiClient::new(router);
            let bench = Bench::from_env();
            ui(|ui| {
                ui.client = Some(client);
                ui.next_test_page = bench.tabs.max(1) + 1;
                ui.bench = bench;
            });
            let mut delegate = UiWindowDelegate::new();
            window_create_top_level(Some(&mut delegate));
        }
    }
}

wrap_window_delegate! {
    struct UiWindowDelegate {}

    impl ViewDelegate {}

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window.cloned() else { return };
            build_window(&window);
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            // Drop our references so the views and their browsers can go.
            let _ = ui(|ui| {
                (
                    ui.window.take(),
                    ui.sidebar.take(),
                    ui.content.take(),
                    ui.command_bar.take(),
                    ui.command_view.take(),
                    std::mem::take(&mut ui.pages),
                )
            });
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            // Destroying the window destroys its browser views, which close
            // their browsers; the last on_before_close ends the message loop.
            // No page in the bake-off has an unload handler to wait for.
            ui(|ui| ui.quitting = true);
            1
        }

        fn initial_bounds(&self, _window: Option<&mut Window>) -> Rect {
            INITIAL_BOUNDS
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}

wrap_browser_view_delegate! {
    struct UiBrowserViewDelegate {
        role: Role,
        preferred_width: i32,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            // CEF ignores an empty size (either side 0) and falls back to
            // the view's own size. The box layout stretches the height.
            Size { width: self.preferred_width, height: 1 }
        }
    }

    impl BrowserViewDelegate {
        fn on_browser_created(
            &self,
            _browser_view: Option<&mut BrowserView>,
            browser: Option<&mut Browser>,
        ) {
            let Some(browser) = browser else { return };
            let id = browser.identifier();
            let role = self.role;
            ui(|ui| ui.roles.insert(id, role));
        }

        fn browser_runtime_style(&self) -> RuntimeStyle {
            // Alloy: a bare browser with no Chrome toolbar or Chrome
            // shortcuts, so Cmd+T and friends reach our keyboard handler.
            RuntimeStyle::ALLOY
        }
    }
}

wrap_panel_delegate! {
    struct ContentDelegate {}

    impl ViewDelegate {
        fn on_layout_changed(&self, _view: Option<&mut View>, new_bounds: Option<&Rect>) {
            let Some(bounds) = new_bounds.cloned() else { return };
            ui(|ui| ui.content_bounds = Some(bounds));
            place_command_bar();
        }
    }

    impl PanelDelegate {}
}

fn browser_view(role: Role, url: &str, preferred_width: i32, background: u32) -> BrowserView {
    let mut client = ui(|ui| ui.client.clone());
    let settings = BrowserSettings {
        background_color: background,
        ..Default::default()
    };
    let mut delegate = UiBrowserViewDelegate::new(role, preferred_width);
    browser_view_create(
        client.as_mut(),
        Some(&CefString::from(url)),
        Some(&settings),
        None,
        None,
        Some(&mut delegate),
    )
    .expect("browser_view_create failed")
}

fn build_window(window: &Window) {
    window.set_title(Some(&CefString::from("jaal web-ui")));
    let layout = window
        .set_to_box_layout(Some(&BoxLayoutSettings {
            horizontal: 1,
            cross_axis_alignment: AxisAlignment::STRETCH,
            ..Default::default()
        }))
        .expect("no box layout");

    let sidebar = browser_view(
        Role::Sidebar,
        &ui_url("sidebar.html"),
        SIDEBAR_WIDTH,
        SIDEBAR_BACKGROUND,
    );
    window.add_child_view(Some(&mut View::from(&sidebar)));

    let content = panel_create(Some(&mut ContentDelegate::new())).expect("no panel");
    content.set_to_fill_layout();
    let mut content_view = View::from(&content);
    window.add_child_view(Some(&mut content_view));
    layout.set_flex_for_view(Some(&mut content_view), 1);

    let command_view = browser_view(
        Role::CommandBar,
        &ui_url("command.html"),
        0,
        COMMAND_BAR_BACKGROUND,
    );
    let command_bar = window
        .add_overlay_view(Some(&mut View::from(&command_view)), DockingMode::CUSTOM, 1)
        .expect("no overlay");
    command_bar.set_visible(0);

    ui(|ui| {
        ui.window = Some(window.clone());
        ui.sidebar = Some(sidebar);
        ui.content = Some(content);
        ui.command_bar = Some(command_bar);
        ui.command_view = Some(command_view);
    });

    let tab_count = ui(|ui| ui.bench.tabs.max(1));
    let mut first = None;
    for n in 1..=tab_count {
        let url = if n == 1 {
            first_tab_url()
        } else {
            test_page_url(n)
        };
        let id = open_tab(&url);
        first.get_or_insert(id);
    }
    let first = first.expect("at least one tab");
    ui(|ui| {
        ui.first_tab = Some(first);
        ui.tabs.activate(first);
    });
    show_active_tab();

    window.show();
    if let Some(id) = crate::window_number(window.window_handle()) {
        eprintln!("jaal web-ui: CGWindowID {id}");
    }
}

fn open_tab(url: &str) -> TabId {
    let id = ui(|ui| ui.tabs.open(url));
    let view = browser_view(Role::Tab(id), url, 0, 0xFFFFFFFF);
    let content = ui(|ui| {
        ui.pages.insert(
            id,
            Page {
                view: view.clone(),
                title: String::new(),
                loaded: false,
            },
        );
        ui.content.clone()
    });
    // Hidden until show_active_tab decides, so a new tab never flashes over
    // the active one.
    view.set_visible(0);
    if let Some(content) = content {
        content.add_child_view(Some(&mut View::from(&view)));
    }
    id
}

/// Makes only the active tab's view visible and returns it.
fn show_active_tab() -> Option<BrowserView> {
    let (active, views) = ui(|ui| {
        let views: Vec<_> = ui
            .pages
            .iter()
            .map(|(id, p)| (*id, p.view.clone()))
            .collect();
        (ui.tabs.active(), views)
    });
    let mut shown = None;
    // Show the new one before hiding the old so no frame shows neither.
    for (id, view) in &views {
        if Some(*id) == active {
            view.set_visible(1);
            shown = Some(view.clone());
        }
    }
    for (id, view) in &views {
        if Some(*id) != active {
            view.set_visible(0);
        }
    }
    push_tabs_to_sidebar();
    shown
}

fn switch_to(tab: TabId, clicked_ms: f64) {
    let changed = ui(|ui| ui.tabs.active() != Some(tab) && ui.tabs.activate(tab));
    if !changed {
        return;
    }
    bench::mark_at("tab_switch_start", clicked_ms);
    let Some(view) = show_active_tab() else {
        return;
    };
    // `tab_switch_done` is the page's second requestAnimationFrame after it
    // became visible: the first runs as the view's first frame begins, the
    // second once that frame is out.
    run_js(
        &view,
        "requestAnimationFrame(() => requestAnimationFrame(() => window.cefQuery({ \
         request: 'tab_shown\\n' + (performance.timeOrigin + performance.now()), \
         onSuccess() {}, onFailure() {} })));",
    );
}

fn close_active_tab() {
    let (closed, last, content) = ui(|ui| {
        let Some(id) = ui.tabs.active() else {
            return (None, true, None);
        };
        ui.tabs.close(id);
        (
            ui.pages.remove(&id),
            ui.tabs.list().is_empty(),
            ui.content.clone(),
        )
    });
    if let Some(page) = closed {
        if let Some(host) = page.view.browser().and_then(|b| b.host()) {
            host.close_browser(1);
        }
        if let Some(content) = content {
            content.remove_child_view(Some(&mut View::from(&page.view)));
        }
    }
    // Closing the last tab closes the window, as in Chrome.
    if last {
        quit();
    } else {
        show_active_tab();
    }
}

fn new_tab() {
    let n = ui(|ui| {
        let n = ui.next_test_page;
        ui.next_test_page = n % 10 + 1;
        n
    });
    open_tab(&test_page_url(n));
    show_active_tab();
}

fn navigate_active(url: &str) {
    let view = ui(|ui| {
        ui.tabs
            .active()
            .and_then(|id| ui.pages.get(&id))
            .map(|p| p.view.clone())
    });
    if let Some(frame) = view.and_then(|v| v.browser()).and_then(|b| b.main_frame()) {
        frame.load_url(Some(&CefString::from(url)));
    }
}

fn place_command_bar() {
    let (bar, content) = ui(|ui| (ui.command_bar.clone(), ui.content_bounds.clone()));
    let (Some(bar), Some(content)) = (bar, content) else {
        return;
    };
    bar.set_bounds(Some(&Rect {
        x: content.x + (content.width - COMMAND_BAR_WIDTH) / 2,
        y: content.y + COMMAND_BAR_TOP,
        width: COMMAND_BAR_WIDTH.min(content.width),
        height: COMMAND_BAR_HEIGHT.min(content.height),
    }));
}

pub fn show_command_bar(show: bool) {
    let (bar, view, page, window) = ui(|ui| {
        let page = ui
            .tabs
            .active()
            .and_then(|id| ui.pages.get(&id))
            .map(|p| p.view.clone());
        (
            ui.command_bar.clone(),
            ui.command_view.clone(),
            page,
            ui.window.clone(),
        )
    });
    let (Some(bar), Some(view)) = (bar, view) else {
        return;
    };
    if show {
        place_command_bar();
        bar.set_visible(1);
        #[cfg(target_os = "macos")]
        if let Some(host) = view.browser().and_then(|b| b.host()) {
            crate::mac::round_overlay_with_shadow(host.window_handle(), COMMAND_BAR_RADIUS);
            crate::mac::make_key_window(host.window_handle());
        }
        view.request_focus();
        run_js(&view, "jaal.open();");
    } else {
        bar.set_visible(0);
        #[cfg(target_os = "macos")]
        if let Some(window) = window {
            crate::mac::make_key_window(window.window_handle());
        }
        if let Some(page) = page {
            page.request_focus();
        }
    }
}

fn command_bar_visible() -> bool {
    ui(|ui| ui.command_bar.clone()).is_some_and(|bar| bar.is_visible() != 0)
}

fn run_js(view: &BrowserView, code: &str) {
    if let Some(frame) = view.browser().and_then(|b| b.main_frame()) {
        frame.execute_java_script(Some(&CefString::from(code)), None, 0);
    }
}

pub fn run_in_sidebar(code: &str) {
    if let Some(sidebar) = ui(|ui| ui.sidebar.clone()) {
        run_js(&sidebar, code);
    }
}

pub fn run_in_active_page(code: &str) {
    let view = ui(|ui| {
        ui.tabs
            .active()
            .and_then(|id| ui.pages.get(&id))
            .map(|p| p.view.clone())
    });
    if let Some(view) = view {
        run_js(&view, code);
    }
}

fn push_tabs_to_sidebar() {
    let code = ui(|ui| {
        if !ui.sidebar_loaded {
            return None;
        }
        let tabs: Vec<String> = ui
            .tabs
            .list()
            .iter()
            .map(|t| {
                let title = ui.pages.get(&t.id).map_or("", |p| p.title.as_str());
                format!(
                    "{{id:{},title:{},url:{}}}",
                    t.id,
                    js_string(title),
                    js_string(&t.url)
                )
            })
            .collect();
        let active = ui
            .tabs
            .active()
            .map_or("null".to_string(), |a| a.to_string());
        Some(format!("jaal.setTabs([{}], {active});", tabs.join(",")))
    });
    if let Some(code) = code {
        run_in_sidebar(&code);
    }
}

fn push_rows_to_sidebar() {
    let rows: Vec<String> = fake_rows(5000)
        .iter()
        .map(|r| format!("[{},{}]", js_string(&r.title), js_string(&r.url)))
        .collect();
    run_in_sidebar(&format!("jaal.setRows([{}]);", rows.join(",")));
}

/// Closes the window. See `can_close`.
pub fn quit() {
    let window = ui(|ui| {
        ui.quitting = true;
        ui.window.clone()
    });
    match window {
        Some(window) => window.close(),
        None => quit_message_loop(),
    }
}

pub fn resize_window(width: i32) {
    if let Some(window) = ui(|ui| ui.window.clone()) {
        let bounds = window.bounds();
        window.set_bounds(Some(&Rect { width, ..bounds }));
    }
}

/// Presses a key like "cmd+k" or "enter" in our window. See `JAAL_PRESS_KEYS`.
pub fn press_key(key: &str) {
    let Some(window) = ui(|ui| ui.window.clone()) else {
        return;
    };
    // The command bar's window takes the keys while it's open.
    let command_bar = ui(|ui| ui.command_view.clone())
        .filter(|_| command_bar_visible())
        .and_then(|v| v.browser())
        .and_then(|b| b.host());
    let target = command_bar.map_or(window.window_handle(), |h| h.window_handle());
    #[cfg(target_os = "macos")]
    if !crate::mac::press_key(target, key) {
        eprintln!("jaal web-ui: can't press {key:?}");
    }
}

/// Tab ids in sidebar order, for the tab switch bench.
pub fn tab_ids() -> Vec<TabId> {
    ui(|ui| ui.tabs.list().iter().map(|t| t.id).collect())
}

fn on_main_frame_loaded(role: Role) {
    match role {
        Role::Sidebar => {
            ui(|ui| ui.sidebar_loaded = true);
            push_rows_to_sidebar();
            push_tabs_to_sidebar();
        }
        Role::Tab(id) => {
            let first_paint = ui(|ui| {
                if let Some(page) = ui.pages.get_mut(&id) {
                    page.loaded = true;
                }
                let first = !ui.first_paint_marked && ui.first_tab == Some(id);
                ui.first_paint_marked |= first;
                first
            });
            if first_paint {
                bench::mark("first_paint");
            }
            let all_loaded = ui(|ui| {
                ui.bench.tabs > 0
                    && ui.pages.len() == ui.bench.tabs
                    && ui.pages.values().all(|p| p.loaded)
            });
            if all_loaded {
                bench::on_tabs_loaded();
            }
        }
        Role::CommandBar => {}
    }
    let ready = ui(|ui| ui.sidebar_loaded && ui.first_paint_marked);
    if ready {
        bench::on_ready();
    }
}

pub fn with_bench<R>(f: impl FnOnce(&mut Bench) -> R) -> R {
    ui(|ui| f(&mut ui.bench))
}

fn handle_command(command: Command) {
    match command {
        Command::Activate { tab, clicked_ms } => switch_to(tab, clicked_ms),
        Command::Navigate(typed) => {
            if let Some(url) = normalize_url(&typed) {
                navigate_active(&url);
            }
        }
        Command::HideCommandBar => show_command_bar(false),
        Command::CommandBarSubmit(typed) => {
            show_command_bar(false);
            if let Some(url) = normalize_url(&typed) {
                navigate_active(&url);
            }
        }
        Command::TabShown { ms } => {
            bench::mark_at("tab_switch_done", ms);
            bench::on_tab_shown();
        }
        Command::Frames { run, ms } => bench::on_frames(run, &ms),
    }
}

/// Cmd+T, Cmd+W, Cmd+K, and the edit shortcuts a menu bar would normally
/// give text fields. Returns true when the key was ours.
fn handle_key(browser: Option<&mut Browser>, event: &KeyEvent) -> bool {
    if event.type_ != KeyEventType::RAWKEYDOWN || event.modifiers & COMMAND_EVENT_FLAG == 0 {
        return false;
    }
    let frame = browser.and_then(|b| b.focused_frame());
    // With no main menu, nothing else routes the edit keys to the field.
    let edit = |f: fn(&Frame)| {
        if let Some(frame) = &frame {
            f(frame);
        }
    };
    match u8::try_from(event.windows_key_code).unwrap_or(0) {
        b'T' => new_tab(),
        b'W' => close_active_tab(),
        b'K' => show_command_bar(!command_bar_visible()),
        b'C' => edit(|f| f.copy()),
        b'V' => edit(|f| f.paste()),
        b'X' => edit(|f| f.cut()),
        b'A' => edit(|f| f.select_all()),
        b'Z' => edit(|f| f.undo()),
        _ => return false,
    }
    true
}

fn role_of(browser: Option<&mut Browser>) -> Option<Role> {
    let id = browser?.identifier();
    ui(|ui| ui.roles.get(&id).copied())
}

struct QueryHandler;

impl BrowserSideHandler for QueryHandler {
    fn on_query_str(
        &self,
        _browser: Option<Browser>,
        _frame: Option<Frame>,
        _query_id: i64,
        request: &str,
        _persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        let Some(command) = parse_command(request) else {
            eprintln!("jaal web-ui: unknown message {request:?}");
            return false;
        };
        if let Ok(callback) = callback.lock() {
            callback.success_str("");
        }
        // Run it as a fresh task: the router holds its own locks while it
        // calls us, and closing a browser re-enters the router.
        let mut task = RunCommand::new(command);
        post_task(ThreadId::UI, Some(&mut task));
        true
    }
}

wrap_task! {
    struct RunCommand {
        command: Command,
    }

    impl Task {
        fn execute(&self) {
            handle_command(self.command.clone());
        }
    }
}

wrap_client! {
    struct UiClient {
        router: Arc<BrowserSideRouter>,
    }

    impl Client {
        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(UiKeyboardHandler::new())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(UiLoadHandler::new())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(UiDisplayHandler::new())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(UiLifeSpanHandler::new(self.router.clone()))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(UiRequestHandler::new(self.router.clone()))
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
                source_process,
                message.cloned(),
            ) as i32
        }
    }
}

wrap_keyboard_handler! {
    struct UiKeyboardHandler;

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: *mut u8,
            _is_keyboard_shortcut: Option<&mut i32>,
        ) -> i32 {
            event.is_some_and(|e| handle_key(browser, e)) as i32
        }
    }
}

wrap_load_handler! {
    struct UiLoadHandler;

    impl LoadHandler {
        fn on_load_end(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: i32,
        ) {
            if frame.is_none_or(|f| f.is_main() == 0) {
                return;
            }
            if let Some(role) = role_of(browser) {
                on_main_frame_loaded(role);
            }
        }
    }
}

wrap_display_handler! {
    struct UiDisplayHandler;

    impl DisplayHandler {
        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let Some(Role::Tab(id)) = role_of(browser) else { return };
            let title = title.map(CefString::to_string).unwrap_or_default();
            ui(|ui| {
                if let Some(page) = ui.pages.get_mut(&id) {
                    page.title = title;
                }
            });
            push_tabs_to_sidebar();
        }

        fn on_address_change(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            if frame.is_none_or(|f| f.is_main() == 0) {
                return;
            }
            let Some(Role::Tab(id)) = role_of(browser) else { return };
            let url = url.map(CefString::to_string).unwrap_or_default();
            ui(|ui| ui.tabs.set_url(id, url));
            push_tabs_to_sidebar();
        }
    }
}

wrap_life_span_handler! {
    struct UiLifeSpanHandler {
        router: Arc<BrowserSideRouter>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, _browser: Option<&mut Browser>) {
            ui(|ui| ui.open_browsers += 1);
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            self.router.on_before_close(browser.cloned());
            let none_left = ui(|ui| {
                ui.open_browsers -= 1;
                ui.open_browsers == 0 && ui.quitting
            });
            if none_left {
                quit_message_loop();
            }
        }
    }
}

wrap_request_handler! {
    struct UiRequestHandler {
        router: Arc<BrowserSideRouter>,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            self.router.on_before_browse(browser.cloned(), frame.cloned());
            0
        }

        fn on_render_process_terminated(
            &self,
            browser: Option<&mut Browser>,
            _status: TerminationStatus,
            _error_code: i32,
            _error_string: Option<&CefString>,
        ) {
            self.router.on_render_process_terminated(browser.cloned());
        }
    }
}
