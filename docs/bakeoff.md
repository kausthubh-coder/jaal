# UI bake-off

We build the same tiny browser three ways, measure each one against plain Helium, and build jaal's UI on the winner.

All three use prebuilt CEF 156 (the `cef` crate, 156.4.0), so nobody has to build Chromium for this. CEF's extension support is partial, so none of these browsers load extensions. That doesn't matter here: the bake-off only compares UI speed, memory and looks. jaal itself will run on our own engine build.

Mac first. The laptop runs the same tests on Windows afterwards.

## The three variants

| Folder | Who draws the UI | How the command bar gets over the page |
|---|---|---|
| `bakeoff/web-ui` | HTML pages inside Chromium (CEF Views). The sidebar is one browser view, the page another | An overlay view in the same window. Chromium's compositor stacks it over the page, shadow and all |
| `bakeoff/gpui-a` | GPUI draws the window. CEF's page view sits on top of GPUI's view as a child | A second, transparent GPUI window glued over the page (fix A) |
| `bakeoff/gpui-b` | GPUI draws the window. CEF renders the page offscreen into a shared GPU texture, and GPUI draws that texture | GPUI draws it directly over the page texture (fix B). GPUI forwards every click and key to CEF |

The GPUI variants let GPUI own the main loop and run CEF through `external_message_pump`. That is the reverse of jaal's final design, where Chromium owns the loop. CEF's docs say the external pump costs a little speed, so the GPUI numbers may come out slightly worse than they would in jaal.

## What every variant has

The same features, so the numbers compare like for like:

- A sidebar with the open tabs plus 5,000 fake rows from `bakeoff/shared` (the same rows in every variant).
- A URL field, new tab (Cmd+T), close tab (Cmd+W) and switching tabs by clicking a row.
- A command bar (Cmd+K) with a blurred background and a drop shadow that overlaps the page.
- A resizable window.

## Tests

Each test runs 5 times per variant. We report the median and the spread.

| Test | How |
|---|---|
| Cold start | From launch to the first paint of a local test page |
| Memory with 10 tabs | Sum of every process's memory (RSS and footprint) after 10 tabs of the same local pages load |
| Tab switch | From the click to the new page's first frame |
| Sidebar scroll | Fling-scroll the 5,000 rows for 5 s, count dropped frames |
| Live resize | Resize the window for 5 s, count dropped frames |
| Idle CPU | Average CPU over 60 s with 10 tabs open and nothing happening |

Helium (same Chromium 156) runs the start, memory, tab switch and idle tests as the baseline. It has no 5,000-row sidebar, so it skips that test.

The test pages are local files under `bakeoff/pages`, so network speed never shows up in the numbers.

Then we put screenshots of the three side by side and judge the look.

## How marks are taken: web-ui

The marks and their log format are in `bakeoff/shared/src/bench.rs`. This is exactly when web-ui writes each one, so the GPUI variants can match it. A time taken in a renderer is `performance.timeOrigin + <time since origin>`, which is milliseconds since the Unix epoch like `now_ms()`. The renderer sends it to the browser process, which writes it with `mark_at`, so marks from both processes line up.

| Mark | When web-ui takes it |
|---|---|
| `main` | The first line of `main` in the browser process, before the CEF framework loads. Helper processes are separate binaries and never mark. |
| `first_paint` | CEF's `LoadHandler::on_load_end` for the first tab's main frame, the first time it fires. Taken in the browser process when the callback runs. |
| `tabs_loaded` | `on_load_end` has fired for the main frame of every tab that `JAAL_BENCH_TABS` opened. |
| `tab_switch_start` | The sidebar's `pointerdown` on a tab, using the event's own `timeStamp`. The switch bench calls the same JS function a click calls, with the time of that call. The IPC from the sidebar to the browser process falls inside the measured time. |
| `tab_switch_done` | The browser process shows the new tab's view and hides the old one, then runs JS in the new page: `requestAnimationFrame` inside `requestAnimationFrame`. The first callback runs as the first frame after the view became visible begins. The second runs as the next frame begins, once the first is out. The mark is the second callback's time. The trip back to the browser process doesn't count. |
| `frame` | Scroll: the sidebar page's `requestAnimationFrame` times. Each callback moves the rows 40 px and keeps its time. Resize: the active page's `requestAnimationFrame` times, because the page is the view that lays out again at every step (the sidebar stays 280 px wide). Either way the times go to the browser process in one message when the run ends, so measuring adds no IPC per frame. |

