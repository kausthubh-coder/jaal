//! M2's layout test. One engine window with:
//! - a GPUI window turned into a WS_CHILD of the engine's HWND, drawing the
//!   5,000 fake rows on the left, and
//! - a blurred GPUI pop-up owned by the engine's HWND, over the page.

use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*,
    px, rgb, rgba, size, uniform_list,
};
use jaal_engine::{Engine, Event, Rect, sys};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::io::Write;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWL_STYLE, GWLP_HWNDPARENT, GetClientRect, GetWindowLongPtrW, HWND_TOP,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetParent, SetWindowLongPtrW, SetWindowPos,
    WS_CHILD, WS_CLIPSIBLINGS, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_VISIBLE,
};

const SIDEBAR_WIDTH: f32 = 260.;

fn log(line: &str) {
    if let Some(path) = std::env::var_os("JAAL_UI_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

fn hwnd_of(window: &Window) -> HWND {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as HWND,
        other => panic!("unexpected GPUI window handle: {other:?}"),
    }
}

struct Sidebar {
    rows: Vec<gpui::SharedString>,
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xe9e4dc))
            .text_color(rgb(0x1f1c18))
            .child(
                div()
                    .h(px(40.))
                    .px_3()
                    .flex()
                    .items_center()
                    .text_sm()
                    .child("GPUI in a child HWND"),
            )
            .child(
                uniform_list("rows", rows.len(), move |range, _window, _cx| {
                    range
                        .map(|ix| {
                            div()
                                .id(ix)
                                .h(px(26.))
                                .px_3()
                                .flex()
                                .items_center()
                                .text_xs()
                                .hover(|d| d.bg(rgba(0x0000001a)))
                                .child(rows[ix].clone())
                        })
                        .collect()
                })
                .flex_1(),
            )
    }
}

struct Overlay;

impl Render for Overlay {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            // See-through, so the screenshot shows whether the page behind
            // is blurred.
            .bg(rgba(0xffffff66))
            .text_color(rgb(0x1f1c18))
            .child("GPUI pop-up owned by the engine window, with system blur")
    }
}

/// # Safety
/// `get_api` is the engine's, on its UI thread.
pub unsafe fn start(get_api: sys::jaal_engine_get_api_fn) {
    // SAFETY: per this function's contract.
    let engine = match unsafe { Engine::from_get_api(get_api) } {
        Ok(engine) => engine,
        Err(error) => return log(&format!("no engine: {error}")),
    };
    let url =
        std::env::var("JAAL_UI_URL").unwrap_or_else(|_| jaal_bakeoff_shared::test_page_url(1));
    let window = engine.create_window().expect("create a window");
    engine.open_tab(window, &url, true).expect("open a tab");
    let parent = engine.native_handle(window).expect("the window's HWND") as HWND;
    log(&format!("engine window {parent:?}"));
    engine
        .on_event(move |event| {
            log(&format!("{event:?}"));
            if event == Event::WindowClosed(window) {
                let _ = engine.quit();
            }
        })
        .expect("set the event handler");

    Application::new().run_embedded(move |cx: &mut App| {
        let (scale, client) = unsafe {
            let mut client = RECT::default();
            GetClientRect(parent, &mut client);
            (GetDpiForWindow(parent) as f32 / 96., client)
        };
        let client_width = client.right as f32 / scale;
        let client_height = client.bottom as f32 / scale;

        let sidebar = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(SIDEBAR_WIDTH), px(client_height)),
                    ))),
                    // A drawn title bar would make GPUI treat the top edge as
                    // a caption; this window is a plain child.
                    titlebar: Some(TitlebarOptions {
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
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| Sidebar {
                        rows: jaal_bakeoff_shared::fake_rows(5000)
                            .into_iter()
                            .map(|row| row.title.into())
                            .collect(),
                    })
                },
            )
            .expect("open the sidebar window");
        let child = sidebar
            .update(cx, |_, window, _| hwnd_of(window))
            .expect("the sidebar's HWND") as usize;

        let plain = std::env::var("JAAL_UI_OVERLAY").is_ok_and(|v| v == "plain");
        let overlay = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(450.), px(150.)),
                        size(px(520.), px(160.)),
                    ))),
                    titlebar: None,
                    focus: plain,
                    show: plain,
                    kind: WindowKind::PopUp,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    window_background: WindowBackgroundAppearance::Blurred,
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Overlay),
            )
            .expect("open the overlay window");
        let bar = overlay
            .update(cx, |_, window, _| hwnd_of(window))
            .expect("the overlay's HWND") as usize;

        engine
            .set_page_rect(
                window,
                Rect {
                    x: SIDEBAR_WIDTH as i32,
                    y: 0,
                    width: (client_width - SIDEBAR_WIDTH) as i32,
                    height: client_height as i32,
                },
            )
            .expect("set the page rect");

        // Native calls that move a GPUI window make Windows send it WM_SIZE
        // at once, and GPUI answers by borrowing its App, which this update
        // still holds. A foreground task runs from the message loop instead.
        let parent = parent as usize;
        cx.foreground_executor()
            .spawn(async move {
                let (parent, child, bar) = (parent as HWND, child as HWND, bar as HWND);
                unsafe {
                    // 1. The sidebar becomes a child of the engine's window.
                    let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
                    SetWindowLongPtrW(
                        child,
                        GWL_EXSTYLE,
                        (ex & !(WS_EX_TOOLWINDOW | WS_EX_APPWINDOW)) as isize,
                    );
                    SetWindowLongPtrW(
                        child,
                        GWL_STYLE,
                        (WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS) as isize,
                    );
                    let previous = SetParent(child, parent);
                    let placed = SetWindowPos(
                        child,
                        HWND_TOP,
                        0,
                        0,
                        (SIDEBAR_WIDTH * scale).round() as i32,
                        client.bottom,
                        SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
                    );
                    log(&format!(
                        "sidebar {child:?}: SetParent returned {previous:?}, SetWindowPos {placed}"
                    ));

                    if plain {
                        apply_blur_variant(bar);
                        return;
                    }
                    if std::env::var("JAAL_UI_OVERLAY").is_ok_and(|v| v == "none") {
                        return;
                    }
                    if std::env::var("JAAL_UI_OVERLAY").is_ok_and(|v| v == "child") {
                        // The overlay as a second child window, over the page.
                        let ex = GetWindowLongPtrW(bar, GWL_EXSTYLE) as u32;
                        SetWindowLongPtrW(
                            bar,
                            GWL_EXSTYLE,
                            (ex & !(WS_EX_TOOLWINDOW | WS_EX_APPWINDOW)) as isize,
                        );
                        SetWindowLongPtrW(
                            bar,
                            GWL_STYLE,
                            (WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS) as isize,
                        );
                        SetParent(bar, parent);
                        let left = (SIDEBAR_WIDTH * scale).round() as i32;
                        let width = (520. * scale).round() as i32;
                        let placed = SetWindowPos(
                            bar,
                            HWND_TOP,
                            left + (client.right - left - width) / 2,
                            (72. * scale).round() as i32,
                            width,
                            (160. * scale).round() as i32,
                            SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
                        );
                        log(&format!(
                            "overlay {bar:?} as a child: SetWindowPos {placed}"
                        ));
                        apply_blur_variant(bar);
                        return;
                    }
                    // 2. The overlay is owned by the engine's window and sits
                    //    over the page.
                    SetWindowLongPtrW(bar, GWLP_HWNDPARENT, parent as isize);
                    let round = DWMWCP_ROUND;
                    DwmSetWindowAttribute(
                        bar,
                        DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                        (&raw const round).cast::<c_void>(),
                        size_of_val(&round) as u32,
                    );
                    let mut origin = POINT { x: 0, y: 0 };
                    ClientToScreen(parent, &mut origin);
                    let page_left = origin.x + (SIDEBAR_WIDTH * scale).round() as i32;
                    let page_width = client.right - (SIDEBAR_WIDTH * scale).round() as i32;
                    let width = (520. * scale).round() as i32;
                    let height = (160. * scale).round() as i32;
                    let shown = SetWindowPos(
                        bar,
                        HWND_TOP,
                        page_left + (page_width - width) / 2,
                        origin.y + (72. * scale).round() as i32,
                        width,
                        height,
                        SWP_SHOWWINDOW | SWP_NOACTIVATE,
                    );
                    log(&format!("overlay {bar:?}: SetWindowPos {shown}"));
                    apply_blur_variant(bar);
                }
            })
            .detach();
    });
    log("gpui started in embedded mode");
}

