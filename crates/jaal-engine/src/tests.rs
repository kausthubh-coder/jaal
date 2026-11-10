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

fn fake_table(abi_major: u32) -> &'static sys::jaal_engine_api {
    Box::leak(Box::new(sys::jaal_engine_api {
        struct_size: std::mem::size_of::<sys::jaal_engine_api>() as u32,
        abi_major,
        abi_minor: 0,
        set_event_sink: Some(fake_set_event_sink),
        window_create: Some(fake_window_create),
        window_close: Some(fake_window_close),
        tab_open: Some(fake_tab_open),
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
            }),
            Event::TabRemoved(Tab(sys::jaal_handle {
                slot: 9,
                generation: 1
            })),
            Event::Unknown(77),
        ]
    );
}
