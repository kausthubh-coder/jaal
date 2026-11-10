//! Drives the safe API the way a UI library does, against a fake engine
//! table, and checks exactly what crosses the C boundary.

use super::*;
use std::cell::RefCell;
use std::sync::Mutex;

// The fake engine's memory of the last calls. Tests run on many threads, so
// each test serialises on LOCK and resets the record first.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Record {
    opened_url: Option<String>,
    opened_foreground: Option<u8>,
    open_calls: u32,
    close_status: i32,
    sink: sys::jaal_event_sink,
    sink_user_data: usize,
    page_placement: Option<sys::jaal_placement>,
    attached_view: Option<(usize, sys::jaal_view_options)>,
    filter: sys::jaal_shortcut_filter,
    filter_user_data: usize,
    window_state: Option<sys::jaal_window_state>,
    window_options: Option<sys::jaal_window_options>,
    window_style: Option<sys::jaal_window_style>,
    nav_action: Option<sys::jaal_nav_action>,
    nav_status: i32,
}

thread_local! {
    static RECORD: RefCell<Record> = RefCell::new(Record::default());
}

unsafe extern "C" fn fake_set_event_sink(
    sink: sys::jaal_event_sink,
    user_data: *mut c_void,
) -> sys::jaal_status {
    RECORD.with_borrow_mut(|r| {
        r.sink = sink;
        r.sink_user_data = user_data as usize;
    });
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_window_create(out: *mut sys::jaal_handle) -> sys::jaal_status {
    // SAFETY: the wrapper passes a valid pointer.
    unsafe {
        *out = sys::jaal_handle {
            slot: 3,
            generation: 7,
        }
    };
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_window_close(_window: sys::jaal_handle) -> sys::jaal_status {
    sys::jaal_status(RECORD.with_borrow(|r| r.close_status))
}

unsafe extern "C" fn fake_tab_open(
    _window: sys::jaal_handle,
    url: *const c_char,
    foreground: u8,
    out: *mut sys::jaal_handle,
) -> sys::jaal_status {
    // SAFETY: the wrapper passes a NUL-terminated string.
    let url = unsafe { CStr::from_ptr(url) }.to_str().unwrap().to_owned();
    RECORD.with_borrow_mut(|r| {
        r.opened_url = Some(url);
        r.opened_foreground = Some(foreground);
        r.open_calls += 1;
    });
    // SAFETY: the wrapper passes a valid pointer.
    unsafe {
        *out = sys::jaal_handle {
            slot: 9,
            generation: 1,
        }
    };
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_set_page_placement(
    _window: sys::jaal_handle,
    placement: sys::jaal_placement,
) -> sys::jaal_status {
    RECORD.with_borrow_mut(|r| r.page_placement = Some(placement));
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_attach_view(
    _window: sys::jaal_handle,
    native_view: *mut c_void,
    options: *const sys::jaal_view_options,
) -> sys::jaal_status {
    // SAFETY: the wrapper passes a valid pointer.
    let options = unsafe { *options };
    RECORD.with_borrow_mut(|r| r.attached_view = Some((native_view as usize, options)));
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_set_shortcut_filter(
    filter: sys::jaal_shortcut_filter,
    user_data: *mut c_void,
) -> sys::jaal_status {
    RECORD.with_borrow_mut(|r| {
        r.filter = filter;
        r.filter_user_data = user_data as usize;
    });
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_set_state(
    _window: sys::jaal_handle,
    state: sys::jaal_window_state,
) -> sys::jaal_status {
    RECORD.with_borrow_mut(|r| r.window_state = Some(state));
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_window_create_with(
    options: *const sys::jaal_window_options,
    out: *mut sys::jaal_handle,
) -> sys::jaal_status {
    // SAFETY: the wrapper passes valid pointers.
    unsafe {
        RECORD.with_borrow_mut(|r| r.window_options = Some(*options));
        *out = sys::jaal_handle {
            slot: 4,
            generation: 2,
        };
    }
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_set_style(
    _window: sys::jaal_handle,
    style: *const sys::jaal_window_style,
) -> sys::jaal_status {
    // SAFETY: the wrapper passes a valid pointer.
    RECORD.with_borrow_mut(|r| r.window_style = Some(unsafe { *style }));
    sys::jaal_status::JAAL_OK
}

unsafe extern "C" fn fake_tab_navigate(
    _tab: sys::jaal_handle,
    action: sys::jaal_nav_action,
) -> sys::jaal_status {
    RECORD.with_borrow_mut(|r| r.nav_action = Some(action));
    sys::jaal_status(RECORD.with_borrow(|r| r.nav_status))
}

fn fake_table(abi_major: u32) -> &'static sys::jaal_engine_api {
    Box::leak(Box::new(sys::jaal_engine_api {
        struct_size: std::mem::size_of::<sys::jaal_engine_api>() as u32,
        abi_major,
        abi_minor: 0,
        set_event_sink: Some(fake_set_event_sink),
        window_create: Some(fake_window_create),
        window_close: Some(fake_window_close),
        tab_open: Some(fake_tab_open),
        set_shortcut_filter: Some(fake_set_shortcut_filter),
        window_set_page_placement: Some(fake_set_page_placement),
        window_attach_view: Some(fake_attach_view),
        window_set_state: Some(fake_set_state),
        window_create_with: Some(fake_window_create_with),
        window_set_style: Some(fake_set_style),
        tab_navigate: Some(fake_tab_navigate),
        ..Default::default()
    }))
}

static mut TABLE: *const sys::jaal_engine_api = std::ptr::null();

unsafe extern "C" fn fake_get_api(abi_major: u32, struct_size: u32) -> *const sys::jaal_engine_api {
    assert_eq!(abi_major, ABI_MAJOR);
    assert_eq!(
        struct_size as usize,
        std::mem::size_of::<sys::jaal_engine_api>()
    );
    // SAFETY: set under LOCK before each call.
    unsafe { TABLE }
}

fn engine_with(table: *const sys::jaal_engine_api) -> Result<Engine, Error> {
    // SAFETY: tests hold LOCK while TABLE is written and read.
    unsafe {
        TABLE = table;
        Engine::from_get_api(Some(fake_get_api))
    }
}

fn reset() -> std::sync::MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    RECORD.with_borrow_mut(|r| *r = Record::default());
    guard
}

#[test]
fn engine_that_refuses_the_abi_is_unsupported() {
    let _guard = reset();
    assert_eq!(
        engine_with(std::ptr::null()).err(),
        Some(Error::Unsupported)
    );
}

#[test]
fn engine_with_another_major_is_unsupported() {
    let _guard = reset();
    assert_eq!(
        engine_with(fake_table(ABI_MAJOR + 1)).err(),
        Some(Error::Unsupported)
    );
}

#[test]
fn open_tab_passes_url_and_foreground_and_returns_the_engines_handle() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();
    assert_eq!(
        window,
        Window(sys::jaal_handle {
            slot: 3,
            generation: 7
        })
    );

    let tab = engine
        .open_tab(window, "https://example.com/a?b=c", true)
        .unwrap();

    assert_eq!(
        tab,
        Tab(sys::jaal_handle {
            slot: 9,
            generation: 1
        })
    );
    RECORD.with_borrow(|r| {
        assert_eq!(r.opened_url.as_deref(), Some("https://example.com/a?b=c"));
        assert_eq!(r.opened_foreground, Some(1));
    });
}

#[test]
fn url_with_a_nul_is_invalid_and_never_reaches_the_engine() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();

    assert_eq!(
        engine.open_tab(window, "https://a\0b", false),
        Err(Error::Invalid)
    );
    RECORD.with_borrow(|r| assert_eq!(r.open_calls, 0));
}

#[test]
fn engine_status_codes_become_errors() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();

    RECORD.with_borrow_mut(|r| r.close_status = sys::jaal_status::JAAL_ERR_GONE.0);
    assert_eq!(engine.close_window(window), Err(Error::Gone));

    RECORD.with_borrow_mut(|r| r.close_status = 42);
    assert_eq!(engine.close_window(window), Err(Error::Unknown(42)));

    RECORD.with_borrow_mut(|r| r.close_status = sys::jaal_status::JAAL_OK.0);
    assert_eq!(engine.close_window(window), Ok(()));
}

#[test]
fn calls_missing_from_the_table_are_unsupported() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    assert_eq!(engine.quit(), Err(Error::Unsupported));
}

#[test]
fn events_reach_the_handler_with_their_strings_copied() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let seen = std::rc::Rc::new(RefCell::new(Vec::new()));
    let sink_seen = seen.clone();
    engine
        .on_event(move |event| sink_seen.borrow_mut().push(event))
        .unwrap();

    let (sink, user_data) =
        RECORD.with_borrow(|r| (r.sink.unwrap(), r.sink_user_data as *mut c_void));
    let url = CString::new("https://example.com/").unwrap();
    let title = CString::new("Example Domain").unwrap();
    let state = sys::jaal_event {
        struct_size: std::mem::size_of::<sys::jaal_event>() as u32,
        kind: sys::jaal_event_kind::JAAL_EVENT_TAB_STATE,
        window: sys::jaal_handle {
            slot: 3,
            generation: 7,
        },
        tab: sys::jaal_handle {
            slot: 9,
            generation: 1,
        },
        url: url.as_ptr(),
        title: title.as_ptr(),
        loading: 1,
        active: 1,
        can_go_back: 1,
        ..Default::default()
    };
    let removed = sys::jaal_event {
        kind: sys::jaal_event_kind::JAAL_EVENT_TAB_REMOVED,
        tab: sys::jaal_handle {
            slot: 9,
            generation: 1,
        },
        ..Default::default()
    };
    let future = sys::jaal_event {
        kind: sys::jaal_event_kind(77),
        ..Default::default()
    };
    // SAFETY: the events and their strings are valid for each call, as the
    // engine guarantees.
    unsafe {
        sink(user_data, &state);
        sink(user_data, &removed);
        sink(user_data, &future);
    }
    drop((url, title));

    assert_eq!(
        *seen.borrow(),
        vec![
            Event::TabState(TabState {
                window: Some(Window(sys::jaal_handle {
                    slot: 3,
                    generation: 7
                })),
                tab: Tab(sys::jaal_handle {
                    slot: 9,
                    generation: 1
                }),
                url: "https://example.com/".into(),
                title: "Example Domain".into(),
                loading: true,
                active: true,
                can_go_back: true,
                can_go_forward: false,
            }),
            Event::TabRemoved(Tab(sys::jaal_handle {
                slot: 9,
                generation: 1
            })),
            Event::Unknown(77),
        ]
    );
}

#[test]
fn a_sidebar_placement_reaches_the_engine_with_unset_fields_as_jaal_auto() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();

    // The page beside a 236 DIP sidebar, inside an 8 DIP frame.
    engine
        .set_page_placement(window, Placement::inset(236, 8, 8, 8))
        .unwrap();

    // JAAL_AUTO is INT32_MIN in jaal_engine.h.
    assert_eq!(
        RECORD.with_borrow(|r| r.page_placement),
        Some(sys::jaal_placement {
            left: 236,
            top: 8,
            right: 8,
            bottom: 8,
            width: i32::MIN,
            height: i32::MIN,
        })
    );
}

