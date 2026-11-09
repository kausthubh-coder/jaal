# Engine plan

The engine is Chromium plus our patches, built as a library that gpui-a hosts instead of CEF. This is the plan for it. Nothing here is built yet.

It merges two plans written separately on 2026-11-09 from the same brief, one by Claude Fable and one by GPT Astra: [Fable's](research/2026-11-09-engine-plan-fable.md) and [Astra's](research/2026-11-09-engine-plan-astra.md). Those two files carry the file and line citations behind each choice. Where the two plans disagreed, the last section says what we picked and why. Chromium paths below are from 156.0.8078.88.

## Summary

- Our code is a layer inside Chromium's `chrome/` browser layer, the way Ardia does it. Our window class reuses the code of Chrome's own toolbar-less window, `WebUIBrowserWindow` (`chrome/browser/ui/webui_browser/`). We leave its `kWebium` feature flag off (`chrome/common/chrome_features.cc:1941`) and hook the one check Chrome's code asks instead (see below).
- Chrome's code keeps doing everything a browser does: extensions and the Web Store, passwords, autofill, downloads, permissions, DevTools, PDF, print. We replace the window UI and nothing else.
- Each top-level window is a Chrome `views::Widget`. GPUI draws the sidebar inside it, each page is a `views::WebView` next to it, and the command bar and other overlays stay see-through child windows (fix A).
- The app talks to the engine through one C header, `jaal_engine.h`, wrapped by two Rust crates. Every call runs on Chromium's UI thread, which is the main thread. GPUI runs on that thread inside Chromium's loop.
- The executable is Chrome's own launcher, renamed. The Rust UI is a library (`jaal_ui.dll`, `libjaal_ui.dylib`) that the engine loads in the browser process.
- Engine work happens on the Windows laptop, Windows first. Mac builds run on Depot, because Chromium can't build a Mac version on Windows.
- How often we rebase onto new Chrome versions is decided when jaal is first published.

## Why a layer inside `chrome/`

| Way | Full extensions and Web Store | Upkeep | Verdict |
|---|---|---|---|
| Layer inside `chrome/`, starting from `WebUIBrowserWindow` | Yes, it is Chrome's own extension code | Our hooks and our folder. Google maintains the toolbar-less window and the guards around it | **Pick** |
| Layer inside `chrome/`, our own window from scratch (Ardia) | Yes | Ardia carries about 40 hooked files and 77 patches, many for spots Webium already guards | Fallback if Webium changes shape |
| CEF in Chrome style, plus our patches | CEF says Chrome style has full Chrome features. Not proven for our window layout | Three layers to rebase each release: Chromium, CEF's own patches to Chrome, ours | Keep only as the comparison build |
| Content-layer embedder (Electron) | No. Electron loads unpacked extensions and supports a subset of the APIs | We would rebuild passwords, autofill, permissions, PDF and the rest | No |
| WebView2 | Edge extensions only, and Windows only | Not ours to change | No |

Webium matters because `BrowserWindow` has 74 pure virtual methods and Chrome's own `BrowserView` is 6,191 lines. `WebUIBrowserWindow` already implements that interface with a widget, a web view, a tab-modal dialog host, an exclusive-access context (fullscreen and pointer lock) and a stub location bar. Fifteen files in `chrome/` already skip `BrowserView` work when Webium is on. Ardia had to write a hook for each of these spots by hand.

We don't turn the flag on, because it does more than skip `BrowserView` work:

- It adds `WebUIBrowserRendererExtension` to every frame of every page (`chrome/renderer/chrome_content_renderer_client.cc:848`). That is code in every renderer we don't need.
- It turns off Chrome's sad-tab page (`chrome/browser/ui/tabs/tab_features.cc:646`).
- It makes Chrome create a `WebUIBrowserWindow` for every normal window (`chrome/browser/ui/views/frame/browser_window_factory.cc:35`), which we replace anyway.

