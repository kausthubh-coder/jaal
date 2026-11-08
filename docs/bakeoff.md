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

## Picking the winner

Speed and memory decide first: a variant that's clearly slower or heavier than Helium loses. If two are close, the one that looks better and is easier for agents to change wins.
