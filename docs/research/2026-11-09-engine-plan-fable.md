# jaal engine plan (Claude Fable)

Written 2026-11-09 against Chromium 156.0.8078.88 at `C:\src\chromium\src`, jaal `bakeoff/all` (5906a4c), Ardia upstream (2026-11-08) and the Intel fork (2026-11-04). Every claim names a file or URL. Anything I could not check says "unverified".

## Summary

- **Approach: a layer inside `chrome/`, like Ardia, but started from Chrome's own view-less browser window.** Chromium 156 ships `WebUIBrowserWindow`, a `BrowserWindow` with no `BrowserView`, behind the `features::kWebium` flag (`chrome/browser/ui/webui_browser/`, 37 files, 5,669 lines; `webui_browser.cc:17-19`). `BrowserWindow::CreateBrowserWindow` already branches to it (`chrome/browser/ui/views/frame/browser_window_factory.cc:32-39`), and 15 files in `chrome/` already guard for "no BrowserView" through `IsWebUIBrowserEnabled()`. jaal's window is a sibling of that class, in our own folder, and we turn that flag on. Ardia had to write every one of those guards as hooks (`ardia-upstream/engine/ardiacore/apply.sh`, 1,646 lines).
- **Windows are Chrome's `views::Widget`s on both OSes. GPUI draws inside them.** This keeps every Chrome bubble, menu, dropdown, dialog and IME path working as in Chrome, because they all parent to the WebContents' top-level Views widget. GPUI gets one fork point: render into a host view the engine hands it, instead of creating the window. Pages are `views::WebView`s the engine places at rects the app gives. Overlays stay fix A: child windows.
- **Boundary: a plain C ABI (`jaal_engine.h`), one thread.** Chromium's UI thread is the process main thread, and GPUI runs on it inside Chromium's loop, so there is no cross-thread marshaling at all. Handles are 64-bit ids. Hot events are C structs. The long tail (passwords, history, bookmarks, site settings) is JSON in, JSON out, which is Ardia's convention (`engine/chromium/src/chrome/browser/ardia/public/ac_engine.h`).
- **Process: the engine owns the executable, the app is a dynamic library the engine loads** (`libjaal_ui.dylib`, `jaal_ui.dll`), on both OSes. On Windows the exe must do Chrome's sandbox and ELF work before anything else (`chrome/app/chrome_exe_main_win.cc`, `main_dll_loader_win.cc:285-286`), so the exe is Chrome's, renamed. The Mac follows the same shape so there is one model.
- **Patches: own folder, `g_jaal_*` hooks declared in one header, a quilt `series`, a `check` that proves the series reproduces the tree.** Target: under 400 changed lines in Chrome's own files, 8-12k lines of our own code for the essentials, and ungoogled-chromium patches carried unchanged (BSD-3).
- **Build: first builds non-official and local; monthly official PGO/ThinLTO builds on Depot** (needs the developer's go). The Mac mini has 29 GB free; an Ardia-style checkout plus out dir is about 45 GB (`ardia-upstream/docs/engine-build.md:403`). That is the first blocker.

## 0. What I checked and what I didn't

Checked by reading: the jaal repo (plan, bake-off rules, Mac results, the GPUI-CEF glue, gpui-a, the vendored gpui 0.2.2), Ardia's four docs, its patch series, its `apply.sh`, its public headers, its layer sizes, the Intel fork's diff, and the Chromium tree at `C:\src\chromium\src` for every Chromium claim below.

Not checked (unverified, marked where used): CEF's current extension matrix beyond its forum and the bake-off note; Brave's exact `chromium_src` rules; whether Depot can build Chromium at all; Windows child-HWND behaviour under GPUI's DirectComposition surface; Chrome's `//chrome/updater` server requirements; the ungoogled-chromium patch names for keeping MV2; the Widevine licence process; `kWebium`'s side effects outside the 15 guard files.

**The Intel fork.** The shallow clone at `ardia-intel-mac` is upstream Ardia at commit e8b8b49 (2026-11-04, "Merge PR #862"). Its `engine/` differs from upstream only by nine patches upstream added after that date (`diff -rq` of `engine/`). Nothing in its `engine/`, `docs/engine-build.md` or build scripts names `x86_64`, Intel or a second out dir; the only out dir is `Release_GN_arm64` (`docs/engine-build.md:112,116,365,383`). So the checkout we have says nothing about building for another CPU or OS. If the Intel work exists, it is on a branch we didn't clone (unverified).

## 1. The approach

### What the engine must do

From `jaal-bakeoff/docs/plan.md` ("Essentials checklist"): full extensions and the Web Store, passwords, autofill, passkeys, PDF viewer, print, DevTools, downloads with prompts, site permissions, PiP, fullscreen, profiles, spellcheck, accessibility, content blocking. Nearly all of that is code under `chrome/browser` (603 entries in `chrome/browser/extensions` alone), not under `content/`.

### Four ways, compared

| | A. Layer inside `chrome/` (Ardia) | A+. Same, built on Chrome's `WebUIBrowserWindow` (**pick**) | B. CEF Chrome runtime + patches | C. Content-layer embedder (Electron) |
|---|---|---|---|---|
| Full extensions, Web Store | Yes: Chrome's own code, Web Store page is a tab (Ardia `docs/ardiacore-parity.md:165-169`) | Yes, same | Partial. CEF: "subset of the extension APIs", and extensions only in Chrome-style Views windows ([CEF forum](https://www.magpcss.org/ceforum/viewtopic.php?t=18515), [cef#1947](https://github.com/chromiumembedded/cef/issues/1947)); jaal's own note: `docs/bakeoff.md` line 3 | No. Electron: unpacked only, a subset of APIs, Web Store is a community add-on ([electronjs.org/docs/api/extensions](https://electronjs.org/docs/api/extensions)) |
| Essentials checklist | All present; our work is the UI around them | Same, with Chrome's own no-BrowserView guards (15 files) | Present in CEF's Chrome runtime, but CEF's patches remove things Ardia had to put back: `AppController`, `--user-data-dir`, startup creator (`docs/ardiacore-spike.md:96-104`) | Rebuild passwords, autofill, permissions, PDF, print, DevTools UI, site settings |
| Windows + Mac | Yes; both OSes have `views::Widget` windows | Yes; `webui_browser/BUILD.gn:7` asserts only `!is_android` | Yes, done by CEF | Yes |
| Code we carry | Ardia today: 118 files / 18,610 lines in `//ardia/core` plus 52 files / 11,970 lines in `chrome/browser/ardia`; hooks: `apply.sh` 1,646 lines, ~77 patches (`engine/patches/series`) | Less: Chrome's modal-dialog host, exclusive-access context, stub location bar, find-bar owner, side-panel UI and Mac key handling exist upstream (`webui_browser_*.cc`) | CEF's 117 patches to Chrome (`docs/engine-build.md:634-758`) plus our patches to CEF plus our app | Our whole browser layer |
| Rebase cost per Chrome release | Ardia's practice rebase 154→155: about 6 edits (`docs/ardiacore-spike.md:169-186`); 0.19.4→0.20.1: 4 patches rewritten, 2 hooks (`docs/engine-build.md:574-589`) | Lower: Google rebases the Webium files and the guards | CEF rebases its 117 patches (weeks behind at major versions), then we rebase ours | Electron-scale team work |
| Speed, memory | Chrome's own paths; bake-off gpui-a on CEF already beat Helium: footprint 523 MB vs 674 MB, start 378 ms vs 284 ms (`bakeoff/results/2026-11-09-Mac.md`) | Same binary as A | External pump costs a little (`docs/bakeoff.md`, "GPUI variants"); otherwise Chrome's | Chrome's |
| Loop ownership | Chromium owns it, as we want (`docs/agent-brief.md:74-76`) | Same | CEF supports both | Electron owns it |
| Licence | Chromium BSD, ours | Same | CEF BSD | Electron MIT |

Also looked at: Brave (brave-core `patches/` plus `chromium_src` overrides that shadow a Chromium file by path; [brave-core README](https://cdn.jsdelivr.net/gh/brave/brave-core@master/README.md)); Vivaldi (proprietary UI over heavily patched Chromium, a few days behind each release; [forum](https://forum.vivaldi.net/post/224697)); Thorium (compiler-flag fork, [thorium.rocks](https://thorium.rocks/)); WebView2 (Edge extensions only behind `AreBrowserExtensionsEnabled`, not Chrome's store; [Microsoft Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions6)). Brave's override idea (replace a file at the same path) is useful for binary files only, which is also how Ardia uses it (`docs/engine-build.md:295-297`). We take that and nothing else from Brave.

### Why A+

1. **It is the only way that gives full extensions and the Web Store without owning the whole `chrome/browser` UI.** B and C both fail the first line of the brief.
2. **Chrome already did the hard refactor for us.** `BrowserWindow` has 74 pure virtuals (`chrome/browser/ui/browser_window.h`, count of `= 0;`), and `BrowserView` is 6,191 lines (`chrome/browser/ui/views/frame/browser_view.cc`). `WebUIBrowserWindow` implements that interface with a `views::Widget`, a `views::WebView` for contents, a tab-modal dialog host, an exclusive-access context, an extensions container and a stub location bar (`webui_browser_window.cc:150-200`). About 40 of its methods are `NOTIMPLEMENTED_LOG_ONCE()`, which is exactly what a toolbar-less window wants.
3. **Chrome's guards come free.** `browser_window_features.cc:691-826` null-checks `browser_view` at every use. Ardia needed hooks for read-anything, toasts, the pinned-tab toast, the sad tab, the storage-pressure bubble, the remoting dialog and the NTP promo (`apply.sh:133-192,429-442`; `docs/engine-build.md:48-51`). With `kWebium` on, Chrome's own `if (!webui_browser::IsWebUIBrowserEnabled())` paths handle several of these (the apply.sh hooks at lines 134 and 149 literally edit those `if`s).
4. **Rebase cost goes to Google.** Webium is Chrome's own experiment, so its interface churn is tracked by Chrome's authors each release.

Risk accepted: Webium is experimental and can move or change shape. Our window is our own class in our own folder, so a move costs include paths, and the fallback is Ardia's from-scratch `ACBrowserWindow` (which Ardia itself "ported from Chrome's viewless Webium window" for the fullscreen bubble, `docs/ardiacore-parity.md:359`).

How we use `kWebium`: our `ChromeMainDelegate` subclass enables the feature in every process. `BrowserWindow::CreateBrowserWindow` gets a 4-line hook: for `TYPE_NORMAL` (and `TYPE_POPUP`, which we turn into tabs) when `g_jaal_browser_window_factory` is set, return ours. `TYPE_DEVTOOLS` keeps Chrome's `BrowserView` window: undocked DevTools in Chrome's own bare window is fine, and it saves a whole surface. Milestone 2 audits every other `kWebium` reader for side effects (unverified today).

## 2. The boundary

### Shape

A C header, `jaal_engine.h`, exported from the engine library. Rust gets `bindgen` bindings in a `jaal-engine-sys` crate and a safe wrapper `jaal-engine`. No C++ or ObjC crosses the boundary. Ardia's lesson: its services already are plain C with one signature and JSON (`ac_engine.h`: `void (const char* profile_dir, const char* args_json, reply, void* context)`, ABI version 2), while its hot path is 403 lines of ObjC (`public/ArdiaCore.h`). We make both halves C.

Three kinds of calls:

| Kind | Form | Examples |
|---|---|---|
| Hot, typed | C functions with ids and structs | `jaal_tabs_open`, `jaal_tabs_activate`, `jaal_tabs_place`, `jaal_window_create`, navigation, zoom, find |
| Long tail | `jaal_call(domain_verb, profile, args_json, reply_cb, ctx)`, JSON object in, JSON object out, `{"error":…}` on failure | passwords, autofill, history, bookmarks, downloads list, site settings, extensions list, browsing data |
| Events | One sink: `jaal_set_event_sink(cb, ctx)`; typed events as a tagged union struct, long-tail events as `(topic, json)` | `tab_inserted`, `tab_state`, `permission_request`, `download_changed` |

### Concrete calls

| Domain | Calls | Events |
|---|---|---|
| Engine | `jaal_engine_abi_version`, `jaal_engine_quit`, `jaal_engine_set_event_sink`, `jaal_engine_set_page_script` | `engine_started`, `engine_will_quit`, `quit_cancelled` (a page's beforeunload said no) |
| Profiles | `jaal_profiles_load(dir, cb)`, `jaal_profiles_private_for(profile)`, `jaal_profiles_release`, `jaal_profiles_delete`, `jaal_profiles_clear_data(json)` | `profile_loaded`, `profile_destroyed` |
| Windows | `jaal_windows_create(profile, rect, style) -> window_id`, `jaal_windows_native(window_id) -> {NSWindow*, host NSView*} / {HWND, host HWND}`, `jaal_windows_close`, `jaal_windows_set_page_rect(window_id, rect)`, `jaal_windows_set_devtools_rect`, `jaal_windows_fullscreen(window_id, bool)` | `window_close_requested` (app answers `jaal_windows_close_reply(ok)`), `window_closed`, `window_fullscreen_changed`, `window_key_event` (pre-handle: app returns handled or not, see "Keys") |
| Tabs | `jaal_tabs_open(window, profile, url, disposition) -> tab_id`, `jaal_tabs_close(tab, now)`, `jaal_tabs_activate`, `jaal_tabs_move(tab, window, index)`, `jaal_tabs_duplicate`, `jaal_tabs_restore_closed(profile) -> tab_id`, `jaal_tabs_discard`, `jaal_tabs_set_muted`, `jaal_tabs_capture_thumbnail(cb)` | `tab_inserted{tab, opener, disposition}`, `tab_removed`, `tab_activated_by_chrome`, `tab_state{url, title, loading, progress, can_back, can_forward, security, audible, muted, favicon_id}` (coalesced per frame), `tab_crashed`, `tab_unresponsive` |
| Navigation | `jaal_nav_load`, `jaal_nav_back`, `jaal_nav_forward`, `jaal_nav_reload(hard)`, `jaal_nav_stop`, `jaal_nav_entries(tab, cb)`, `jaal_nav_go_to_offset` | `nav_committed`, `nav_error{code}`, `nav_status_text` (hovered link) |
| Find | `jaal_find_start(tab, text, forward, match_case)`, `jaal_find_next`, `jaal_find_stop(action)` | `find_result{active, total, rect}` (Chrome's `FindTabHelper` is headless, as Ardia found: `docs/ardiacore-spike.md:64`) |
| Zoom | `jaal_zoom_step(tab, +1/-1/0)`, `jaal_zoom_set(host, level)`, `jaal_zoom_list` | `zoom_changed{tab, percent}` |
| Downloads | `jaal_downloads_list(profile, cb)`, `jaal_downloads_command(id, cancel/pause/resume/keep/remove/open/show)` | `download_changed{json}`, `download_removed`, `download_danger_prompt` |
| Permissions | `jaal_permissions_resolve(request_id, allow/deny/dismiss, remember)` | `permission_request{request_id, tab, origin, types[]}`, `permission_dismissed` (one answer per prompt; Ardia's rule, `docs/ardiacore-parity.md:290`) |
| Extensions | `jaal_ext_list(profile)`, `jaal_ext_set_enabled`, `jaal_ext_uninstall`, `jaal_ext_install_unpacked(path)`, `jaal_ext_install_prompt_reply(id, accept)`, `jaal_ext_action_run(ext, tab)`, `jaal_ext_action_popup_open(ext, tab, anchor_rect)`, `jaal_ext_side_panel_open(ext, tab, rect)`, `jaal_ext_context_menu_items(tab)` | `ext_changed`, `ext_action_state{ext, tab, icon_id, badge, enabled}`, `ext_install_prompt{id, json}`, `ext_installed{ext}` (app shows the "added" bubble), `ext_side_panel_requested` |
| DevTools | `jaal_devtools_toggle(tab, dock)`, `jaal_devtools_inspect(tab, x, y)` | `devtools_dock_changed{tab, docked, has_view}` (docked: the engine lays the DevTools `WebView` into the rect from `jaal_windows_set_devtools_rect`) |
| Prompts | `jaal_prompt_reply(id, json)` | `password_save_prompt{tab, username, origin}`, `password_update_prompt`, `external_app_prompt`, `popup_blocked{tab, url}`, `beforeunload_dialog` is Chrome's (tab-modal), `file_chooser` is the OS's |
| Menus | `jaal_menu_context_show(tab, x, y)` is automatic (Chrome's own menu, see §4); `jaal_commands_execute(window, IDC_*)` runs any Chrome command (print, save page, view source, caret browsing) | `menu_command{tab, id}` for items the app adds |
| Fullscreen, PiP | `jaal_fullscreen_exit(tab)`, `jaal_pip_request(tab)`, `jaal_pip_exit(tab)` | `tab_fullscreen_changed`, `pip_changed{tab, kind, open}` |
| Media | `jaal_media_command(tab, play/pause/…)`, `jaal_media_access_reply(id, source)` | `media_state`, `now_playing`, `display_media_request` |
| Page script | `jaal_page_eval(tab, frame, js, cb)`, `jaal_page_post(tab, kind, json)` (a renderer-side client plus one mojo interface, as Ardia's `ACContentRendererClient`, `docs/ardiacore-parity.md:368-380`) | `page_message{tab, frame, kind, json}` |
| Chrome pages | none: `chrome://settings`, `chrome://extensions`, `chrome://history`, the Web Store and `chrome://downloads` open as tabs; our UI calls the typed APIs above for its own screens | |

### Threading

Everything on the UI thread, which is the process main thread on both OSes (`base/message_loop/message_pump_apple.mm:788` runs `[NSApp run]` on it; `message_pump_win.cc:538` dispatches window messages on it). GPUI runs on the same thread (§3). Rules:

1. Every `jaal_*` call must be made on the UI thread. Debug builds `CHECK` it.
2. The engine never calls the event sink from inside a `jaal_*` call. Events are posted as UI-thread tasks and arrive at the top of the loop. The bake-off had the same rule for CEF (`bakeoff/gpui-cef/src/lib.rs:24-27`). This keeps GPUI's `App` borrow rules safe: the Rust sink runs `App::update` with no GPUI frame in progress.
3. Replies to `jaal_call` run once, on the UI thread, possibly before the call returns (Ardia's rule, `docs/engine-build.md:304-307`).

### Ownership

- The engine owns every Chromium object: profiles, `Browser`s, `WebContents`, widgets, the page views. The app holds ids. An id after its `*_removed` event is invalid and calls on it return `JAAL_ERR_GONE`.
- Strings in: `(const char*, size_t)` valid for the call. Strings out: valid during the callback; the app copies. Ardia cleanses argument buffers after each call (`docs/engine-build.md:311`); we do the same, and secrets go through a separate `jaal_call_secret` whose reply buffer the engine wipes.
- Native handles from `jaal_windows_native` are borrowed: valid until `window_closed`.
- Images (favicons, thumbnails) come as `image_id` plus a `jaal_image_lock(id) -> {ptr, w, h, stride}` / `jaal_image_release` pair, so GPUI uploads without a JSON base64 round trip.

## 3. Loop and startup

### Who owns the loop

Chromium. On the Mac, `ChromeMainDelegate` registers `BrowserCrApplication` as NSApp (`chrome/app/chrome_main_delegate.cc:1939`), and `chrome_browser_application_mac.mm:40-45` `CHECK`s that NSApp is that class. The pump needs NSApp to conform to `CrAppProtocol` (`base/message_loop/message_pump_apple.h:53-57,383`). On Windows, `MessagePumpForUI::ProcessNextWindowsMessage` (`message_pump_win.cc:538`) runs `PeekMessage`/`DispatchMessage` for every window on the thread, so GPUI's `WM_GPUI_*` messages (`third_party/gpui/src/platform/windows/platform.rs:698-727`) are delivered without any change.

### Process layout

| | Mac | Windows |
|---|---|---|
| Executable | `jaal.app/Contents/MacOS/jaal`: Chrome's stub, which `dlopen`s the framework and calls `ChromeMain` (`chrome/app/chrome_exe_main_mac.cc:184-220`), renamed through the BRANDING file (`chrome/app/theme/chromium/BRANDING`) | `jaal.exe`: Chrome's stub, which sets up `install_static`, crash reporting, the sandbox broker info and loads the main DLL (`chrome_exe_main_win.cc:267-268,378-380`; `main_dll_loader_win.cc:117-119,285-286`). Child processes run the same exe with `--type=…`. Branding through `chrome/install_static/chromium_install_modes.h` |
| Engine library | `jaal Engine.framework` (`mac_framework_bundle("chrome_framework")`, `chrome/BUILD.gn:1369`) with the helper apps inside (`chrome/BUILD.gn:778-792,1041`) | `jaal_engine.dll` (`shared_library("chrome_dll")`, `chrome/BUILD.gn:422`) plus `jaal_elf.dll` |
| App library | `libjaal_ui.dylib` in `Contents/Frameworks` | `jaal_ui.dll` next to the exe |
| Who loads the app | Our `JaalMainDelegate` (a `ChromeMainDelegate` subclass made in `ChromeMain`, the same 4-line hook Ardia uses: `apply.sh:73-86`), in the browser process only, at `PostBrowserStart` | Same |

Why the app is a library and not the exe: on Windows the exe has about 400 lines of sandbox and ELF duty that must run before anything else, and it must be the binary every child process is (`chrome_exe_main_win.cc`, 388 lines). On the Mac the Rust exe route is easy (Ardia's `main.swift` does it, `docs/agent-brief.md:74`), but one model on both OSes is worth more than saving a 227-line stub. The UI developer loop is unchanged: `cargo build` writes the dylib, a script copies it into the downloaded app skeleton, run.

### How GPUI starts inside Chromium

`jaal_ui_main(const jaal_engine_api*)` runs on the UI thread after `PostBrowserStart`, posted as a task so Chrome's startup burst is over (Ardia started RN "on the loop's first idle" for the same reason, `docs/ardiacore-parity.md:353`). It calls the forked `gpui::Application::new_embedded()`, which does everything `run()` does except enter the loop (`third_party/gpui/src/platform/mac/platform.rs:474`, `windows/platform.rs:308-315`).

GPUI's dispatcher already fits: on the Mac it posts to `dispatch_get_main_queue()` (`src/platform/mac/dispatcher.rs:25,49-50`), which Chromium's CFRunLoop-based pump drains; on Windows it `PostMessageW`s to its own HWND (`src/platform/windows/dispatcher.rs:85-87`).

### The Mac app-class conflict

GPUI registers `GPUIApplication` (an `NSApplication` subclass) and `GPUIApplicationDelegate` at load time (`src/platform/mac/platform.rs:76,83`). Only one app class can exist, and Chromium's must win. The fork:

1. Delete `GPUIApplication` and the `#[ctor]` that registers it. The bake-off's runtime patch that bolted `CrAppProtocol` onto GPUI's class (`bakeoff/gpui-cef/src/nsapp.rs`) goes too: `BrowserCrApplication` already conforms.
2. Keep `GPUIApplicationDelegate`'s methods but hang them off our engine's NSApp-delegate stand-in. Chrome's `AppController` is referenced from 48 files under `chrome/browser` (count of `rg -l AppController`), so we do what Ardia does: never create it, answer `+[AppController sharedController]` with a stand-in that implements the selectors Chrome asks for, and route quit through `applicationShouldTerminate:` so beforeunload and downloads-in-progress can cancel (`docs/ardiacore-spike.md:300-301`). Our stand-in forwards `applicationDidBecomeActive:`, `openURLs:`, reopen and the dock menu to GPUI through a C callback table.
3. GPUI's per-app singleton becomes a plain global.

On Windows nothing else changes: the fork only skips `run()`.

### Startup order

1. Exe → `ChromeMain` → `JaalMainDelegate`. In every process it sets `--no-first-run`, `--no-default-browser-check`, `--no-startup-window` and enables `kWebium` (Ardia's H6 list of what an embedder skips: `docs/engine-build.md:162`).
2. Browser process: profiles load, `PostBrowserStart`, a keep-alive is taken so the process outlives its last window (Ardia: `docs/ardiacore-spike.md:102-104`).
3. `jaal_ui_main` runs: GPUI starts embedded, creates a window through `jaal_windows_create`, gets the native handles, renders into the host view, opens tabs.
4. Quit: `jaal_engine_quit` → close every Browser with beforeunload → `engine_will_quit` → the keep-alive drops → Chromium's loop ends → exit 0.

## 4. Pages and overlays

### Windows and pages

The engine creates each top-level window as a `views::Widget`, exactly as `WebUIBrowserWindow` does (`webui_browser_window.cc:157-170`). Inside it:

- On the Mac, the Views window's content view gets two subviews: a host `NSView` for GPUI (the fork renders its `CAMetalLayer` view into it) and the page area, a `views::WebView` hosted by Views' own `NativeViewHostMac` (`ui/views/controls/native/native_view_host_mac.mm`). Hit-testing must ask our host view first, which is Ardia's `BridgedContentView.ardiaEmbeddedView` patch (`docs/engine-build.md:63`), about 20 lines.
- On Windows, the Views window is one HWND whose contents Chrome's compositor paints. GPUI's fork creates its window as a `WS_CHILD` of that HWND (`src/platform/windows/window.rs:362-410` is where styles are chosen today) covering the sidebar area. Chrome itself already puts a child HWND inside its window (`content/browser/renderer_host/legacy_render_widget_host_win.cc:237`, `WS_CHILDWINDOW`), so a child HWND over a composited parent is a known shape. Unverified: whether GPUI's `WS_EX_NOREDIRECTIONBITMAP` DirectComposition surface (`window.rs:410`) works as a child HWND, and whether a system backdrop blur applies to a child. Milestone 3 tests both; the fallback is the reverse layout (GPUI owns the HWND, the page is a `TYPE_CONTROL` child widget, `ui/views/widget/widget.h:243`), which needs CEF's "external parent" patch because stock Views only takes an `aura::Window` parent (`desktop_window_tree_host_win.cc:264-282`; Ardia's CEF table: `views_widget`, `docs/engine-build.md:747`).

The app never gets a per-tab HWND or NSView. It gives rects: `jaal_windows_set_page_rect` and `jaal_windows_set_devtools_rect`. Tab switching is `views::WebView::SetWebContents` on one contents view per window, Chrome's own path. If the bake-off's 26.9 ms median switch (`bakeoff/results/2026-11-09-Mac.md`) is not met, milestone 4 tries one `WebView` per tab, shown and hidden, which is what gpui-a on CEF did (`bakeoff/gpui-cef/src/lib.rs:302-311`).

Why not the bake-off's layout (GPUI owns the NSWindow, the page is a child NSView)? Chrome's dropdowns and dialogs find their parent through `Widget::GetWidgetForNativeView` and `NativeWidgetMacNSWindowHost::GetFromNativeView(params.parent)` (`ui/views/widget/native_widget_mac.mm:277-278,306-308`). A plain GPUI `NSWindow` returns null there, so autofill, passkeys, print preview and HTTP auth would need a patch each or a rewrite. Ardia chose the Views-backed window for this reason (`docs/agent-brief.md:77-78`), and its spike proved the autofill dropdown and passkey dialog attach correctly (`docs/ardiacore-spike.md:89`). The GPUI rendering, memory and overlay model the bake-off measured do not change: GPUI still draws one Metal layer, the page is still Chrome's native view beside it, overlays are still child windows.

### Overlays (fix A)

Unchanged from gpui-a: the command bar is a transparent GPUI pop-up window added with `addChildWindow:ordered:` (`bakeoff/gpui-a/src/overlay.rs:1-7,59`), an `NSPanel` that can be key (`command_bar.rs:55-57`). On Windows it is an owned `WS_POPUP` layered window the app moves on `WM_MOVE`/`WM_SIZE` of the parent (the fork exposes those). Chrome's own bubbles are child windows too, so nothing new is being asked of either OS.

### Which Chrome UI stays and which we rebuild

| Surface | Decision | Why |
|---|---|---|
| Context menus | Chrome's | `RenderViewContextMenu` needs the WebContents and a native window, not a `BrowserView`. Ardia keeps Chrome's and only adds items (`docs/ardiacore-parity.md:154`). App items come through `ext_context_menu_items`/`menu_command` |
| Autofill and password dropdowns | Chrome's | `AutofillPopupControllerImpl` parents to the WebContents' top-level widget, which is ours |
| Tab-modal dialogs (HTTP auth, passkeys/WebAuthn, print preview, JS dialogs, FedCM) | Chrome's | They use `BrowserWindow::GetWebContentsModalDialogHost` (`browser_window.h:416`); Webium's `webui_browser_modal_dialog_host.cc` is reusable |
| File pickers, colour picker, print system dialog | OS, via Chrome | No window UI involved |
| Picture in picture | Chrome's | `VideoOverlayWindowViews` is its own widget (`chrome/browser/ui/views/overlay/video_overlay_window_views.h`). Ardia restyled it with a 6-virtual-method patch; we skip that until polish |
| DevTools docked | Chrome's contents, our layout | The engine gives the DevTools `WebView` the rect the app sets; Ardia's `chromium-window-docked-devtools.patch` shows the hook point (`docs/engine-build.md:61`) |
| DevTools undocked | Chrome's window | `TYPE_DEVTOOLS` keeps `BrowserView` (§1) |
| Fullscreen exit bubble, keyboard lock | Chrome's | Webium's `webui_browser_exclusive_access_context.cc` plus `ExclusiveAccessBubbleViews` (Ardia did the same port, `docs/ardiacore-parity.md:359`) |
| Extension action popup | Chrome's popup if it anchors to a rect, else ours | `ExtensionPopup::ShowPopup(BrowserWindowInterface*, …)` (`chrome/browser/ui/views/extensions/extension_popup.h:63`); bubbles can anchor to a rect (`ui/views/bubble/bubble_dialog_delegate_view.h:305`). Fallback: our panel holding Chrome's `ExtensionViewHost` (Ardia: `docs/ardiacore-spike.md:62`) |
| Extension install prompt | Chrome's dialog or ours, no patch | `ExtensionInstallPrompt::ShowDialogCallback` is injectable upstream (`chrome/browser/extensions/extension_install_prompt.h:54-61`). Start with Chrome's dialog parented to our window; move to GPUI for polish |
| Extension "added" bubble | Ours (GPUI) | `extension_install_ui_desktop.cc:161-162` looks up a `BrowserView`; one hook to hand us the event |
| Extension side panel | Ours (layout), Chrome's contents | Webium's `webui_browser_side_panel_ui.cc` is the model; Ardia needed `chromium-side-panel-hook.patch` |
| Permission prompts (camera, mic, location, notifications) | Ours (GPUI) | Chrome's bubble anchors to the omnibox chip. Hook at `CreatePermissionPrompt` (`chrome/browser/ui/permission_bubble/permission_prompt.h:16`, `permission_prompt_factory.cc:292`); Ardia's H3 |
| Password save and update bubble | Ours (GPUI) | Anchors to the omnibox key icon. Ardia's `chromium-password-bubble-hook.patch` |
| Find bar | Ours (GPUI) | `FindTabHelper` is headless; Webium has a `find_bar_owner` we can mirror |
| Zoom bubble, page info, bookmark bubble, translate | Ours, or none | Omnibox-anchored. Translate needs Google API keys and stays off (`google_apis/BUILD.gn:25`, empty by default) |
| Downloads bubble | Ours (GPUI) over `DownloadManager` | `GetDownloadBubbleUIController` returns null; 13 non-test call sites must tolerate it (count from `rg`; Ardia ships with null, so they do in 156 at least where Ardia reaches: unverified for all 13) |
| Status bubble (hovered URL) | Ours | `GetStatusBubbles` returns empty; Webium does the same |
| Sad tab, toasts, NTP promos, storage-pressure bubble | None or ours | Several are behind `IsWebUIBrowserEnabled()` upstream already; the rest were Ardia one-liners (`docs/engine-build.md:48-51`) |
| Tab strip, toolbar, omnibox, bookmarks bar, app menu | Ours (GPUI) | The whole point |
| Settings, extensions, history, downloads, Web Store pages | Chrome's, as tabs | They work without a BrowserView (Ardia: `docs/ardiacore-parity.md:165-169`) |

### Keys and focus

Reserved shortcuts (Cmd+T, Cmd+W, Cmd+L) must beat the page; the page must get Cmd+F if it wants it, and so on. The engine's `BrowserWindow::PreHandleKeyboardEvent` asks the app (`window_key_event` with a synchronous bool reply, the one exception to rule 2 in §2, because Chrome's key path is synchronous); unhandled keys that the page leaves come back as `window_key_event{after_page: true}`. Webium's Mac implementation (`webui_browser_window_mac.mm`, `ShouldHandleKeyboardEvent`) is the template. Ardia's S3 checks describe the exact expected behaviour (`docs/ardiacore-spike.md:302`).

## 5. Patches and upkeep

### Where our code lives

A separate repo, `jaal-engine`, because its checkout is 40 GB and its build is hours, and UI agents must never need it. Layout:

```
jaal-engine/
  chromium_version.txt          # 156.0.8078.88
  engine.json                   # abi version, artifact names
  patches/series                # quilt order, one concern per patch
  patches/ungoogled/*.patch     # carried unchanged, BSD-3, with their upstream names
  patches/jaal/*.patch          # ours: hooks and gn wiring only
  src/jaal/                     # copied to //jaal: BrowserWindow, delegates, hooks.h, C API, renderer client, mojom
  src/chrome/browser/jaal/      # copied to //chrome/browser/jaal: service calls (passwords, history, …), as Ardia's chrome/browser/ardia
  src/replace/                  # files that replace Chromium's at the same path (icons only)
  args/mac-arm64.gn args/mac-x64.gn args/win-x64.gn
  scripts/ sync, apply, check, build, package, release (bun)
  ui/jaal_engine.h              # the public header, copied into the artifact
```

Rules, each taken from Ardia with the reason it earned them:

- **Hooks are `g_jaal_*` function pointers or flags, null means stock Chrome, all declared in `//jaal/hooks.h` (and `//jaal/content_hooks.h` for files under `content/`, `components/`, `extensions/`).** Both the Chrome file and our layer include the header, so a drifted signature is a compile error, not a crash (`docs/engine-build.md:130-135`).
- **New behaviour goes in our folders; Chrome's files get hooks only.** Ardia's `chrome/browser/ardia` convention (`docs/engine-build.md:285-317`).
- **`scripts/check` proves `series` reproduces the tree**: apply the series strictly onto a captured base in a scratch dir and byte-compare every touched file (Ardia's `series.py check`, `docs/engine-build.md:327-348`). It runs in CI on every PR to `jaal-engine`. We write our own; Ardia's is GPL.
- **Binary files are replaced by path, never patched** (`docs/engine-build.md:295-297`).
- **Nothing writes into the app bundle.** Ardia found Chrome indexing declarativeNetRequest rulesets into the extension's own folder (`docs/engine-build.md:239-257`); a signed bundle must stay sealed. Component extensions load from the user data dir or ship without DNR.
- **The Safe Storage item name is chosen on day one and never changes.** Every saved password and cookie is encrypted with it (`docs/engine-build.md:43-44`).

### Privacy patches: how they are picked and kept

ungoogled-chromium's patches are quilt-format files under `patches/` with a `series`, split into `core/` and `extra/`, BSD-3 ([design.md](https://www.github.com/ungoogled-software/ungoogled-chromium/blob/master/docs/design.md), [LICENSE](https://github.com/ungoogled-software/ungoogled-chromium)). We do **not** take domain substitution (it rewrites hundreds of hosts to `qjz9zk` and breaks the Web Store and extension updates, which Ardia then had to exempt file by file: `docs/engine-build.md:477-480`) and we do **not** take binary pruning (our checkout is a git checkout, not a tarball). We pick individual patches:

| Take | Why |
|---|---|
| `disable-google-host-detection`, `disable-missing-key-warning`, `disable-domain-reliability`, `disable-network-time`, `disable-gaia`/signin, `disable-translate`, `disable-webrtc-log-uploader`, `disable-fonts-googleapis-references` (names from ungoogled's series, unverified exact spelling for this version) | Background traffic to Google that a browser without API keys cannot use anyway |
| `disable-default-extensions` (without the webstore_installer part, as Ardia: `docs/engine-build.md:88`) | No Web Store hosted app or payments app bundled |
| The patch that keeps MV2 extensions loadable (Helium reports "MV2 ... as before", `docs/engine-build.md:526-527`; exact ungoogled name unverified) | Full uBlock Origin is MV2 |
| Flags-only patches (`add-flag-to-…`) | Harmless, let power users choose |

Leave: anything that removes a user-facing feature (Safe Browsing stubs, the password manager, EV indicators, FedCM), anything touching the Web Store, extension updater or component updater (we keep Chrome's security components reaching Google, as Ardia's `chromium-component-updater-security-only.patch` argues: `docs/engine-build.md:56`). Each pick is a row in `patches/PICKS.md` with the reason and the upstream file hash, and `scripts/sync-ungoogled` diffs our copies against the ungoogled tag for the same Chromium version each release.

Helium's own patches (fingerprinting noise, HTTPS-first, the UI work) are GPL-3.0 (`docs/engine-build.md:591-595`) and are not taken.

### Rebasing to each Chrome release

Chrome stable ships every four weeks. The engine moves to the new stable's last patch release once a month, and takes point releases only for security fixes we are exposed to. Steps, all scripted:

1. `scripts/sync <version>`: fetch the tag at depth 1, `gclient sync --no-history` with PGO profiles. Ardia's note on shallow fetch stalls applies (`docs/engine-build.md:430-436`).
2. `scripts/apply`: capture base, apply `patches/ungoogled` (refreshed from ungoogled's tag for that version), then `patches/jaal`, then copy `src/`. Any hunk that needs fuzz fails the step and names the patch.
3. Fix: our code against moved interfaces (the `BrowserWindow` pure-virtual list changes by one or two each release: `docs/ardiacore-spike.md:176`), refresh patch context, `scripts/check`.
4. Build non-official on one machine, run the engine acceptance (hidden instance, §7), then official builds on Depot.
5. Tag `engine-<chromium>-<n>`, publish.

Budget: half a day for the sync and apply, half a day for breakage, measured against Ardia's data (6 edits for 154→155; two patches and two hooks rewritten for 155→156). If a release costs more than two days, the cause goes in `docs/rebase-log.md` and we look for a hook to retire.

### Target size of the diff

| What | Target | Ardia today (for scale) |
|---|---|---|
| Changed lines in Chrome's own files (hooks, gn, `ChromeMain`) | under 400 | `apply.sh` edits ~40 files; CEF-era carryover patch plus ~75 patches (`engine/patches/series`) |
| ungoogled patches carried | 10-15 files, unchanged | 3 (`docs/engine-build.md:88-90`), the rest through Helium |
| Our own code for bake-off parity (milestone 4) | 4-6k lines | spike: 3,165 lines + 21 hook lines (`docs/ardiacore-spike.md:190`) |
| Our own code for the essentials (milestone 7) | 8-12k lines | 30,580 lines across both layers after a year (118 + 52 files) |

## 6. Build and ship

### Machines

| Machine | Can it build the engine? | Use |
|---|---|---|
| Mac mini, 10 cores, 16 GB, 29 GB free, no Xcode | Not today. Checkout + out dir is about 45 GB (`docs/engine-build.md:403`); Xcode 27.x plus the Metal toolchain is needed (`docs/engine-build.md:551-553`, 840 MB extra); `mac_sdk.gni:51` expects SDK 26.5. 16 GB is tight for a ThinLTO link (unverified for 156; Ardia's machine is an M5 Pro with more memory) | Non-official incremental builds once an external SSD or 60 GB is freed. Engine acceptance runs |
| Windows laptop, 16 threads, 15 GB | A non-official build, slowly. Chromium's docs say 2-8 hours and recommend 16 GB+, 100 GB disk ([windows_build_instructions.md](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/windows_build_instructions.md)). VS 2022 (`build/vs_toolchain.py:69-72`). A plain `chrome` already builds here (brief) | Windows non-official builds, Windows acceptance |
| Depot (cloud) | Unverified; needs the developer's go | Official builds: `is_official_build=true` (ThinLTO), `chrome_pgo_phase=2` with Chrome's own profiles (`chrome/build/mac-arm.pgo.txt`, `mac.pgo.txt`, `win64.pgo.txt`, `win-arm64.pgo.txt` all exist in tree). Ardia's first official Mac build took 2.5 h on an M5 Pro, incremental relinks 7 min (`docs/engine-build.md:386`) |

Siso is the build tool Chromium now expects; Ninja support ended in 2025 ([chromium-dev](https://groups.google.com/a/chromium.org/g/chromium-dev/c/v-WOvWUtOpg/m/2HlNqp-6BgAJ)).

### GN args that matter

`is_official_build`, `chrome_pgo_phase=2`, `symbol_level=0`, `proprietary_codecs=true`, `ffmpeg_branding="Chrome"` (H.264/AAC, as Ardia: `docs/engine-build.md:13-14`), `enable_widevine=true` (off for Chromium branding by default: `third_party/widevine/cdm/widevine.gni:14-15`), `branding_file_path` to our BRANDING, `safe_browsing_mode=1` (see §8), `google_api_key` empty, `enable_rlz=false`, `enable_mdns=true` (Cast dialog), `dcheck_always_on=false`, `target_cpu` arm64 and x64 for Mac, x64 for Windows (arm64 Windows later).

### The artifact

`jaal-engine-<chromium>-<n>-<os>-<arch>.tar.zst`, published as a GitHub Release of `jaal-engine`, containing:

- Mac: `jaal.app` skeleton (exe, `jaal Engine.framework` with helpers, `Info.plist`s from `chrome/app/*-Info.plist`, entitlements from `chrome/app/*.plist`), signed with Developer ID and notarized, with an empty slot for `libjaal_ui.dylib`.
- Windows: `jaal.exe`, `jaal_engine.dll`, `jaal_elf.dll`, resource paks, locales, `jaal_ui.dll` slot, all Authenticode-signed.
- `include/jaal_engine.h`, `engine.json` (chromium version, engine revision, ABI version, sha256 per file).

The UI repo pins it in `engine.lock`; `bun scripts/fetch-engine.ts` downloads and verifies; `bun scripts/run.ts` builds the dylib, copies it in, re-signs the bundle ad hoc for local runs (the bake-off's `scripts/bundle.ts` already does ad-hoc signing). The ABI version in the header is checked at `jaal_ui_main`; a mismatch aborts with a message. Ardia fetches prebuilt engines the same way (`docs/engine-build.md:122-124`).

### Versioning and signing

- Engine version = `<chromium version>-<engine revision>`. ABI version is a separate integer, bumped only on incompatible header changes.
- Mac: Developer ID Application for the app, framework and every helper, hardened runtime, notarization. Chrome's own `chrome/installer/mac` has the signing sources and a `pkg-dmg` tool. Passkeys need the keychain access group and Bluetooth usage string (Ardia: `docs/ardiacore-spike.md:85`).
- Windows: Authenticode on every PE (exe, both DLLs, installer). Azure Trusted Signing or an EV certificate; SmartScreen reputation is earned over time (unverified timelines).

### Windows-specific needs

| Need | Plan |
|---|---|
| Sandbox | Chrome's exe does it (`main_dll_loader_win.cc:285-286`, `content/public/app/sandbox_helper_win.h`). Nothing for us, as long as the exe stays Chrome's |
| Installer | Chrome's `mini_installer` + `setup.exe` exist (`chrome/installer/mini_installer`, `chrome/installer/setup`) and are per-user by default. Default: use them with our install modes; fallback an Inno Setup or MSIX installer if their branding surface is too deep (unverified effort) |
| Updater | `//chrome/updater` is "a drop-in replacement for Google Update/Omaha/Keystone" that "can be customized by 3rd party embedders" (`chrome/updater/README.md`). It needs an Omaha-protocol server (unverified what the minimum is). Default for the first ship: our own small updater (manifest JSON, download, verify, swap on next launch) on both OSes, then evaluate `//chrome/updater`. Ardia uses Sparkle on the Mac (`docs/engine-build.md:461`) |
| Widevine | `enable_widevine=true` builds the key-system plumbing; the CDM arrives through the component updater (`chrome/browser/component_updater/widevine_cdm_component_installer.cc`). Decrypting Netflix needs Widevine's licence and VMP signing (`media/media_options.gni:261,277` is `enable_cdm_host_verification`, Mac and Windows only; [Electron's Widevine notes](https://electronjs.org/pt/docs/latest/tutorial/testing-widevine-cdm)). Apply early; it is a gate on "never worse than Chrome". Until then, Ardia's `chromium-widevine-without-cdm.patch` idea stops sites treating us as a bot (`docs/engine-build.md:97`), and Windows has no WebKit fallback |
| Default browser, protocol handlers | Chrome's `shell_integration` works with our install modes; Ardia's rule that test instances never set defaults is worth copying (`docs/engine-build.md:91`) |

## 7. Milestones

Each has a pass-or-fail check. "Hidden instance" means a scripted run that never takes focus, drives the app through the C API's test hooks and CDP, and writes a log, as Ardia's `acceptance.mjs` does (`docs/ardiacore-parity.md:139-194`).

| # | Milestone | Pass when |
|---|---|---|
| 0 | **Plain Chromium builds.** `jaal-engine` repo, `scripts/sync` at 156.0.8078.88, non-official `chrome` target on the Windows laptop (already does) and on the Mac mini once disk and Xcode are there | Both machines launch the built `chrome` at a local test page; screenshot saved. Depot builds one official Mac target if the developer okays it |
| 1 | **Library shape.** `JaalMainDelegate`, `kWebium` on, the exe/framework/dylib layout, `jaal_ui_main` in a 50-line Rust dylib that opens one window and one tab through the C API, no GPUI yet | The process tree shows Chrome's helpers; the tab paints a test page inside a window with no Chrome toolbar or tab strip; `jaal_engine_quit` exits 0 with no crash report; the hidden-instance log shows `[NSApp run]` entered from `MessagePumpNSApplication` (Mac) |
| 2 | **jaal's BrowserWindow and the tabs API.** Our `BrowserWindow` sibling of `WebUIBrowserWindow`, `WebContentsDelegate` that turns popups into tabs, keep-alive, quit flow, the `kWebium` side-effect audit, `tab_state` events, session service off for our windows | Hidden instance passes: open/close/activate 10 tabs; `window.open`, `target=_blank`, Cmd-click and a POST form all arrive as `tab_inserted` with opener and disposition; an unpacked MV3 extension's `chrome.windows.getAll` lists exactly our window with the right tabs; beforeunload cancels a close and a quit; a renderer crash does not take the browser down |
| 3 | **GPUI inside Chromium's loop.** The gpui fork: embedded start, no app class on Mac, host-view rendering on both OSes, overlay as child window, app-delegate forwarding | The bake-off sidebar with 5,000 rows renders in our window on Mac and Windows; clicks and typing work; a stack sample during scrolling shows Chromium's pump, not GPUI's `run`; the command bar overlay draws over the page with blur and shadow on both OSes (or the Windows fallback layout is chosen and documented) |
| 4 | **gpui-a on the engine, Mac.** Every bake-off feature (`docs/bakeoff.md`, "What every variant has"), the bench marks in `bakeoff/shared/src/bench.rs` fired from the engine's events, 5 runs each | Medians no worse than gpui-a on CEF by more than 10%: start ≤ 416 ms, footprint ≤ 576 MB, switch ≤ 30 ms, scroll dropped ≤ 1, resize dropped ≤ 7, idle CPU ≤ 2.3% (`bakeoff/results/2026-11-09-Mac.md`); and still better than Helium on footprint. Same machine, 5 runs, median and spread |
| 5 | **gpui-a on the engine, Windows.** Same features and marks | The same tests run on the laptop; numbers recorded; no feature missing versus the Mac build |
| 6 | **Essentials, wave 1 (Chrome's own surfaces).** Web Store install with the install prompt, action popup, side panel, uninstall; Chrome's autofill and password dropdowns; passkeys; permission prompts in GPUI; find in page; zoom; downloads list with Chrome's danger prompt; DevTools docked and undocked; PiP; tab fullscreen with Chrome's exit bubble; context menus; print preview; PDF viewer; view source | A per-item hidden-instance check plus a screenshot per surface on each OS. The Web Store check installs a real extension from `chromewebstore.google.com` and sees its popup |
| 7 | **Essentials, wave 2 (ours).** Profiles and private windows, our session restore, closed-tab restore, history and bookmarks screens over `jaal_call`, import from Chrome and Arc, site settings, clearing data, HTTPS-first (Chrome's pref), spellcheck, accessibility pass with VoiceOver and Narrator, memory saver, content blocking with uBlock Origin as a component extension | Each row of `docs/plan.md` "Essentials checklist" has a check and a screenshot, or a written reason it is deferred. Content blocking: the bake-off's ad-fixture page shows zero ad requests on the network |
| 8 | **Ship.** Signed, notarized Mac builds; signed Windows installer; the updater; monthly engine release pipeline with `scripts/check` in CI | A clean Mac and a clean Windows VM install from the download, open a page, update to the next build, and `codesign --verify --deep --strict` / `signtool verify` pass |

Milestones 0-3 can overlap: 0 and 3 do not depend on each other.

## 8. Risks and open questions, ranked

1. **Build machines.** 29 GB free on the Mac mini against a 45 GB need; 16 GB and 15 GB of RAM against ThinLTO links. Mitigation: external SSD, non-official local builds, official builds on Depot. Decide in milestone 0.
2. **`kWebium` side effects.** We rely on a Chrome experiment's flag. The 15 guard files are a gift, but other readers of `features::kWebium` may do things we don't want (unverified). Mitigation: milestone 2 audits every reader; worst case we keep our own flag and copy the guards, which is Ardia's position today.
3. **Windows child-HWND layout.** DirectComposition child surfaces and system blur on a child HWND are unverified. Mitigation: milestone 3 tests both layouts; the fallback needs one CEF-style patch.
4. **GPUI fork drift.** gpui 0.2.2 is vendored with small marked changes (`third_party/gpui/JAAL.md`). The embedded fork touches `platform/mac/platform.rs`, `platform/mac/window.rs`, `platform/windows/platform.rs`, `platform/windows/window.rs` and `app.rs`. Mitigation: keep each change behind `jaal:` markers as the vendoring already does, and try to upstream "host in an existing view" to gpui-ce.
5. **Extension surfaces that assume a toolbar.** Action popup anchors, the extensions menu, the "added" bubble, `chrome.action.openPopup` with no active tab (Ardia hit a crash there: `docs/engine-build.md:51`). Mitigation: anchor by rect; own panels where Chrome insists on a `BrowserView`.
6. **Keys, IME, accessibility across GPUI's view and Chrome's window.** Ardia could not prove IME and VoiceOver in hidden runs (`docs/ardiacore-spike.md:313-314`). Mitigation: a person at a screen for milestone 4 and 7; Narrator and VoiceOver in the checklist.
7. **Close, quit, downloads in progress.** Ardia's stage 1 list (`docs/ardiacore-spike.md:148-153,300-301`). Mitigation: copy its design: `UnloadController::RunBeforeUnloadForShutdown`, `applicationShouldTerminate:` with `NSTerminateLater`, keep-alive dropped only at `NSApplicationWillTerminateNotification`.
8. **Widevine and Safe Browsing.** Both need Google's agreement. Default: apply for Widevine at milestone 1; build Safe Browsing (`safe_browsing_mode=1`, `components/safe_browsing/buildflags.gni:12-27`) and ask Google for Safe Browsing API access; if refused, proxy as Brave does (plan.md already says to look at Brave and Vivaldi).
9. **Rebase churn in `browser.cc` and `browser_window_features.cc`.** They changed by 26 and 80 lines in one release (`docs/ardiacore-spike.md:185-186`). Our two hooks there are the ones most likely to need context refreshes each month.
10. **Licence hygiene.** Ardia is GPL-3.0; we copy none of its code, only its design and its lessons, and this plan cites the lessons so a reviewer can see that line. ungoogled patches are BSD-3 and carried verbatim with headers.

### Open questions (defaults chosen)

- Separate `jaal-engine` repo (default) or `engine/` in the jaal repo. Default: separate, because UI agents must never clone 40 GB.
- One `views::WebView` per window (default, Chrome's path) or one per tab. Milestone 4 decides on the switch number.
- Rust app as a dylib on the Mac too (default) or as the exe like Ardia. Default: dylib, one model.
- Chrome's `mini_installer` and `//chrome/updater` (evaluate) or our own installer and updater (default for the first ship).
- MV2 kept through an ungoogled patch (default) so full uBlock Origin runs; revisit when uBO Lite is good enough.
- Sync: Ardia's encrypted files in the user's cloud folder (plan.md) is out of scope for the engine; the engine needs only `jaal_call` readers for passwords (with wiping, as `ac_passwords_logins`), bookmarks, history and extensions.

### Where we'd be better than Ardia

- **Two OSes from one design.** Ardia is Mac-only and its Intel fork, as cloned, shows no second architecture.
- **Chrome's view-less window instead of a hand-written one**, so Chrome's authors carry the `BrowserWindow` churn and the no-BrowserView guards.
- **One thread, no bridge.** Ardia crosses RN's JS thread with JSON and coalesces events per frame (`docs/ardiacore-parity.md:223-225`). GPUI runs on the UI thread, so a `tab_state` is a struct handed over in place.
- **Plain Chromium.** Ardia carries about 12 `chromium-helium-*` patches just to undo Helium (`docs/engine-build.md:501-518`) and a licence it did not choose.
- **A C header both sides compile against**, with an ABI number, instead of `dlsym` by name and ObjC classes.
- **`views::WebView` for pages**, Chrome's own hosting path, instead of raw NSView placement that cost Ardia the `keep-old-frame-on-swap` patch (`docs/engine-build.md:77`).

### Where Ardia's lessons save us time

Each of these is a day or more we don't spend finding it:

- What an embedder must skip in `ChromeMainDelegate`, `ChromeBrowserMainParts` and the Mac parts (H6, `docs/engine-build.md:162`), and the switches to pass (`--no-first-run`, `--no-default-browser-check`, `--no-startup-window`).
- The `AppController` stand-in and the quit flow (`docs/ardiacore-spike.md:300-301`); `[NSApp terminate:]` from the Dock and logout.
- Popups as tabs through `BrowserWebContentsDelegate::AddNewContents` and `OpenURLFromTab`, keeping Chrome's `WebContents` and POST bodies (`docs/ardiacore-spike.md:49-54`).
- The Browser-constructor hook so `chrome.windows.create` and incognito land in our windows (`docs/ardiacore-spike.md:304`), and hiding our own utility windows from `chrome.windows` (`docs/engine-build.md:28`).
- One answer per permission prompt, and leaving "ask macOS" steps to Chrome (`docs/ardiacore-parity.md:290`).
- Holding a new profile's navigations until the content blocker's rulesets are in force (`docs/ardiacore-parity.md:40`).
- Chrome's `SessionService` must skip our windows or it restores ghosts after a crash (`docs/engine-build.md:85`).
- Nothing writes into the bundle; DNR indexes go next to the extension (`docs/engine-build.md:239-257`).
- Test hygiene: hidden instances that never take focus, `--use-mock-keychain`, fake media devices, `--disable-modal-animations` because a locked screen never advances `NSAnimation` (`docs/ardiacore-parity.md:430-437`), and a `series check` that runs nightly.
- The list of Chrome surfaces that crash without a `BrowserView` (storage pressure, remoting dialog, NTP promo, `openPopup` without a tab) so we test them on day one instead of in production (`docs/engine-build.md:48-51`).