The browser-side guards all ask one function, `webui_browser::IsWebUIBrowserEnabled()` (`chrome/browser/ui/webui_browser/webui_browser.cc:17`). One hook makes it return true in jaal's browser process while the flag stays off, so we get the guards without the renderer add-on. We show our own crash page from the `tab_crashed` event.

Webium is an experiment, so Google can move or reshape it. Our window is our own class in our own folder, and it copies or reuses Webium's helper classes (modal dialog host, exclusive-access context, find-bar owner). A move costs us include paths. Milestone M2 audits every caller of `IsWebUIBrowserEnabled()` for side effects we don't want.

## Process and startup

| | Mac | Windows |
|---|---|---|
| Executable | `jaal.app/Contents/MacOS/jaal`, Chrome's stub that loads the framework and calls `ChromeMain` | `jaal.exe`, Chrome's stub. It sets up crash reporting, `chrome_elf` and the sandbox before loading the main DLL. Child processes run the same exe |
| Engine library | `jaal Engine.framework`, with Chrome's helper apps inside | `jaal_engine.dll` and `jaal_elf.dll` |
| UI library | `libjaal_ui.dylib` | `jaal_ui.dll` |

The UI is a library because on Windows the executable has to be Chrome's: it does the sandbox and `chrome_elf` setup before anything else (`chrome/app/chrome_exe_main_win.cc`, `main_dll_loader_win.cc`). The Mac uses the same layout so there is one model. UI work still never needs a Chromium build: `cargo build` writes the library, and a script copies it into a downloaded engine.

Startup:

1. The exe calls `ChromeMain`, which creates our `JaalMainDelegate`, a subclass of Chrome's `ChromeMainDelegate`. It sets the `IsWebUIBrowserEnabled()` hook and passes `--no-first-run`, `--no-default-browser-check` and `--no-startup-window`.
2. The browser process loads profiles and takes a keep-alive, so it outlives its last window.
3. After `PostBrowserStart`, the engine loads the UI library from a fixed path inside the bundle, checks its ABI version, and posts a task that calls `jaal_ui_main(api)`. Helper processes never load it.
4. `jaal_ui_main` starts GPUI in embedded mode, asks the engine for a window, and opens tabs.

GPUI needs an embedded mode in our fork (`third_party/gpui`, recorded in `JAAL.md`):

- On both OSes, setup is split from the loop. GPUI's Windows `run()` also starts its vsync thread before `GetMessageW` (`third_party/gpui/src/platform/windows/platform.rs:308`), so embedded start has to do that setup without entering the loop.
- On the Mac, Chromium registers `BrowserCrApplication` as the app class and checks it. Our fork deletes GPUIApplication and the code that registers it. GPUI's per-app pointer becomes a plain global. Activation, open-URL, reopen and quit reach GPUI through a stand-in for Chrome's `AppController`, which Ardia also needed.
- On Windows, Chromium's message pump already delivers GPUI's window messages. Nothing else changes.
- The bake-off's `pump.rs`, the `nsapp.rs` protocol patch and the CEF loader are not used by the engine build.

Quitting has one path for the close button, Cmd+Q or Ctrl+Q, the Dock, OS logout and the API. A page's beforeunload or a running download can cancel it. The keep-alive drops only after the quit is committed.

## Windows, pages and overlays

- **Mac.** The window is a Views `NSWindow`. Its content view holds GPUI's Metal view and the page area, which Views hosts with `NativeViewHostMac`. Hit-testing asks GPUI's view first. Ardia needed about 20 lines for that.
- **Windows.** The window is a Views HWND. GPUI renders into a `WS_CHILD` HWND inside it. A page is an Aura window inside Views, not an HWND (`ui/gfx/native_ui_types.h:123`), so the app never gets a page handle.
- **Pages.** The app gives rectangles: the page rect and the docked DevTools rect. A tab switch is `views::WebView::SetWebContents` on one web view per window, Chrome's own path. If that is slower than the bake-off's 26.9 ms switch, M3 tries one web view per tab.