#[test]
fn attach_view_passes_the_view_and_every_option() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();
    let options = ViewOptions {
        placement: Placement {
            left: Some(236),
            right: Some(8),
            top: Some(40),
            width: Some(668),
            height: Some(400),
            bottom: None,
        },
        visible: true,
        backdrop_blur: 28.,
        backdrop_inset: 64,
        backdrop_corner_radius: 12.,
    };

    engine
        .attach_view(window, 0x1234 as *mut c_void, options)
        .unwrap();

    assert_eq!(
        RECORD.with_borrow(|r| r.attached_view),
        Some((
            0x1234,
            sys::jaal_view_options {
                struct_size: std::mem::size_of::<sys::jaal_view_options>() as u32,
                placement: sys::jaal_placement {
                    left: 236,
                    top: 40,
                    right: 8,
                    bottom: i32::MIN,
                    width: 668,
                    height: 400,
                },
                visible: 1,
                backdrop_blur: 28.,
                backdrop_inset: 64,
                backdrop_corner_radius: 12.,
            }
        ))
    );
}

#[test]
fn the_shortcut_filter_answers_the_engine_for_each_key() {
    fn ctrl_t_only(key: Key) -> bool {
        key == Key {
            code: 0x54,
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        }
    }
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();

    engine.set_shortcut_filter(ctrl_t_only).unwrap();

    let (filter, user_data) =
        RECORD.with_borrow(|r| (r.filter.unwrap(), r.filter_user_data as *mut c_void));
    // SAFETY: the engine calls the filter with the user data it was given.
    unsafe {
        assert_eq!(filter(user_data, 0x54, sys::JAAL_MOD_CTRL), 1);
        assert_eq!(filter(user_data, 0x54, 0), 0);
        assert_eq!(
            filter(user_data, 0x54, sys::JAAL_MOD_CTRL | sys::JAAL_MOD_SHIFT),
            0
        );
        assert_eq!(filter(user_data, 0x4b, sys::JAAL_MOD_CTRL), 0);
    }
}

