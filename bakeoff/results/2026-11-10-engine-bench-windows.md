# jaal on the engine: a first benchmark on Windows, 2026-11-10

A baseline, not a comparison. These are the first numbers for jaal running on its own engine: 5 runs each on the Windows laptop, median and range. They say what this build does today. They don't say how it compares with gpui-a on CEF, Chrome or Helium, because the engine here is a development build and those are optimized ones. That comparison is M3 and M8.

## Numbers

| Test | Median | Range | Runs |
|---|---|---|---|
| Start: process created to first tab loaded | 1,557 ms | 1,453 to 1,762 ms | 5 |
| of which, until the UI library is called | 757 ms | 675 to 903 ms | 5 |
| Memory: private working set, all processes | 284 MB | 277 to 291 MB | 5 |
| Processes | 9 | 9 to 10 | 5 |
| Idle CPU, over 60 s | 3.6% of one core | 3.1 to 5.1% | 5 |
| Sidebar scroll, 5 s: frames drawn | 300 | 299 to 300 | 5 |
| Sidebar scroll: frames dropped | 1 | 1 to 1 | 5 |
| Sidebar scroll: 95th percentile frame time | 18.0 ms | 18.0 to 18.1 ms | 5 |

Run with `bun engine/scripts/bench.ts --runs 5`. The raw runs are in [2026-11-10-engine-bench-windows.json](2026-11-10-engine-bench-windows.json).

**Start moves by about 200 ms between sessions.** The same script ran about two hours earlier, before the last changes to the UI, and measured a start of 1,359 ms (1,286 to 1,543), of which 555 ms (511 to 750) passed before the UI library was called. The whole difference is in that first part, which is Chromium's own startup and runs no jaal UI code, so the UI changes didn't cause it. What did is not known: the laptop was on mains power and idle both times. Until that is explained, a start difference under about 200 ms between two sessions on this laptop means nothing. The other numbers agreed between the two sessions: memory 292 MB, idle CPU 3.5%, the same scroll result.

## What was measured

- **Machine.** The Windows laptop: AMD Radeon 780M graphics, 16 threads, 15 GB of RAM, Windows 11 build 26200, one 2880 by 1800 display at 60 Hz, on mains power. T3 Code and ChatGPT were open; the whole machine sat at 4 to 9% CPU before the run and nothing was building.
- **Build.** Chromium 156.0.8078.88 plus jaal's layer, built with `engine/args/windows-x64-dev.gn`: not an official build, so no PGO and no ThinLTO. The UI library is a Rust release build.
- **Load.** One tab on `bakeoff/pages/page-1.html` (a local file, so no network) and the 5,000 fake rows in the sidebar, as in the bake-off. Every run starts from an empty profile folder, so each start also pays for creating a profile.
- **Start** is timed from just before the script creates the process to the engine's event that the first tab finished loading. The bake-off's own start (`main` to `first_paint`) is measured inside the process and ends at the first painted frame, so the two are not the same number.
- **Memory** is the private working set of the browser process and every process under it, read 10 s after the page loaded. Shared pages are not counted.
- **Idle CPU** is the CPU time those processes used over the next 60 s.
- **Scroll** is the bake-off's scroll run: the sidebar moves its list 40 px every frame for 5 s and marks each frame it draws. At 60 Hz, 5 s is 300 frames. A gap of about two frame times counts as one dropped frame.

## Checks on the numbers

- **Did the work happen?** Every start run waited for the engine's load event, and the script fails a run whose browser has exited. Every scroll run exited with code 0 after its 5 s and wrote at least 299 frame marks.
- **Is the scroll number at a limit?** Yes: 300 frames in 5 s is the display's 60 Hz. The run shows GPUI inside Chromium's loop keeps up with the display here. It can't show how much headroom is left.
- **What limits start and idle CPU?** Not profiled, so not known. 555 ms of the start passes before the engine calls the UI library at all, which is Chromium's own startup in this build. For idle CPU, a likely part is GPUI's vsync thread, which sends every GPUI window a paint message 60 times a second whether or not anything changed; that is a guess until it is profiled.

## Not measured

- **Tab switch.** The bake-off times a click to the first frame that shows the new page. The engine has no page-shown event yet.
- **Resize.** The sidebar doesn't change size when the window's width changes, and the page's frames are Chrome's, which aren't marked.
- **gpui-a on CEF, on this laptop.** M3 measures it, so the engine has something to be within 10% of.