This changes one thing from the bake-off, where GPUI created the window and the page was a child view in it. Chrome's dropdowns and dialogs find their parent through Views (`ui/views/widget/native_widget_mac.mm:277-308`). A window GPUI created gives them nothing to attach to, so autofill, passkeys, print preview and HTTP auth would each need a patch. Ardia picked a Views window for the same reason. The rest of what the bake-off measured stays: GPUI draws one Metal or DirectX surface, the page is Chrome's native view beside it, and overlays are child windows.

Two things are unverified on Windows: whether GPUI's DirectComposition surface works in a child HWND, and whether system blur applies to a child window. M2 tests both first. The fallback is the bake-off layout, where GPUI owns the HWND and the page is a child Views widget. It needs one patch like CEF's "external parent" patch, because stock Views only accepts an Aura parent.

Overlays stay fix A. On the Mac the command bar is a child `NSPanel`. On Windows it is an owned, layered `WS_POPUP` that follows the main window's `WM_MOVE` and `WM_SIZE` and has no taskbar entry. A security prompt closes any GPUI overlay that would cover it.

Keys: the engine asks the app first about reserved shortcuts (Cmd+T, Cmd+W, Cmd+L and their Ctrl versions) through `BrowserWindow::PreHandleKeyboardEvent`. Everything else goes to the page first, and keys the page leaves come back to the app.

### Who draws what

| Surface | Drawn by |
|---|---|
| Sidebar, URL field, command bar, browser menu | GPUI |
| Context menus, select dropdowns, date and color pickers | Chrome. App menu items come through the API |
| Autofill and password dropdowns, passkeys, FedCM | Chrome |
| Tab-modal dialogs: HTTP auth, print preview, JavaScript dialogs, beforeunload | Chrome, using Webium's modal dialog host |
| File pickers, print dialog | The OS, through Chrome |
| Picture in picture | Chrome's own window |
| Fullscreen exit notice, keyboard lock | Chrome, through Webium's exclusive-access context |
| DevTools | Chrome's front end. Docked in our rect, or in Chrome's own window when undocked |
| Extension action popup | Chrome's extension view, anchored to a rect we give |
| Extension side panel | Chrome's extension view, laid out by GPUI |
| Extension install prompt | Chrome's dialog first (its show callback can be replaced upstream, `extension_install_prompt.h:54-61`), then GPUI |
| Extension "added" notice | GPUI |
| Site permission prompts | GPUI, hooked at `CreatePermissionPrompt` |
| Password save and update | GPUI over Chrome's password controller |
| Find bar | GPUI. Chrome's `FindTabHelper` does the finding |
| Downloads list and progress | GPUI over `DownloadManager`. Danger verdicts stay Chrome's |
| Zoom, page info, bookmark star | GPUI, or nothing |
| Settings, extensions, history, downloads pages, Web Store | Chrome pages, opened as tabs |

## The C API

`jaal_engine.h` is the only thing that crosses between engine and app. `jaal_engine_get_api(abi_major, struct_size)` returns a table of functions, and a mismatch stops the app before any profile opens. Rust gets bindgen bindings in `jaal-engine-sys` and a safe wrapper in `jaal-engine`. No C++, Objective-C or Rust types cross.

Rules:

1. Every call happens on the UI thread. Debug builds check it.
2. The engine never calls the app back from inside a call. Replies and events are posted as tasks and arrive at the top of the loop, so GPUI never sees a callback while it is in the middle of an update. The one exception is the reserved-shortcut question above, because Chrome's key path is synchronous.
3. Handles are a slot plus a generation. A handle used after its `*_removed` event returns `JAAL_ERR_GONE`. Chrome's tab and window IDs travel as separate fields for extension-facing features.
4. Calls made every frame or every tab switch take C structs. Rare calls (passwords, history, bookmarks, site settings) take JSON in and return JSON out, with an `error` field on failure.
5. Strings passed in are borrowed for the call. Strings passed out are valid during the callback, and Rust copies what it keeps. Secrets go through a separate call whose buffers the engine wipes. Favicons and thumbnails come as image IDs with lock and release calls, not base64.
6. Chrome owns the one tab model. Popups, tabs made by extensions and restored tabs arrive as `tab_inserted` events with their opener and disposition. The app never recreates a tab by loading its URL, which would lose POST bodies and opener links.
7. Results have a status: ok, invalid, gone, cancelled, unsupported, denied, internal. An unfinished feature returns unsupported, never an empty success.

