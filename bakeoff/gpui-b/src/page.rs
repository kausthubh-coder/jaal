//! Fix B: each tab is a windowless CEF browser. Chromium renders the page
//! into a shared IOSurface and hands it to `on_accelerated_paint`; GPUI
//! samples that IOSurface in its own Metal pass (`PageView`), so anything
//! GPUI draws afterwards sits over the page in the same window. The price is
//! that the page has no NSView of its own: every mouse move, click, wheel
//! tick and key goes from GPUI to CEF through the functions here.

// core-video 0.4, where gpui's CVPixelBuffer type comes from, wraps an
// IOSurface only through the older io-surface crate's types.
#![allow(deprecated)]

use core_foundation::base::TCFType;
use core_video::pixel_buffer::{CVPixelBuffer, kCVPixelFormatType_32BGRA};
use gpui::{
    App, Bounds, Corners, CursorStyle, DispatchPhase, Element, ElementId, FocusHandle,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollDelta,
    ScrollWheelEvent, Size, Style, Window, point, px, size,
};
use io_surface::{IOSurface, IOSurfaceRef};
use jaal_gpui_cef::EventHandler;
use jaal_gpui_cef::cef::*;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

/// Chromium's own pixels per wheel "line" on the Mac
/// (`kScrollbarPixelsPerCocoaTick`).
const PIXELS_PER_LINE: f32 = 40.;

/// The page's frame rate. CEF's default for windowless browsers is 30.
const FRAME_RATE: i32 = 60;

/// One painted frame: the IOSurface Chromium drew into, wrapped as the
/// CVPixelBuffer GPUI's surface primitive takes. No pixels are copied; the
/// buffer retains the IOSurface.
///
/// CEF hands the IOSurface back to Chromium's pool when the callback
/// returns, and Chromium does not wait for us: it double-buffers, drawing
/// the next frame into the other surface, and then reuses this one even if
/// we still hold it (its use count doesn't stop that; checked by logging
/// surface IDs). So GPUI only ever draws the newest frame, the one Chromium
/// isn't drawing into. What's left is a short race: a GPUI frame still on
/// the GPU, sampling a surface Chromium has just started drawing into again.
fn frame_buffer(surface: IOSurfaceRef) -> Option<CVPixelBuffer> {
    let surface: IOSurface = unsafe { TCFType::wrap_under_get_rule(surface) };
    CVPixelBuffer::from_io_surface(&surface, None).ok()
}

/// What the render handler and the UI share for one page. Main thread only:
/// CEF calls the render handler on its UI thread, which is our main thread.
struct PageState {
    /// The page area in points; what `view_rect` reports.
    view_size: Cell<(i32, i32)>,
    scale: Cell<f32>,
    frame: RefCell<Option<CVPixelBuffer>>,
    cursor: Cell<CursorStyle>,
    /// Asks GPUI for a redraw. Called inside CEF callbacks, so it only
    /// queues.
    on_frame: Box<dyn Fn()>,
    warned_cpu_paint: Cell<bool>,
    /// Frames painted so far, for the demo's frame-rate check.
    paints: Cell<u64>,
}

wrap_render_handler! {
    struct PageRenderHandler {
        state: Rc<PageState>,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let (width, height) = self.state.view_size.get();
                *rect = Rect { x: 0, y: 0, width: width.max(1), height: height.max(1) };
            }
        }

        fn screen_info(&self, _browser: Option<&mut Browser>, screen_info: Option<&mut ScreenInfo>) -> ::std::os::raw::c_int {
            let Some(info) = screen_info else { return 0 };
            info.device_scale_factor = self.state.scale.get();
            1
        }

        fn on_accelerated_paint(&self, _browser: Option<&mut Browser>, type_: PaintElementType, _dirty_rects: Option<&[Rect]>, info: Option<&AcceleratedPaintInfo>) {
            // Popups (HTML select lists) are skipped in the bake-off.
            if type_ != PaintElementType::VIEW {
                return;
            }
            let Some(info) = info else { return };
            let surface = info.shared_texture_io_surface as IOSurfaceRef;
            if surface.is_null() {
                return;
            }
            let Some(buffer) = frame_buffer(surface) else {
                eprintln!("gpui-b: could not wrap the page's IOSurface");
                return;
            };
            if buffer.get_pixel_format() != kCVPixelFormatType_32BGRA {
                eprintln!("gpui-b: page IOSurface isn't BGRA, skipping the frame");
                return;
            }
            *self.state.frame.borrow_mut() = Some(buffer);
            self.state.paints.set(self.state.paints.get() + 1);
            (self.state.on_frame)();
        }

        fn on_paint(&self, _browser: Option<&mut Browser>, _type_: PaintElementType, _dirty_rects: Option<&[Rect]>, _buffer: *const u8, _width: ::std::os::raw::c_int, _height: ::std::os::raw::c_int) {
            // With shared textures on, CEF only calls this if it couldn't
            // use the GPU. gpui-b has no CPU path: say so instead of showing
            // a blank page silently.
            if !self.state.warned_cpu_paint.replace(true) {
                eprintln!("gpui-b: CEF sent CPU pixels (on_paint); shared textures aren't working");
            }
        }
    }
}

