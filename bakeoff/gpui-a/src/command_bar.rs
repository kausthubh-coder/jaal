//! The Cmd+K command bar: a second, borderless GPUI window with a blurred
//! backdrop, attached over the page as a child window (fix A, see
//! overlay.rs). It takes keyboard focus while open and closes when it loses
//! it, which is what a click anywhere outside it does.

use crate::browser::{BrowserHandle, BrowserWindow, CloseTab, NewTab, Quit, ToggleCommandBar};
use crate::overlay;
use crate::text_input::{TextInput, TextInputEvent};
use gpui::{
    App, Bounds, Context, Entity, Focusable, Pixels, SharedString, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point,
    prelude::*, px, rgb, rgba, size,
};
use jaal_bakeoff_shared::TabId;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;

const TEXT: u32 = 0x1f1c18;
const TEXT_MUTED: u32 = 0x6f6961;

pub struct CommandBar {
    owner: BrowserHandle,
    input: Entity<TextInput>,
    gpui_view: *mut c_void,
    tabs: Vec<(TabId, SharedString, bool)>,
    was_active: bool,
    _subscriptions: Vec<Subscription>,
}

impl CommandBar {
    /// Opens the bar over `page`. `main_bounds` is the main window's bounds
    /// in GPUI's screen coordinates, `tabs` the open tabs (id, label,
    /// active).
    pub fn open(
        owner: BrowserHandle,
        parent_view: *mut c_void,
        main_bounds: Bounds<Pixels>,
        page: Bounds<Pixels>,
        tabs: Vec<(TabId, SharedString, bool)>,
        cx: &mut App,
    ) -> Option<WindowHandle<CommandBar>> {
        let (x, y, width, height) = overlay::bar_bounds(page);
        let bounds = Bounds::new(
            point(
                main_bounds.origin.x + px(x as f32),
                main_bounds.origin.y + px(y as f32),
            ),
            size(px(width as f32), px(height as f32)),
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: true,
            show: true,
            // An NSPanel: it can be key without the app's main window
            // looking inactive behind it.
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Blurred,
            ..Default::default()
        };
        let bar = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| CommandBar::new(owner, tabs, window, cx))
            })
            .map_err(|e| eprintln!("gpui-a: could not open the command bar: {e}"))
            .ok()?;
        let _ = bar.update(cx, |bar, window, _| {
            unsafe {
                overlay::attach(parent_view, bar.gpui_view);
                overlay::place(parent_view, bar.gpui_view, page);
            }
            window.activate_window();
        });
        Some(bar)
    }

    fn new(
        owner: BrowserHandle,
        tabs: Vec<(TabId, SharedString, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let gpui_view = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::AppKit(handle)) => handle.ns_view.as_ptr(),
            _ => panic!("gpui-a runs on macOS only"),
        };
        let input = cx.new(|cx| {
            TextInput::new(
                "Search, enter a URL, or pick a tab",
                rgb(TEXT).into(),
                rgba(0x1f1c1873).into(),
                cx,
            )
        });
        window.focus(&input.read(cx).focus_handle(cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &input,
                window,
                |this, _, event: &TextInputEvent, _, cx| match event {
                    TextInputEvent::Submit(text) => {
                        let text = text.clone();
                        this.finish(cx, move |browser, window, cx| {
                            browser.navigate(&text, window, cx)
                        })
                    }
                    TextInputEvent::Cancel => this.finish(cx, |_, _, _| {}),
                },
            ),
            cx.observe_window_activation(window, |this, window, cx| {
                let active = window.is_window_active();
                // Losing key status after having had it means a click (or
                // Cmd+Tab) went elsewhere.
                if this.was_active && !active {
                    this.dismiss(cx);
                }
                this.was_active |= active;
            }),
        ];
        Self {
            owner,
            input,
            gpui_view,
            tabs,
            was_active: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn gpui_view(&self) -> *mut c_void {
        self.gpui_view
    }

    pub fn query(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }

    /// Closes without touching focus: whatever took it keeps it.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        let _ = self
            .owner
            .update(cx, |browser, _, cx| browser.close_command_bar(cx));
    }

    /// Runs `action` on the main window, closes the bar and gives focus back
    /// to the main window.
    fn finish(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut BrowserWindow, &mut Window, &mut Context<BrowserWindow>) + 'static,
    ) {
        let owner = self.owner;
        cx.defer(move |cx| {
            let _ = owner.update(cx, |browser, window, cx| {
                action(browser, window, cx);
                browser.close_command_bar(cx);
                window.activate_window();
            });
        });
    }
}

impl Render for CommandBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.tabs.iter().take(6).map(|(id, label, active)| {
            let id = *id;
            div()
                .id(("bar-tab", id as usize))
                .h(px(34.))
                .px_3()
                .mx_2()
                .flex()
                .items_center()
                .gap_3()
                .rounded_md()
                .text_sm()
                .text_color(rgb(TEXT))
                .hover(|d| d.bg(rgba(0x0000000f)))
                .child(div().size(px(14.)).rounded_sm().bg(rgb(0xc9c3ba)))
                .child(div().flex_1().truncate().child(label.clone()))
                .when(*active, |d| {
                    d.child(div().text_xs().text_color(rgb(TEXT_MUTED)).child("Current"))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.finish(cx, move |browser, window, cx| {
                        browser.activate_tab(id, window, cx)
                    })
                }))
        });

        let owner = self.owner;
        let forward =
            move |cx: &mut App,
                  f: fn(&mut BrowserWindow, &mut Window, &mut Context<BrowserWindow>)| {
                // The main window is another window, so update it after this
                // one's dispatch returns.
                cx.defer(move |cx| {
                    let _ = owner.update(cx, f);
                });
            };
        div()
            .size_full()
            .flex()
            .flex_col()
            .rounded_xl()
            .on_action(move |_: &NewTab, _, cx| forward(cx, |b, w, cx| b.new_tab(w, cx)))
            .on_action(move |_: &CloseTab, _, cx| forward(cx, |b, w, cx| b.close_active_tab(w, cx)))
            .on_action(move |_: &ToggleCommandBar, _, cx| {
                forward(cx, |b, _, cx| b.close_command_bar(cx))
            })
            .on_action(move |_: &Quit, _, cx| forward(cx, |b, _, cx| b.quit(cx)))
            .bg(rgba(0xfbfaf8a6))
            .font_family(".SystemUIFont")
            .text_color(rgb(TEXT))
            .child(
                div()
                    .h(px(58.))
                    .flex_none()
                    .px_5()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_size(px(18.))
                    .line_height(px(24.))
                    .child(
                        div()
                            .size(px(16.))
                            .flex_none()
                            .rounded_full()
                            .border_2()
                            .border_color(rgb(TEXT_MUTED)),
                    )
                    .child(self.input.clone()),
            )
            .child(div().h(px(1.)).bg(rgba(0x0000001a)))
            .child(
                div()
                    .px_5()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child("Switch to tab"),
            )
            .child(div().flex_1().flex().flex_col().gap(px(2.)).children(tabs))
            .child(
                div()
                    .h(px(34.))
                    .flex_none()
                    .px_5()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .bg(rgba(0x0000000a))
                    .child("↵  Open")
                    .child("esc  Close")
                    .child("⌘T  New tab"),
            )
    }
}