#[test]
fn set_window_state_passes_the_state() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();

    engine
        .set_window_state(window, WindowState::Fullscreen)
        .unwrap();

    assert_eq!(
        RECORD.with_borrow(|r| r.window_state),
        Some(sys::jaal_window_state::JAAL_WINDOW_FULLSCREEN)
    );
}

#[test]
fn key_and_window_state_events_are_decoded() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let seen = std::rc::Rc::new(RefCell::new(Vec::new()));
    let sink_seen = seen.clone();
    engine
        .on_event(move |event| sink_seen.borrow_mut().push(event))
        .unwrap();
    let (sink, user_data) =
        RECORD.with_borrow(|r| (r.sink.unwrap(), r.sink_user_data as *mut c_void));
    let window = sys::jaal_handle {
        slot: 3,
        generation: 7,
    };
    let size = std::mem::size_of::<sys::jaal_event>() as u32;
    let key = sys::jaal_event {
        struct_size: size,
        kind: sys::jaal_event_kind::JAAL_EVENT_KEY,
        window,
        key_code: 0x4b,
        modifiers: sys::JAAL_MOD_CTRL | sys::JAAL_MOD_SHIFT,
        key_reserved: 1,
        ..Default::default()
    };
    let state = sys::jaal_event {
        struct_size: size,
        kind: sys::jaal_event_kind::JAAL_EVENT_WINDOW_STATE,
        window,
        window_active: 1,
        window_state: sys::jaal_window_state::JAAL_WINDOW_MAXIMIZED,
        client_width: 1440,
        client_height: 860,
        scale: 2.,
        ..Default::default()
    };
    // What a 1.0 engine would send: too short to hold the 1.1 fields.
    let short = sys::jaal_event {
        struct_size: size - 8,
        kind: sys::jaal_event_kind::JAAL_EVENT_KEY,
        ..Default::default()
    };
    // SAFETY: the events are valid for each call.
    unsafe {
        sink(user_data, &key);
        sink(user_data, &state);
        sink(user_data, &short);
    }

    assert_eq!(
        *seen.borrow(),
        vec![
            Event::Key {
                window: Some(Window(window)),
                key: Key {
                    code: 0x4b,
                    shift: true,
                    ctrl: true,
                    alt: false,
                    meta: false,
                },
                reserved: true,
            },
            Event::WindowState(WindowInfo {
                window: Window(window),
                state: WindowState::Maximized,
                active: true,
                client_width: 1440,
                client_height: 860,
                scale: 2.,
            }),
            Event::Unknown(sys::jaal_event_kind::JAAL_EVENT_KEY.0),
        ]
    );
}