/// A tab's windowless browser. Cheap to clone; clones share the browser.
#[derive(Clone)]
pub struct Page {
    browser: Browser,
    state: Rc<PageState>,
    input: Rc<InputState>,
}

/// Mouse and wheel state the forwarding needs between events.
#[derive(Default)]
struct InputState {
    /// The button held since a press inside the page. Moves and the release
    /// go to the page while it is held, also outside it, like a native view's
    /// mouse capture.
    pressed: Cell<Option<MouseButton>>,
    hovered: Cell<bool>,
    /// CEF takes whole wheel pixels; trackpad deltas are fractional. The
    /// remainder carries over so slow scrolls and momentum tails aren't lost.
    wheel_remainder: Cell<(f32, f32)>,
}

impl Page {
    /// Creates the browser at `view_size` points and `scale`. `on_frame` is
    /// called (inside a CEF callback) whenever a new frame is ready.
    pub fn create(
        gpui_view: *mut c_void,
        url: &str,
        view_size: Size<Pixels>,
        scale: f32,
        handler: EventHandler,
        on_frame: Box<dyn Fn()>,
    ) -> Option<Page> {
        let state = Rc::new(PageState {
            view_size: Cell::new(dips(view_size)),
            scale: Cell::new(scale),
            frame: RefCell::new(None),
            cursor: Cell::new(CursorStyle::Arrow),
            on_frame,
            warned_cpu_paint: Cell::new(false),
            paints: Cell::new(0),
        });
        let mut client =
            jaal_gpui_cef::windowless_client(handler, PageRenderHandler::new(state.clone()));
        let window_info = WindowInfo {
            windowless_rendering_enabled: 1,
            shared_texture_enabled: 1,
            // CEF uses it for screen info and IME placement; nothing is
            // added to it.
            parent_view: gpui_view,
            runtime_style: RuntimeStyle::ALLOY,
            ..Default::default()
        };
        let settings = BrowserSettings {
            windowless_frame_rate: FRAME_RATE,
            ..Default::default()
        };
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&CefString::from(url)),
            Some(&settings),
            None,
            None,
        )?;
        Some(Page {
            browser,
            state,
            input: Rc::default(),
        })
    }

    pub fn id(&self) -> i32 {
        self.browser.identifier()
    }

    fn host(&self) -> Option<BrowserHost> {
        self.browser.host()
    }

    /// Hidden pages stop rendering and running animation frames.
    pub fn set_hidden(&self, hidden: bool) {
        if let Some(host) = self.host() {
            host.was_hidden(hidden as i32);
        }
        if hidden {
            self.mouse_leave();
        }
    }

    pub fn set_focus(&self, focused: bool) {
        if let Some(host) = self.host() {
            host.set_focus(focused as i32);
        }
    }

    /// Tells CEF about a new page size or scale. Cheap when nothing changed.
    pub fn set_view(&self, view_size: Size<Pixels>, scale: f32) {
        let new_size = dips(view_size);
        let size_changed = self.state.view_size.replace(new_size) != new_size;
        let scale_changed = self.state.scale.replace(scale) != scale;
        let Some(host) = self.host() else { return };
        if scale_changed {
            host.notify_screen_info_changed();
        }
        if size_changed || scale_changed {
            host.was_resized();
        }
    }

    /// The last painted frame and its size in points. The frame keeps the
    /// size it was painted at; during a resize it lags the window by a frame
    /// or two, and is drawn at that size rather than stretched.
    pub fn frame(&self) -> Option<(CVPixelBuffer, Size<Pixels>)> {
        let frame = self.state.frame.borrow();
        let buffer = frame.as_ref()?;
        let scale = self.state.scale.get();
        let size = size(
            px(buffer.get_width() as f32 / scale),
            px(buffer.get_height() as f32 / scale),
        );
        Some((buffer.clone(), size))
    }

    pub fn paint_count(&self) -> u64 {
        self.state.paints.get()
    }

    pub fn set_cursor(&self, cursor: CursorType) {
        self.state.cursor.set(cursor_style(cursor));
    }

    pub fn load_url(&self, url: &str) {
        if let Some(frame) = self.browser.main_frame() {
            frame.load_url(Some(&CefString::from(url)));
        }
    }

    pub fn run_js(&self, code: &str) {
        if let Some(frame) = self.browser.main_frame() {
            frame.execute_java_script(Some(&CefString::from(code)), None, 0);
        }
    }

    /// Edit commands. On the Mac these reach a page through the Edit menu's
    /// selectors, not as keys, so Cmd+C sent as a key event does nothing.
    pub fn edit_command(&self, key: &str, shift: bool) -> bool {
        let Some(frame) = self.browser.main_frame() else {
            return false;
        };
        match (key, shift) {
            ("c", false) => frame.copy(),
            ("x", false) => frame.cut(),
            ("v", false) => frame.paste(),
            ("a", false) => frame.select_all(),
            ("z", false) => frame.undo(),
            ("z", true) => frame.redo(),
            _ => return false,
        }
        true
    }

    /// Starts closing; `BeforeClose` follows once CEF is done.
    pub fn close(&self) {
        if let Some(host) = self.host() {
            host.close_browser(1);
        }
    }

    fn mouse_move(&self, position: gpui::Point<Pixels>, modifiers: &Modifiers) {
        let Some(host) = self.host() else { return };
        let event = self.mouse_event(position, modifiers);
        host.send_mouse_move_event(Some(&event), 0);
    }

    fn mouse_leave(&self) {
        if !self.input.hovered.replace(false) {
            return;
        }
        if let Some(host) = self.host() {
            host.send_mouse_move_event(Some(&MouseEvent::default()), 1);
        }
    }

    fn mouse_button(
        &self,
        position: gpui::Point<Pixels>,
        button: MouseButton,
        up: bool,
        click_count: usize,
        modifiers: &Modifiers,
    ) {
        let Some(host) = self.host() else { return };
        let button_type = match button {
            MouseButton::Left => MouseButtonType::LEFT,
            MouseButton::Middle => MouseButtonType::MIDDLE,
            MouseButton::Right => MouseButtonType::RIGHT,
            // Back and forward buttons aren't part of the bake-off.
            MouseButton::Navigate(_) => return,
        };
        let event = self.mouse_event(position, modifiers);
        host.send_mouse_click_event(
            Some(&event),
            button_type,
            up as i32,
            click_count.max(1) as i32,
        );
    }

    fn wheel(&self, position: gpui::Point<Pixels>, delta: ScrollDelta, modifiers: &Modifiers) {
        let Some(host) = self.host() else { return };
        // NSEvent's scrollingDelta has the sign CEF wants: positive scrolls
        // toward the top, as a wheel turned away from you does.
        let delta = delta.pixel_delta(px(PIXELS_PER_LINE));
        let (rest_x, rest_y) = self.input.wheel_remainder.get();
        let x = f32::from(delta.x) + rest_x;
        let y = f32::from(delta.y) + rest_y;
        let (whole_x, whole_y) = (x.trunc(), y.trunc());
        self.input.wheel_remainder.set((x - whole_x, y - whole_y));
        if whole_x == 0. && whole_y == 0. {
            return;
        }
        let event = self.mouse_event(position, modifiers);
        host.send_mouse_wheel_event(Some(&event), whole_x as i32, whole_y as i32);
    }

    /// Forwards the key event GPUI is dispatching right now. GPUI's key
    /// events carry no Mac key code, which CEF needs on the Mac to rebuild
    /// the event, so this reads the NSEvent AppKit is delivering instead,
    /// as cefclient's windowless view does. Returns false if there is none.
    pub fn forward_current_key_event(&self) -> bool {
        let Some(host) = self.host() else {
            return false;
        };
        let Some(key) = (unsafe { current_key_event() }) else {
            return false;
        };
        let mut event = KeyEvent {
            size: std::mem::size_of::<KeyEvent>(),
            type_: if key.down {
                KeyEventType::RAWKEYDOWN
            } else {
                KeyEventType::KEYUP
            },
            modifiers: key.modifiers,
            native_key_code: key.key_code as i32,
            character: key.character,
            unmodified_character: key.unmodified_character,
            ..Default::default()
        };
        host.send_key_event(Some(&event));
        // Text arrives as a CHAR event after the key down, the way
        // cefclient sends what `insertText:` gets. Without IME this is one
        // character per key; function keys and Cmd/Ctrl shortcuts have none.
        let command_or_control = key.modifiers
            & (sys::cef_event_flags_t::EVENTFLAG_COMMAND_DOWN.0
                | sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0)
            != 0;
        if key.down && !command_or_control && is_text(key.character) {
            event.type_ = KeyEventType::CHAR;
            host.send_key_event(Some(&event));
        }
        true
    }

    fn mouse_event(&self, position: gpui::Point<Pixels>, modifiers: &Modifiers) -> MouseEvent {
        let mut flags = event_flags(modifiers);
        if let Some(button) = self.input.pressed.get() {
            flags |= match button {
                MouseButton::Left => sys::cef_event_flags_t::EVENTFLAG_LEFT_MOUSE_BUTTON.0,
                MouseButton::Middle => sys::cef_event_flags_t::EVENTFLAG_MIDDLE_MOUSE_BUTTON.0,
                MouseButton::Right => sys::cef_event_flags_t::EVENTFLAG_RIGHT_MOUSE_BUTTON.0,
                MouseButton::Navigate(_) => 0,
            };
        }
        MouseEvent {
            x: f32::from(position.x).round() as i32,
            y: f32::from(position.y).round() as i32,
            modifiers: flags,
        }
    }
}