| Domain | Calls (examples) | Events (examples) |
|---|---|---|
| Engine | `abi_version`, `quit`, `set_event_sink` | `started`, `will_quit`, `quit_cancelled` |
| Profiles | `load`, `open_private`, `release`, `delete`, `clear_data` | `profile_loaded`, `profile_destroyed` |
| Windows | `create`, `native_handles`, `set_page_rect`, `set_devtools_rect`, `fullscreen`, `close` | `close_requested`, `closed`, `fullscreen_changed`, `key_event` |
| Tabs | `open`, `close`, `activate`, `move`, `duplicate`, `restore_closed`, `discard`, `set_muted`, `thumbnail` | `tab_inserted`, `tab_removed`, `tab_state` (URL, title, loading, security, audio), `tab_crashed` |
| Navigation | `load`, `back`, `forward`, `reload`, `stop`, `entries`, `suggest` | `nav_committed`, `nav_error`, `status_text` |
| Find and zoom | `find_start`, `find_next`, `find_stop`, `zoom_step`, `zoom_set` | `find_result`, `zoom_changed` |
| Downloads | `list`, `pause`, `resume`, `cancel`, `open`, `reveal`, `remove`, `resolve_warning` | `download_changed`, `download_removed` |
| Permissions and sites | `permission_resolve`, `site_settings_get`, `site_settings_set`, `clear_site_data` | `permission_request`, `permission_invalidated` |
| Extensions | `list`, `set_enabled`, `uninstall`, `install_unpacked`, `install_prompt_reply`, `action_run`, `action_popup_open`, `side_panel_open` | `ext_changed`, `ext_action_state`, `ext_install_prompt`, `ext_installed` |
| DevTools | `toggle`, `inspect`, `set_dock` | `devtools_dock_changed` |
| Prompts | `prompt_reply` | `password_save_prompt`, `external_app_prompt`, `popup_blocked` |
| Menus and commands | `execute` (any Chrome command: print, save page, view source) | `menu_command` |
| Fullscreen, PiP, media | `fullscreen_exit`, `pip_request`, `media_command` | `pip_changed`, `media_state` |
| Data | `history_query`, `bookmarks_*`, `import_*`, `passwords_*`, `autofill_*`, `settings_*` | change events |

Profiles start simple: one profile per window, with private windows separate.

## Where the code lives

The engine's source goes in this repo under `engine/`. The Chromium checkout stays outside the repo (`C:\src\chromium` on the laptop), and scripts recreate it from the lock file. One PR can change the C header and the Rust wrapper together, and UI agents never need the checkout.

```
engine/
  chromium.lock            Chromium version, DEPS hash, toolchain, PGO inputs
  public/jaal_engine.h     the C API and its ABI version
  src/jaal/                our window class, delegates, hooks.h, C API, renderer client
  src/chrome/browser/jaal/ service calls: passwords, history, downloads, extensions
  patches/series           application order
  patches/jaal/            our hooks and GN wiring only
  patches/privacy/         picked ungoogled-chromium patches, unchanged
  patches/PICKS.md         why each privacy patch is in, its upstream hash and licence
  args/                    GN args per OS, CPU and build kind
  scripts/                 sync, apply, check, build, package (bun)
crates/jaal-engine-sys/    bindgen output
crates/jaal-engine/        safe Rust API
```

Rules:

- Hooks are `g_jaal_*` function pointers declared in one header, `hooks.h`. A null hook means stock Chrome. Chrome's file and our layer both include the header, so a changed signature fails to compile.
- New behaviour lives in our folders. Chrome's own files get hooks and nothing more.
- One ordered patch series. A patch that needs fuzz to apply fails the step.
- `scripts/check` applies the series to a clean copy and byte-compares every touched file with the tree we built. CI runs it on every engine PR.
- Binary files are replaced by path, never patched.
- Nothing writes into the signed app bundle at run time. Ardia found Chrome writing extension rule indexes into the extension's own folder.
- The Safe Storage keychain name is chosen once and never changed, because every saved password and cookie is encrypted with it.
- Size: under 400 changed lines in Chrome's own files at bake-off parity. Over 30 files or 1,500 lines triggers a design review. Our own code is reported separately.

### Privacy patches

The library comes up on unmodified Chromium first. Then privacy patches go in one concern at a time, so each failure has one cause.

| | Patches (exact names checked at import) |
|---|---|
| Take | Domain reliability uploads off, WebRTC log uploads off, network time off, Google host detection off, missing-API-key warning off, default bundled extensions off (keeping the Web Store installer), Manifest V2 support kept for full uBlock Origin |
| Reject | Domain substitution (breaks the Web Store and extension updates), `disable-webstore-urls`, `disable-gcm`, download quarantine off, Safe Browsing off, binary pruning |
| Never | Helium's own patches. They are GPL-3.0 |

Every pick has a row in `PICKS.md`: upstream commit and hash, licence, reason, what it removes, and when we'd drop it.

### Keeping up with Chrome

Chrome ships a new milestone to stable every two weeks and refreshes stable weekly. An extended stable channel on Windows and Mac moves every eight weeks instead (`docs/process/release_cycle.md:8-25` in the Chromium tree). A weekly refresh stays on the same milestone, so our patches should apply as they are. A new milestone is the real work.

We pick the cadence when jaal is first published, with Depot doing the builds. Until then the engine stays on one milestone. M9 rehearses a full rebase so we know what one costs before choosing.

## Build and ship

| Machine | Role |
|---|---|
| Windows laptop | Where engine work happens. Builds and tests Windows. Plain Chromium 156 is building there now (step M0). 16 threads, 15 GB RAM, about 220 GB free after the 2026-11-09 cleanup |
| Depot | Builds Mac, because Chromium's Mac build needs macOS and Xcode. Also official builds (ThinLTO plus PGO) for both OSes |
| Mac mini | Runs and tests the Mac builds Depot makes. It doesn't build Chromium |

Two build recipes:

- Development: non-official, symbols off, fast relinks.
- Release: `is_official_build=true`, `chrome_pgo_phase=2` with Chrome's own profiles, `proprietary_codecs=true`, `ffmpeg_branding="Chrome"`, `enable_widevine=true`, our branding file, no Google API keys, symbols in a separate archive.

Each build is published as `jaal-engine-<chromium>-<n>-<os>-<arch>`. Each archive has the exe, engine library, helpers and resources, an empty slot for the UI library, `jaal_engine.h`, and a manifest with versions and a hash per file. The UI side pins one archive in `engine.lock`, and a bun script downloads it, checks the hashes, drops in the UI library, and ad-hoc signs it for local runs.

First targets are `macos-arm64` and `windows-x64`. Intel Macs come later as their own target and test run. The ardia-intel-mac fork doesn't help with that: its README still says "Apple Silicon only" and its build sets `target_cpu="arm64"`.

Signing and installers:

- Mac: Developer ID for the app, framework and every helper, hardened runtime, notarization. The keychain identity is fixed before real user data exists.
- Windows: Authenticode on every executable and the installer. Chrome's exe keeps the sandbox, so `--no-sandbox` never ships. Chrome's `mini_installer` is the starting point, with our own small updater for the first release.

Features that need someone else's permission are tracked one by one, and each blocks its own "as good as Chrome" claim until it passes:

- **Widevine.** Apply early, because the licence and VMP signing take time.
- **Safe Browsing.** Keep the plumbing and get API access.
- **Translate.**
- **Cast.**
- **Push notifications.**
- **Sync.** We build our own. Google closed Chrome Sync to other browsers.