#[test]
fn a_frameless_window_is_asked_for_and_its_handle_returned() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();

    let window = engine
        .create_window_with(WindowOptions { frameless: true })
        .unwrap();

    assert_eq!(
        window,
        Window(sys::jaal_handle {
            slot: 4,
            generation: 2
        })
    );
    assert_eq!(
        RECORD.with_borrow(|r| r.window_options),
        Some(sys::jaal_window_options {
            struct_size: std::mem::size_of::<sys::jaal_window_options>() as u32,
            frameless: 1,
        })
    );
}

#[test]
fn set_window_style_passes_every_color_and_size() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();

    engine
        .set_window_style(
            window,
            WindowStyle {
                background_top: 0xffed_e9e2,
                background_bottom: 0xffe4_dfd5,
                page_corner_radius: 10.,
                page_shadow_color: 0x1f3d_3526,
                page_shadow_blur: 10.,
                page_shadow_offset_y: 2.,
                page_outline_color: 0x293d_3526,
            },
        )
        .unwrap();

    assert_eq!(
        RECORD.with_borrow(|r| r.window_style),
        Some(sys::jaal_window_style {
            struct_size: std::mem::size_of::<sys::jaal_window_style>() as u32,
            background_top: 0xffed_e9e2,
            background_bottom: 0xffe4_dfd5,
            page_corner_radius: 10.,
            page_shadow_color: 0x1f3d_3526,
            page_shadow_blur: 10.,
            page_shadow_offset_y: 2.,
            page_outline_color: 0x293d_3526,
        })
    );
}

#[test]
fn navigate_passes_the_action_and_reports_a_tab_with_nowhere_to_go() {
    let _guard = reset();
    let engine = engine_with(fake_table(ABI_MAJOR)).unwrap();
    let window = engine.create_window().unwrap();
    let tab = engine
        .open_tab(window, "https://example.com/", true)
        .unwrap();

    assert_eq!(engine.navigate(tab, NavAction::Reload), Ok(()));
    assert_eq!(
        RECORD.with_borrow(|r| r.nav_action),
        Some(sys::jaal_nav_action::JAAL_NAV_RELOAD)
    );

    // What the engine answers for Back on a tab with no history.
    RECORD.with_borrow_mut(|r| r.nav_status = sys::jaal_status::JAAL_ERR_INVALID.0);
    assert_eq!(engine.navigate(tab, NavAction::Back), Err(Error::Invalid));
    assert_eq!(
        RECORD.with_borrow(|r| r.nav_action),
        Some(sys::jaal_nav_action::JAAL_NAV_BACK)
    );
}