fn dips(view_size: Size<Pixels>) -> (i32, i32) {
    (
        f32::from(view_size.width).round() as i32,
        f32::from(view_size.height).round() as i32,
    )
}

fn event_flags(modifiers: &Modifiers) -> u32 {
    use sys::cef_event_flags_t as f;
    let mut flags = 0;
    if modifiers.shift {
        flags |= f::EVENTFLAG_SHIFT_DOWN.0;
    }
    if modifiers.control {
        flags |= f::EVENTFLAG_CONTROL_DOWN.0;
    }
    if modifiers.alt {
        flags |= f::EVENTFLAG_ALT_DOWN.0;
    }
    if modifiers.platform {
        flags |= f::EVENTFLAG_COMMAND_DOWN.0;
    }
    flags
}

/// Printable: not a control character, not DEL, not one of AppKit's
/// function-key characters (arrows, F-keys, Home...). Return is text too:
/// forms submit on its keypress.
fn is_text(character: u16) -> bool {
    character == u16::from(b'\r')
        || (character >= 0x20 && character != 0x7f && !(0xf700..=0xf8ff).contains(&character))
}

struct NativeKey {
    down: bool,
    key_code: u16,
    character: u16,
    unmodified_character: u16,
    modifiers: u32,
}

const NS_EVENT_TYPE_KEY_DOWN: usize = 10;
const NS_EVENT_TYPE_KEY_UP: usize = 11;