/// JAAL_UI_BLUR picks how the overlay asks Windows for blur, so one build
/// can try each way:
/// - unset: what GPUI's `WindowBackgroundAppearance::Blurred` does.
/// - `accent:<state>:<flags>:<aabbggrr>`: the undocumented accent policy.
/// - `backdrop:<type>:<dark|light>`: Windows 11's DWMWA_SYSTEMBACKDROP_TYPE.
unsafe fn apply_blur_variant(bar: HWND) {
    use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::UI::Controls::MARGINS;

    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }
    #[repr(C)]
    struct CompositionAttribute {
        attribute: u32,
        data: *mut c_void,
        size: usize,
    }
    let set_accent = |state: u32, flags: u32, color: u32| unsafe {
        let user32 = GetModuleHandleA(c"user32.dll".as_ptr().cast());
        let Some(function) =
            GetProcAddress(user32, c"SetWindowCompositionAttribute".as_ptr().cast())
        else {
            return log("no SetWindowCompositionAttribute");
        };
        let function: unsafe extern "system" fn(HWND, *mut CompositionAttribute) -> i32 =
            std::mem::transmute(function);
        let mut policy = AccentPolicy {
            state,
            flags,
            gradient_color: color,
            animation_id: 0,
        };
        let mut data = CompositionAttribute {
            // WCA_ACCENT_POLICY
            attribute: 19,
            data: (&raw mut policy).cast(),
            size: size_of::<AccentPolicy>(),
        };
        let result = function(bar, &mut data);
        log(&format!(
            "accent state {state} flags {flags} color {color:#010x}: {result}"
        ));
    };

    let Ok(variant) = std::env::var("JAAL_UI_BLUR") else {
        return;
    };
    let parts: Vec<&str> = variant.split(':').collect();
    match parts.as_slice() {
        ["accent", state, flags, color] => set_accent(
            state.parse().unwrap(),
            flags.parse().unwrap(),
            u32::from_str_radix(color, 16).unwrap(),
        ),
        ["backdrop", kind, theme] => unsafe {
            set_accent(0, 0, 0);
            // DWMWA_USE_IMMERSIVE_DARK_MODE picks the backdrop's dark or
            // light tint.
            let dark: i32 = (*theme == "dark") as i32;
            DwmSetWindowAttribute(bar, 20, (&raw const dark).cast(), 4);
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let extended = DwmExtendFrameIntoClientArea(bar, &margins);
            let kind: i32 = kind.parse().unwrap();
            // DWMWA_SYSTEMBACKDROP_TYPE
            let set = DwmSetWindowAttribute(bar, 38, (&raw const kind).cast(), 4);
            log(&format!(
                "backdrop {kind}: extend {extended:#x}, set {set:#x}"
            ));
        },
        _ => log(&format!("unknown JAAL_UI_BLUR {variant}")),
    }
}