## Milestones

| | Milestone | Passes when |
|---|---|---|
| M0 | Plain Chromium builds | **Windows passed 2026-11-09** ([results](../bakeoff/results/2026-11-09-m0-windows.md)): the laptop's `chrome` build opens a local test page with the sandbox on and exits cleanly, with a screenshot. Still open: Depot builds the same for Mac, and it runs on the Mac mini |
| M1 | Library bootstrap, on Windows | `JaalMainDelegate`, the `IsWebUIBrowserEnabled()` hook, the exe and engine and UI library layout, and a 50-line Rust UI library that opens one window and one tab through the C API. The window has no Chrome toolbar or tab strip, helper processes never load the UI library, a wrong ABI is refused before any profile opens, and quit and cancelled quit both work |
| M2 | GPUI inside the engine on both OSes | The 5,000-row sidebar renders in our window on Mac and Windows. Page input, IME, shortcuts, a file picker and a tab-modal dialog work. The command bar draws over the page with blur and shadow. Moving, resizing, minimizing, fullscreen and mixed DPI keep everything in place. A stack sample shows Chromium's pump, not GPUI's loop. If Windows fails here, we switch to the fallback layout and say so |
| M3 | gpui-a on the engine | Every bake-off feature works on Windows, then Mac. Over 5 runs on the same machine, medians are within 10% of gpui-a on CEF. On the Mac that means start ≤ 416 ms, footprint ≤ 576 MB, switch ≤ 30 ms, scroll dropped ≤ 1, resize dropped ≤ 7, idle CPU ≤ 2.3% (from `bakeoff/results/2026-11-09-Mac.md`). Windows has no CEF numbers yet, so the laptop measures gpui-a on CEF first (branch `windows/gpui-a`). The CEF build stays as a comparison |
| M4 | Lifecycle | `window.open`, `target=_blank`, Cmd-click and a POST form all arrive as tabs with their opener. Beforeunload cancels a close and a quit. A renderer crash leaves the browser up. Session restore and private windows work, and an extension's `chrome.windows.getAll` lists exactly our windows and tabs |
| M5 | Extensions | See "Extensions that must work" below. Every check passes on Windows and Mac |
| M6 | Essentials | Every row of the essentials checklist in `plan.md` has a check and a screenshot per OS, or a written reason it waits |
| M7 | Privacy and content blocking | The privacy series replays without fuzz. An idle profile sends no unwanted Google traffic. uBlock Origin matches Helium on a blocking test set |
| M8 | Performance | Release builds against gpui-a on CEF, Chrome and Helium. 5 runs each, median and spread, same machine, nothing else heavy running |
| M9 | Rebase rehearsal | Move to the next Chrome milestone using only committed scripts, and record the hours it took |

After M9, `plan.md`'s order continues: good UX, ship, fix bugs, then AI.

## Extensions that must work

uBlock Origin and 1Password must work exactly as they do in Chrome, and so must most popular extensions. Each line below is an M5 check on Windows and Mac.

- **uBlock Origin, the full version.** It is a Manifest V2 extension, and Chrome has turned Manifest V2 off. One ungoogled-chromium patch keeps it on. Passes when it installs from the Web Store, blocks every ad request on a local ad test page, and its popup and dashboard open.
- **1Password, with the desktop app.** The extension alone signs in and fills like in Chrome. Unlocking through the desktop app uses native messaging, which needs three things Ardia already found:
  - Chromium only looks for native messaging host files in its own folders, where 1Password doesn't put one. At launch, the engine copies the installed password managers' Chrome host files into jaal's folder, as Ardia's `ArdiaNativeMessaging.mm` does.
  - 1Password's helper checks the signature of the browser that started it. On the Mac the user adds jaal once in 1Password › Settings › Browser › Add Browser, and the app must be in Applications. The Windows behaviour is unverified.
  - After an unlock, 1Password reopens its popup with `chrome.action.openPopup()`. That call failed in Ardia with "Browser window has no toolbar" until Ardia fixed it, so it gets its own test.

  Passes when the desktop app unlocks the extension, a login fills on a test page, and the popup comes back after the unlock.