/// `[NSApp currentEvent]`, if it is a key down or up.
unsafe fn current_key_event() -> Option<NativeKey> {
    unsafe {
        let app: *mut AnyObject = msg_send![AnyClass::get(c"NSApplication")?, sharedApplication];
        let event: *mut AnyObject = msg_send![app, currentEvent];
        if event.is_null() {
            return None;
        }
        let kind: usize = msg_send![event, type];
        let down = match kind {
            NS_EVENT_TYPE_KEY_DOWN => true,
            NS_EVENT_TYPE_KEY_UP => false,
            _ => return None,
        };
        let key_code: u16 = msg_send![event, keyCode];
        let flags: usize = msg_send![event, modifierFlags];
        let characters: *mut AnyObject = msg_send![event, characters];
        let unmodified: *mut AnyObject = msg_send![event, charactersIgnoringModifiers];
        Some(NativeKey {
            down,
            key_code,
            character: first_utf16(characters),
            unmodified_character: first_utf16(unmodified),
            modifiers: native_flags(flags),
        })
    }
}

unsafe fn first_utf16(string: *mut AnyObject) -> u16 {
    if string.is_null() {
        return 0;
    }
    unsafe {
        let length: usize = msg_send![string, length];
        if length == 0 {
            return 0;
        }
        msg_send![string, characterAtIndex: 0usize]
    }
}

