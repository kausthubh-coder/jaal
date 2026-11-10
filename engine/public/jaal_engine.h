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
#define JAAL_ENGINE_ABI_MINOR 1

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

// Marks a jaal_placement field as not set.
#define JAAL_AUTO INT32_MIN

// Where something sits in a window's client area, in DIPs, like CSS absolute
// positioning. left, top, right and bottom are distances from that edge of
// the client area; width and height are sizes. On each axis:
// - two edges: the box stretches between them;
// - one edge and a size: the box keeps that size at that edge;
// - both edges and a size: the box keeps that size, centered between them;
// - a size alone: centered in the client area.
// The engine applies it again on every resize, in the same layout pass as
// the page, so nothing waits for the app.
typedef struct jaal_placement {
  int32_t left;
  int32_t top;
  int32_t right;
  int32_t bottom;
  int32_t width;
  int32_t height;
} jaal_placement;

typedef enum jaal_window_state {
  JAAL_WINDOW_NORMAL = 0,
  JAAL_WINDOW_MINIMIZED = 1,
  JAAL_WINDOW_MAXIMIZED = 2,
  JAAL_WINDOW_FULLSCREEN = 3,
} jaal_window_state;

typedef enum jaal_nav_action {
  JAAL_NAV_BACK = 0,
  JAAL_NAV_FORWARD = 1,
  JAAL_NAV_RELOAD = 2,
  JAAL_NAV_STOP = 3,
} jaal_nav_action;

// Bits of a key's modifiers.
#define JAAL_MOD_SHIFT 1u
#define JAAL_MOD_CTRL 2u
#define JAAL_MOD_ALT 4u
#define JAAL_MOD_META 8u

// One of the app's own native views (an HWND on Windows) shown inside an
// engine window: the sidebar, the command bar. See window_attach_view.
typedef struct jaal_view_options {
  uint32_t struct_size;
  jaal_placement placement;
  uint8_t visible;
  // For a see-through view over the page: the engine blurs what is under it,
  // with this Gaussian sigma in DIPs. 0 for no blur.
  float backdrop_blur;
  // The blurred area is the view's rectangle shrunk by this many DIPs on
  // every side, with corners of this radius, so the view can draw its own
  // shadow around it.
  int32_t backdrop_inset;
  float backdrop_corner_radius;
} jaal_view_options;

// How a new window is made. See window_create_with.
typedef struct jaal_window_options {
  uint32_t struct_size;
  // 1 for a window without the system title bar: the app draws its own
  // minimize, maximize and close buttons and starts window drags with
  // window_begin_drag. The window keeps the system's resize borders, shadow,
  // rounded corners and snapping.
  uint8_t frameless;
} jaal_window_options;

// What the engine paints in a window's client area around the page. Colors
// are 0xAARRGGBB.
typedef struct jaal_window_style {
  uint32_t struct_size;
  // Behind the page and the app's views: a top-to-bottom gradient.
  uint32_t background_top;
  uint32_t background_bottom;
  // The page's corners, in DIPs.
  float page_corner_radius;
  // A soft shadow under the page and a hairline around it. A color with
  // zero alpha draws nothing.
  uint32_t page_shadow_color;
  float page_shadow_blur;
  float page_shadow_offset_y;
  uint32_t page_outline_color;
} jaal_window_style;

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
  // A key went down while a page had focus, and either the app's shortcut
  // filter claimed it (`key_reserved` is 1, the page never saw it) or the
  // page left it unhandled (`key_reserved` is 0).
  JAAL_EVENT_KEY = 7,
  // The window's size, state or activation changed. Also sent once after
  // window_create.
  JAAL_EVENT_WINDOW_STATE = 8,
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
  // Added in 1.1. Check `struct_size` before reading them from an older
  // engine.
  // JAAL_EVENT_KEY: a Windows virtual-key code and JAAL_MOD_* bits.
  uint32_t key_code;
  uint32_t modifiers;
  uint8_t key_reserved;
  // JAAL_EVENT_WINDOW_STATE: the client area in DIPs, physical pixels per
  // DIP, and whether the window is the active one.
  uint8_t window_active;
  jaal_window_state window_state;
  int32_t client_width;
  int32_t client_height;
  float scale;
  // Tab events: whether the tab has history to go back or forward to.
  uint8_t can_go_back;
  uint8_t can_go_forward;
} jaal_event;

typedef void (*jaal_event_sink)(void* user_data, const jaal_event* event);

// Answers 1 when a key down is one of the app's reserved shortcuts (Ctrl+T,
// Ctrl+W, Ctrl+L), so the page must not get it. This is the one call the
// engine makes into the app from the middle of Chrome's work, because
// Chrome's key path is synchronous. It must only look the key up: no engine
// calls, no UI work. The key then arrives as a JAAL_EVENT_KEY like every
// other event.
typedef uint8_t (*jaal_shortcut_filter)(void* user_data,
                                        uint32_t key_code,
                                        uint32_t modifiers);

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

  // Added in 1.1.
  jaal_status (*set_shortcut_filter)(jaal_shortcut_filter filter,
                                     void* user_data);
  // Where the page goes, kept as the window resizes. Replaces any
  // window_set_page_rect.
  jaal_status (*window_set_page_placement)(jaal_handle window,
                                           jaal_placement placement);
  // Shows one of the app's native views inside the window, above the page.
  // The engine makes it a child of the window, keeps it placed, and keeps
  // later views above earlier ones. The app still owns the view and destroys
  // it; the engine lets go of it when either goes away.
  jaal_status (*window_attach_view)(jaal_handle window,
                                    void* native_view,
                                    const jaal_view_options* options);
  jaal_status (*window_update_view)(jaal_handle window,
                                    void* native_view,
                                    const jaal_view_options* options);
  jaal_status (*window_detach_view)(jaal_handle window, void* native_view);
  // Gives keyboard focus back to the page, after one of the app's views had
  // it.
  jaal_status (*window_focus_page)(jaal_handle window);
  jaal_status (*window_set_state)(jaal_handle window, jaal_window_state state);
  // window_create, with options.
  jaal_status (*window_create_with)(const jaal_window_options* options,
                                    jaal_handle* out_window);
  jaal_status (*window_set_style)(jaal_handle window,
                                  const jaal_window_style* style);
  // Starts moving the window with the mouse, as a drag on a title bar does.
  // Call it from a mouse-down in one of the app's views, while the button is
  // still down.
  jaal_status (*window_begin_drag)(jaal_handle window);
  // Back, forward, reload or stop. Back and forward answer JAAL_ERR_INVALID
  // when the tab has nowhere to go.
  jaal_status (*tab_navigate)(jaal_handle tab, jaal_nav_action action);
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