- **Popular extensions.** Bitwarden, Dark Reader, Grammarly, React DevTools, Return YouTube Dislike, SponsorBlock, Google Translate, Honey, Loom and Wappalyzer cover popups, side panels, content scripts, DevTools panels, context menus and native messaging. Passes when each installs from the Web Store and does its main job on a test page. The list grows with every extension a user reports.

## Risks, worst first

1. **Laptop build time.** The laptop has 15 GB of RAM, and the first full build takes hours. Incremental builds after it should take minutes, but every engine change waits on them. Depot takes the official and Mac builds.
2. **Webium side effects.** We reuse code from a Chrome experiment and answer its check for our windows. M2 audits every caller. If Google removes Webium, we keep our copies of its helper classes and add the guards ourselves, which is where Ardia is today.
3. **Windows layout.** GPUI's DirectComposition surface and blur in a child HWND are untested. M2 decides, and the fallback costs one patch.
4. **External services.** Widevine, Safe Browsing, Translate and Cast each need Google's or a licensor's agreement. Until then, "never worse than Chrome" is false for that feature, and we say so.
5. **Chrome surfaces that expect a toolbar.** Extension popups with no active tab, the extensions menu, the "added" bubble. Ardia hit a crash in `chrome.action.openPopup`. We test these on day one.
6. **Keys, IME and accessibility** across GPUI's view and Chrome's window. Hidden test runs can't prove these, so a person checks VoiceOver and Narrator at M3 and M6.
7. **Saved data.** A changed signing identity or keychain name can lock users out of their passwords. We fix both before storing real data, and test signed upgrades with seeded data.
8. **GPUI fork drift.** Every change in `third_party/gpui` keeps its `jaal:` marker and a line in `JAAL.md`. We try to upstream "render into an existing view" to gpui-ce.

## Where the two plans differed

| Question | Fable | Astra | We picked | Why |
|---|---|---|---|---|
| Where our window class starts | Chrome's `WebUIBrowserWindow`, with the `kWebium` flag on | Our own adapters; Webium not considered | Webium's code, with the flag off and one hook on its check | It exists in our exact Chromium (`chrome/browser/ui/webui_browser/`, 37 files) and already guards the spots Ardia hooked by hand. The flag itself also adds a renderer add-on to every page and turns off the sad-tab page |
| Threads | Every call on the UI thread; replies may arrive before the call returns | Calls from any thread; replies always later | UI thread only, replies always later | GPUI already runs on the UI thread, so cross-thread calls buy nothing. A reply during a call would break GPUI's borrow rules |
| Where engine source lives | A separate `jaal-engine` repo | `engine/` in this repo | `engine/` here | The 40 GB checkout is outside the repo either way, and one PR can change the header and the Rust side together |
| How often to rebase | Monthly | Every two-week milestone plus weekly refreshes | Decided at first publish | Chromium's release doc confirms the two-week cycle. Depot does the builds; M9 measures what one rebase costs first |
| Diff budget | Under 400 changed lines in Chrome files | Review at 30 files or 1,500 lines | Both: 400 is the goal, 1,500 triggers review | |
| Privacy patches | Picked list from the start | Library on unmodified Chromium first | Unmodified first, then one concern at a time | A failure then has one cause |
| Tab changes | Plain events | Numbered transactions with conflict checks | Plain events for now | One thread already orders every event. Numbered transactions come back if a race shows up |
| Ardia's code | Ideas only | Ideas only | Ideas only | Ardia is GPL-3.0. jaal keeps its licence choice open |

## Decided by the developer (2026-11-09)

- Engine work happens on the Windows laptop. Mac and official builds run on Depot.
- Keep Manifest V2 so the full uBlock Origin runs. 1Password and most popular extensions must work as in Chrome.
- The rebase cadence is decided when jaal is first published.