fn native_flags(flags: usize) -> u32 {
    use sys::cef_event_flags_t as f;
    const CAPS_LOCK: usize = 1 << 16;
    const SHIFT: usize = 1 << 17;
    const CONTROL: usize = 1 << 18;
    const OPTION: usize = 1 << 19;
    const COMMAND: usize = 1 << 20;
    const NUMERIC_PAD: usize = 1 << 21;
    let mut out = 0;
    for (mask, flag) in [
        (CAPS_LOCK, f::EVENTFLAG_CAPS_LOCK_ON.0),
        (SHIFT, f::EVENTFLAG_SHIFT_DOWN.0),
        (CONTROL, f::EVENTFLAG_CONTROL_DOWN.0),
        (OPTION, f::EVENTFLAG_ALT_DOWN.0),
        (COMMAND, f::EVENTFLAG_COMMAND_DOWN.0),
        (NUMERIC_PAD, f::EVENTFLAG_IS_KEY_PAD.0),
    ] {
        if flags & mask != 0 {
            out |= flag;
        }
    }
    out
}

/// The GPUI cursor closest to CEF's.
fn cursor_style(cursor: CursorType) -> CursorStyle {
    match cursor {
        CursorType::HAND => CursorStyle::PointingHand,
        CursorType::IBEAM => CursorStyle::IBeam,
        CursorType::VERTICALTEXT => CursorStyle::IBeamCursorForVerticalLayout,
        CursorType::CROSS | CursorType::CELL => CursorStyle::Crosshair,
        CursorType::EASTRESIZE => CursorStyle::ResizeRight,
        CursorType::WESTRESIZE => CursorStyle::ResizeLeft,
        CursorType::EASTWESTRESIZE => CursorStyle::ResizeLeftRight,
        CursorType::NORTHRESIZE => CursorStyle::ResizeUp,
        CursorType::SOUTHRESIZE => CursorStyle::ResizeDown,
        CursorType::NORTHSOUTHRESIZE => CursorStyle::ResizeUpDown,
        CursorType::NORTHWESTSOUTHEASTRESIZE
        | CursorType::NORTHWESTRESIZE
        | CursorType::SOUTHEASTRESIZE => CursorStyle::ResizeUpLeftDownRight,
        CursorType::NORTHEASTSOUTHWESTRESIZE
        | CursorType::NORTHEASTRESIZE
        | CursorType::SOUTHWESTRESIZE => CursorStyle::ResizeUpRightDownLeft,
        CursorType::COLUMNRESIZE => CursorStyle::ResizeColumn,
        CursorType::ROWRESIZE => CursorStyle::ResizeRow,
        CursorType::GRAB | CursorType::MOVE => CursorStyle::OpenHand,
        CursorType::GRABBING => CursorStyle::ClosedHand,
        CursorType::NOTALLOWED | CursorType::NODROP => CursorStyle::OperationNotAllowed,
        CursorType::ALIAS => CursorStyle::DragLink,
        CursorType::COPY => CursorStyle::DragCopy,
        _ => CursorStyle::Arrow,
    }
}

