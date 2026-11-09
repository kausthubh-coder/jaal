//! The Cmd+K command bar, drawn by GPUI in the main window over the page
//! (fix B). `BrowserWindow` draws its box: the drop shadow and a blurred
//! copy of the page texture behind it. This view is what's inside.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription, Window,
    div, prelude::*, px, rgb, rgba,
};
use jaal_bakeoff_shared::TabId;
use jaal_gpui_cef::text_input::{TextInput, TextInputEvent};

const TEXT: u32 = 0x1f1c18;
const TEXT_MUTED: u32 = 0x6f6961;

/// The bar's size and its distance below the page's top edge, in points.
/// Same as gpui-a's.
pub const BAR_WIDTH: f32 = 620.;
pub const BAR_HEIGHT: f32 = 340.;
pub const BAR_TOP: f32 = 72.;
pub const BAR_MARGIN: f32 = 24.;
pub const BAR_RADIUS: f32 = 12.;

pub enum CommandBarEvent {
    Navigate(String),
    ActivateTab(TabId),
    Close,
}

pub struct CommandBar {
    input: Entity<TextInput>,
    tabs: Vec<(TabId, SharedString, bool)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CommandBarEvent> for CommandBar {}

impl CommandBar {
    /// `tabs` are the open tabs: id, label, whether it's the active one.
    pub fn new(
        tabs: Vec<(TabId, SharedString, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                "Search, enter a URL, or pick a tab",
                rgb(TEXT).into(),
                rgba(0x1f1c1873).into(),
                cx,
            )
        });
        window.focus(&input.read(cx).focus_handle(cx));
        let subscriptions = vec![cx.subscribe(&input, |_, _, event: &TextInputEvent, cx| {
            cx.emit(match event {
                TextInputEvent::Submit(text) => CommandBarEvent::Navigate(text.clone()),
                TextInputEvent::Cancel => CommandBarEvent::Close,
            })
        })];
        Self {
            input,
            tabs,
            _subscriptions: subscriptions,
        }
    }

    pub fn query(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }
}

impl Focusable for CommandBar {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
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
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(CommandBarEvent::ActivateTab(id))))
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(BAR_RADIUS))
            .overflow_hidden()
            .bg(rgba(0xfbfaf8a6))
            .border_1()
            .border_color(rgba(0x0000001a))
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