Timing of the runs:

- Scroll and resize start 1 s after both the sidebar and the first tab have loaded, so the end of startup doesn't land in the frame times. Each lasts 5 s.
- Resize: a task on CEF's UI thread sets the window width every 16 ms. The width follows a triangle wave from 900 to 1400 px and back every 2.5 s, worked out from the time since the run started, so a late task doesn't slow the sweep.
- Switch: 2 s after `tabs_loaded`, 20 switches 500 ms apart, round-robin through the sidebar's tabs starting with tab 2.
- The app quits when every run it was asked for has finished. `JAAL_BENCH_TABS` alone keeps the app open; the harness quits it by its PID.

For a GPUI variant, `first_paint` and `tab_switch_done` can be taken the same way. `on_load_end` is the same CEF callback, and the double `requestAnimationFrame` works in any CEF page that can send a message back. `frame` there is one mark per frame GPUI draws.

## Picking the winner

Speed and memory decide first: a variant that's clearly slower or heavier than Helium loses. If two are close, the one that looks better and is easier for agents to change wins.

## How marks are taken: GPUI variants

Marks go through `bakeoff/shared/src/bench.rs` and only when `JAAL_BENCH_LOG` names a file. Each variant takes them at the same moments:

| Mark | Moment | Where gpui-a takes it |
|---|---|---|
| `main` | First line of `main` in the browser process | `main.rs` |
| `first_paint` | CEF's `on_load_end` for the first tab's main frame | Inside the CEF callback, not after a hop to the UI |
| `tabs_loaded` | Every tab opened by `JAAL_BENCH_TABS` has had its main frame's `on_load_end` | Inside the CEF callback |
| `tab_switch_start` | The switch begins: the sidebar click handler, or the switch bench's step, which calls the same code. Clicking the tab that is already active marks nothing | `BrowserWindow::activate_tab` |
| `tab_switch_done` | The new page's second `requestAnimationFrame` after it became visible: the first runs as its first frame begins, the second once that frame is out. The page logs `performance.timeOrigin + performance.now()` to the console and the mark uses that time, so the trip back to the browser process doesn't count | Console handler |
| `frame` | One per UI frame, only with `JAAL_BENCH_FRAMES=1` | Each GPUI render of the main window's root view. During the scroll and resize runs that is every display-link tick |

gpui-b takes every mark at the same place as gpui-a (same function names in its `browser.rs` and `main.rs`). One difference to keep in mind when reading its tab switch numbers: in gpui-b the page's frame is a texture GPUI draws, so after `tab_switch_done` it still waits for GPUI's next render, about one display frame, before it is on screen. In gpui-a the page view puts the frame on screen itself. The mark leaves that frame out in both.

Bench modes, set by environment variables:

- `JAAL_BENCH_TABS=N`: open test pages 1 to N as tabs, tab 1 active. Mark `tabs_loaded`, then stay open until the harness quits the app.
- `JAAL_BENCH_SWITCH=1` (with `JAAL_BENCH_TABS=10`): 2 s after `tabs_loaded`, switch tabs 20 times, 500 ms apart, round robin from tab 2 (2, 3, ... 10, 1, 2, ...), then quit.
- `JAAL_BENCH_SCROLL=1`: after `first_paint`, scroll the 5,000 sidebar rows 40 px per frame for 5 s, then quit.
- `JAAL_BENCH_RESIZE=1`: after `first_paint`, sweep the window width 900 → 1400 → 900 px every 2 s for 5 s, height fixed, then quit. This sets the size from code each frame; it is not AppKit's live resize, which only a drag starts.
- `JAAL_PROFILE_DIR`: the CEF profile and cache folder. Defaults to `target/profiles/<variant>`.
- The first command-line argument, if it isn't a switch, is the URL of the first tab instead of test page 1.