/// Draws the active page's texture and forwards the mouse to it. Keys come
/// from the focusable div around it (`BrowserWindow::render`).
pub struct PageView {
    page: Option<Page>,
    focus: FocusHandle,
    corner_radius: Pixels,
    /// Where the page texture was drawn this frame, for the command bar's
    /// blurred backdrop.
    drawn_at: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl PageView {
    pub fn new(
        page: Option<Page>,
        focus: FocusHandle,
        corner_radius: Pixels,
        drawn_at: Rc<Cell<Option<Bounds<Pixels>>>>,
    ) -> Self {
        PageView {
            page,
            focus,
            corner_radius,
            drawn_at,
        }
    }
}

impl IntoElement for PageView {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for PageView {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.).into();
        style.size.height = gpui::relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> Hitbox {
        if let Some(page) = &self.page {
            page.set_view(bounds.size, window.scale_factor());
        }
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        _cx: &mut App,
    ) {
        self.drawn_at.set(None);
        let Some(page) = self.page.clone() else {
            return;
        };
        if let Some((buffer, frame_size)) = page.frame() {
            let drawn_at = Bounds::new(bounds.origin, frame_size);
            window.paint_surface_clipped(
                drawn_at,
                buffer,
                bounds,
                Corners::all(self.corner_radius),
                px(0.),
            );
            self.drawn_at.set(Some(drawn_at));
        }
        window.set_cursor_style(page.state.cursor.get(), hitbox);

        let origin = bounds.origin;
        let local = move |p: gpui::Point<Pixels>| point(p.x - origin.x, p.y - origin.y);

        let (move_page, move_hitbox) = (page.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let input = &move_page.input;
            if move_hitbox.is_hovered(window) || input.pressed.get().is_some() {
                input.hovered.set(true);
                move_page.mouse_move(local(event.position), &event.modifiers);
            } else {
                move_page.mouse_leave();
            }
        });

        let (down_page, down_hitbox, focus) = (page.clone(), hitbox.clone(), self.focus.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble || !down_hitbox.is_hovered(window) {
                return;
            }
            window.focus(&focus);
            down_page.input.pressed.set(Some(event.button));
            down_page.mouse_button(
                local(event.position),
                event.button,
                false,
                event.click_count,
                &event.modifiers,
            );
        });

        let (up_page, up_hitbox) = (page.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let input = &up_page.input;
            let captured = input.pressed.get() == Some(event.button);
            if !captured && !up_hitbox.is_hovered(window) {
                return;
            }
            up_page.mouse_button(
                local(event.position),
                event.button,
                true,
                event.click_count,
                &event.modifiers,
            );
            if captured {
                input.pressed.set(None);
            }
        });

        let (wheel_page, wheel_hitbox) = (page, hitbox.clone());
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble || !wheel_hitbox.should_handle_scroll(window) {
                return;
            }
            wheel_page.wheel(local(event.position), event.delta, &event.modifiers);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_characters_are_text_but_function_keys_are_not() {
        assert!(is_text(u16::from(b'a')));
        assert!(is_text(u16::from(b' ')));
        assert!(is_text(u16::from(b'\r')));
        assert!(is_text('é' as u16));
        // Backspace, escape, tab, the left arrow (NSLeftArrowFunctionKey), F1.
        for key in [0x7f, 0x1b, 0x09, 0xf702, 0xf704] {
            assert!(!is_text(key), "{key:#x}");
        }
    }

    #[test]
    fn mac_modifier_flags_become_cef_flags() {
        use sys::cef_event_flags_t as f;
        let shift_command = (1 << 17) | (1 << 20);
        assert_eq!(
            native_flags(shift_command),
            f::EVENTFLAG_SHIFT_DOWN.0 | f::EVENTFLAG_COMMAND_DOWN.0
        );
        assert_eq!(native_flags(0), 0);
    }

    #[test]
    fn page_cursors_map_to_gpui_cursors() {
        assert_eq!(cursor_style(CursorType::HAND), CursorStyle::PointingHand);
        assert_eq!(cursor_style(CursorType::IBEAM), CursorStyle::IBeam);
        assert_eq!(cursor_style(CursorType::POINTER), CursorStyle::Arrow);
    }
}
