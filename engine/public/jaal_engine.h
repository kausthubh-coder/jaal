// jaal_engine.h: the only thing that crosses between the jaal engine and the
// app's UI library. Plain C, so the Rust side can use bindgen output and no
// C++, Objective-C or Rust types cross. Rules are in docs/engine.md › The C API.
//
// Who exports what:
// - The UI library (jaal_ui.dll, libjaal_ui.dylib) exports
//   `jaal_ui_abi_major` and `jaal_ui_main`.
// - The engine passes `jaal_engine_get_api` to `jaal_ui_main`, so the UI
//   library never links against the engine.
//
// Every call happens on the engine's UI thread, which is the main thread.
// Events are never delivered from inside a call: they are posted and arrive at
// the top of the loop.

#ifndef JAAL_ENGINE_H_
#define JAAL_ENGINE_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Bump the major version for any change an older UI library can't handle.
// The engine refuses a UI library with a different major before any profile
// opens. Minor versions only add calls at the end of the table.
#define JAAL_ENGINE_ABI_MAJOR 1
#define JAAL_ENGINE_ABI_MINOR 0

typedef enum jaal_status {
  JAAL_OK = 0,
  // The arguments are wrong: a null pointer, an unknown kind, a bad URL.
  JAAL_ERR_INVALID = 1,
  // The handle names a window or tab that has been removed.
  JAAL_ERR_GONE = 2,
  JAAL_ERR_CANCELLED = 3,
  // The engine doesn't do this yet. Never returned as an empty success.
  JAAL_ERR_UNSUPPORTED = 4,
  JAAL_ERR_DENIED = 5,
  JAAL_ERR_INTERNAL = 6,
} jaal_status;

// A window or tab. `slot` is reused after removal; `generation` changes on
// every reuse, so a stale handle returns JAAL_ERR_GONE instead of reaching a
// different window or tab. The zero handle names nothing.
typedef struct jaal_handle {
  uint32_t slot;
  uint32_t generation;
} jaal_handle;

typedef struct jaal_rect {
  int32_t x;
  int32_t y;
  int32_t width;
  int32_t height;
} jaal_rect;

typedef enum jaal_event_kind {
  // The engine has called jaal_ui_main and finished starting.
  JAAL_EVENT_STARTED = 0,
  // A quit is committed: every window passed its beforeunload checks.
  JAAL_EVENT_WILL_QUIT = 1,
  // A page's beforeunload (or a running download) cancelled the quit.
  JAAL_EVENT_QUIT_CANCELLED = 2,
  // A window is gone. `window` is already stale when this arrives.
  JAAL_EVENT_WINDOW_CLOSED = 3,
  // A tab joined a window: opened by the app, a page or an extension.
  JAAL_EVENT_TAB_INSERTED = 4,
  // A tab left its window. `tab` is already stale when this arrives.
  JAAL_EVENT_TAB_REMOVED = 5,
  // The tab's URL, title or loading state changed.
  JAAL_EVENT_TAB_STATE = 6,
} jaal_event_kind;

// One event. Fields that don't apply to `kind` are zero or null. Strings are
// UTF-8 and valid only during the callback; copy what you keep.
typedef struct jaal_event {
  uint32_t struct_size;
  jaal_event_kind kind;
  jaal_handle window;
  jaal_handle tab;
  const char* url;
  const char* title;
  uint8_t loading;
  uint8_t active;
} jaal_event;

typedef void (*jaal_event_sink)(void* user_data, const jaal_event* event);

// The engine's calls. Check `struct_size` before using a call added in a later
// minor version.
typedef struct jaal_engine_api {
  uint32_t struct_size;
  uint32_t abi_major;
  uint32_t abi_minor;

  // Engine.
  jaal_status (*set_event_sink)(jaal_event_sink sink, void* user_data);
  // Starts quitting. Every window runs its beforeunload checks first; the app
  // then gets JAAL_EVENT_WILL_QUIT or JAAL_EVENT_QUIT_CANCELLED.
  jaal_status (*quit)(void);

  // Windows. Profiles start simple: every window uses the last used profile.
  jaal_status (*window_create)(jaal_handle* out_window);
  // Asks the window to close. A page's beforeunload can refuse.
  jaal_status (*window_close)(jaal_handle window);
  // The window's HWND on Windows or NSWindow* on the Mac.
  jaal_status (*window_native_handle)(jaal_handle window, void** out_handle);
  // Where the page goes, in the window's client coordinates (DIPs). Until the
  // app sets one, the page fills the window.
  jaal_status (*window_set_page_rect)(jaal_handle window, jaal_rect rect);

  // Tabs.
  jaal_status (*tab_open)(jaal_handle window,
                          const char* url,
                          uint8_t foreground,
                          jaal_handle* out_tab);
  jaal_status (*tab_close)(jaal_handle tab);
  jaal_status (*tab_activate)(jaal_handle tab);
  jaal_status (*tab_load)(jaal_handle tab, const char* url);
} jaal_engine_api;

// Returns the call table, or null when `abi_major` isn't the engine's or
// `struct_size` is smaller than the first version's table.
typedef const jaal_engine_api* (*jaal_engine_get_api_fn)(uint32_t abi_major,
                                                         uint32_t struct_size);

// Exported by the UI library. The engine reads it before any profile opens and
// refuses to start on a mismatch.
typedef uint32_t (*jaal_ui_abi_major_fn)(void);

// Exported by the UI library. Called once on the UI thread after the browser
// has started.
typedef void (*jaal_ui_main_fn)(jaal_engine_get_api_fn get_api);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // JAAL_ENGINE_H_
