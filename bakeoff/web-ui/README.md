# web-ui

The bake-off browser whose UI is all HTML drawn by Chromium. One CEF Views window holds a sidebar browser view (`ui/sidebar.html`), one browser view per tab (only the active one visible), and the command bar (`ui/command.html`) as an overlay view. Every browser uses CEF's Alloy style, so Chrome's own toolbar and shortcuts stay out of the way.

The HTML talks to Rust through `window.cefQuery` (CEF's message router, from the `cef` crate's `wrapper`). Rust talks to the HTML with `execute_java_script`. `parse_command` in `src/lib.rs` lists every message.

## Run it

```sh
export CEF_PATH=~/.local/share/cef
~/.cargo/bin/bundle-cef-app web-ui -o target/bundle
target/bundle/web-ui.app/Contents/MacOS/web-ui [first-tab-url]
```

The app loads `ui/` and the test pages from this checkout at run time, so a bundle only runs on a machine with the repo at the same path.

The app prints its window's CGWindowID to stderr. Screenshot only this app with `swift scripts/window-shot.swift <pid> out.png`, which keeps the command bar's shadow. `screencapture -l` drops it, because the bar is a child window.

For a release bundle, run `cargo build --release -p web-ui --bins` and copy `target/release/web-ui` and `target/release/web-ui-helper` over the executables inside the bundle (the main one and the five helper apps). On macOS, `bundle-cef-app` only builds debug.

## Environment

| Variable | What it does |
|---|---|
| `JAAL_PROFILE_DIR` | Profile and cache folder. Default `target/profiles/web-ui` |
| `JAAL_BENCH_LOG`, `JAAL_BENCH_SCROLL`, `JAAL_BENCH_RESIZE`, `JAAL_BENCH_FRAMES`, `JAAL_BENCH_TABS`, `JAAL_BENCH_SWITCH` | Bench modes; see "How marks are taken" in `docs/bakeoff.md` |
| `JAAL_PRESS_KEYS` | Test aid. Presses keys once the UI is ready, as real key events sent to the app's own NSApp, e.g. `cmd+t,cmd+k,type:/tmp/a.html,enter`. `type:` takes lowercase letters, digits and unshifted punctuation |
| `JAAL_SCROLL_TO_ROW` | Test aid. Jumps the sidebar to that row, for screenshots |

## The command bar can't be see-through

CEF paints a windowed browser's background opaque. A `background_color` with alpha 0 falls back to `CefSettings.background_color`, and to white when that is transparent too (`cef_types.h`). We tried both, and the margin around the card came out solid. So CSS can't draw a shadow outside the bar's rectangle, and `backdrop-filter` only blurs the bar's own page, never the page under it.

On macOS a CEF overlay is its own borderless child NSWindow. `mac::round_overlay_with_shadow` makes that window clear, rounds its layer, and turns on the window shadow, so macOS draws the rounded corners and the shadow over the page. The page under the bar isn't blurred. `mac::make_key_window` hands the bar's window the keyboard when it opens, because CEF's `request_focus` leaves the main window key.
