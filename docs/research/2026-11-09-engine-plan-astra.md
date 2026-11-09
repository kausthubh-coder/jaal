# Astra plan: jaal's engine library

Build a small layer inside Chromium's `chrome/` browser layer, exposed through a versioned C ABI to Rust and GPUI.
Keep Chrome's browser services and native page rendering. Replace its window UI, and qualify Windows and Mac together.

This is a design proposal, not an implementation or a runtime validation. Research date: 2026-11-09. Every proposed API, budget and gate below is new work unless identified as checked source behavior.

## 1. Decision and evidence

The source roots used below are:

| Label | Exact source |
|---|---|
| Jaal | `C:/src/jaal-council/jaal-bakeoff`, commit `5906a4cdd268ea87a32b4d0a79170ed3be6348e3` |
| Ardia | `C:/src/jaal-council/ardia-upstream`, commit `dc81514b22bcf6dd7f945a86d44e1f06e6c9ca72` |
| Intel fork | `C:/src/jaal-council/ardia-intel-mac`, commit `e8b8b491ce5f52aed25bb6445cab9e2220e58eb7` |
| Chromium | `C:/src/chromium/src`; `chrome/VERSION` reports `156.0.8078.88` |

The brief supersedes older research that chose Helium or a different UI. Preserve the decided sequence: engine, gpui-a migration, essentials, product UX, release, bugs, then AI. The benchmark fixture remains unchanged. [Jaal plan][J1] [Bake-off contract][J2]

### Compare the actual alternatives

Costs below are design judgments, not measured line counts or performance results.

| Approach | Extensions and Web Store | Essentials checklist | Windows and Mac | Code and release upkeep | Speed and memory |
|---|---|---|---|---|---|
| **Chrome-layer library, selected** | Retains the browser layer needed for real extension tabs, windows, profiles and UI integration. Store installation and updates still need end-to-end qualification. | Retain browser services. Implement window, toolbar and prompt adapters. Google-dependent services remain separate blockers. | Chromium supplies both platform foundations. Our native hosts and GPUI integration must be built on both. | Carry our adapters and narrow hooks. Rebase directly against Chromium. We own every broken assumption about missing `BrowserView`. | Native composition avoids a deliberate page-to-GPUI texture copy. No evidence yet that the final browser beats either baseline. |
| CEF Chrome runtime/style, with jaal patches | A credible candidate, not the old partial-extension Alloy model. Current CEF describes full Chrome functionality for Chrome style. Removing its window assumptions still needs validation. | More embedder callbacks and packaging already exist. CEF does not establish that every jaal service and UI seam has a public API. | Existing CEF platform support reduces initial hosting work. GPUI integration still remains. | Likely less bootstrap code initially. Custom service exports and window changes mean maintaining Chromium, CEF and jaal patches together. | Same underlying engine. No measured basis for claiming meaningful overhead merely because it is CEF. |
| Content-layer embedder, using Electron as the comparison | Electron explicitly supports a subset, not arbitrary Store extensions; it loads unpacked extensions. This fails the core requirement as supplied. | Rebuild or port much more browser-level functionality. A page engine is not the password, profile, extension and browser UI stack. | Electron supports desktop platforms, but substituting GPUI would create another custom embedder. | Highest browser-service reconstruction burden. A stable public content API does not provide missing Chrome behavior. | A lean shell is possible, but no verified savings justify missing functionality. Electron's JavaScript shell is unnecessary for this Rust UI. |
| Keep stock Chrome window UI and customize it | Keeps the closest existing browser behavior. | Lowest initial UI-adapter burden. | Both native platforms. | Fewer viewless-browser seams, but product changes touch Chrome UI. | Useful control build. Fails the chosen GPUI window design. |

Evidence for the layer distinction: [CEF's rationale][W1]. Current CEF distinguishes Chrome and Alloy styles, allows native parent windows, and limits a Chrome-style window to one Chrome-style BrowserView. Its toolbar can be omitted or accessed separately. These are constraints to test, not proof that CEF cannot work. [CEF style contract][W2] [CEF toolbar contract][W3] [Electron extension contract][W4]

The selected route keeps the services jaal needs without making CEF a permanent second integration layer. Ardia's source demonstrates the main architectural pattern. It creates a real Chrome `Browser`, supplies a `BrowserWindow`, hosts live `WebContents`, and exports service calls. Its code also shows how much work remains beyond that pattern. [Ardia architecture][A1] [Window implementation][A2] [Service boundary][A3]

Do not promise a tiny finished engine because an early spike was tiny. Ardia's spike size and rehearsal rebase were historical snapshots. Its later patch series covers many more prompt, extension, lifecycle and privacy changes. [Historical spike][A4] [Current series][A5]

### Other projects: what to take

| Project | Checked lesson | Decision for jaal |
|---|---|---|
| Brave | Prefers code in its own layer, then compile-time overrides, then small patches. Its rebase guide requires reviewing removed or changed upstream behavior. [Override rules][W5] [Rebase guide][W6] | Adopt ownership and minimal hooks. Prefer explicit typed hooks over a broad macro override system. Do not copy entire Chromium files. |
| ungoogled-chromium | Maintains an ordered, no-fuzz patch series; separates core and optional patches. The root license is BSD-3-Clause. [Design][W7] [License][W8] | Select individual patches and dependencies. Record provenance and license per patch. Never apply all defaults blindly. |
| Helium | Publishes platform packaging and a custom uBlock Origin fork with its filters and CNAME support. [Repository][W9] | Use behavior as a benchmark. Do not import its GPL engine patch chain. Blocking parity needs explicit tests. |
| Thorium | Documents ThinLTO, PGO and CPU-specific release profiles. [README][W10] | Use normal Chromium release optimization first. Add CPU variants only after measured benefit and hardware coverage. No speed claims imported from Thorium. |
| Vivaldi | Publishes Chromium changes used to support a separate HTML/CSS/JS UI. [Source explanation][W11] | Further evidence that browser logic and custom UI can be separated. Its HTML UI is not our selected renderer. No source-package audit performed. |
| Arc on Windows | The maker documents Chromium, Chrome extensions and a Swift Windows UI. [FAQ][W12] | Product precedent only. Its internal hosting ABI and maintenance cost are unverified. |
| Opera | Official help supports installing Chrome extensions. [Help][W13] | Compatibility reference. Its internal engine boundary is unverified. |
| WebView2 | Microsoft exposes installation of unpacked extensions into a profile. [API][W14] | Do not describe it as having no extensions. This API does not establish full Store/browser parity, and it does not meet our own Chromium engine plus Mac requirement. |

### Corrections that change the plan

1. **Updates cannot wait for a monthly build.** Chrome's current policy is a milestone every two weeks and weekly stable refreshes. Track those releases and emergency fixes. The monthly estimate in jaal's plan is stale. [Release cycle][W15]
2. **A Windows page is not simply an HWND.** Chromium's `gfx::NativeView` is `aura::Window*` on Aura platforms; its accelerated native widget is an HWND. The engine must supply an Aura/Views host before handing Windows handles to GPUI. [Native types][C1]
3. **Skipping GPUI's Windows loop also skips initialization.** This checkout starts its vsync thread inside `run()`, before `GetMessageW`. Split setup from loop ownership. [Windows GPUI loop][J3]
4. **The Intel fork is not verified Intel support.** Its only advertised remote branch was `main` at the checked commit. Its GN arguments set ARM64; its Xcode project sets ARM64; its README says Intel is unsupported. No Intel implementation delta was found in the examined build paths. Do not infer Windows support from its repository name. [Intel GN args][I1] [Intel Xcode project][I2] [Intel README][I3]

Chromium itself documents Mac cross-compilation in both directions. An Intel release still requires architecture-correct dependencies and testing on Intel hardware. That is a separate qualification task. [Chromium Mac architectures][C2]

## 2. Engine and app boundary

### Ownership

The engine owns `Profile`, `Browser`, tab strips, `WebContents`, navigation history, downloads, permissions, extension hosts, session restore and Chrome service objects. The app owns GPUI entities, sidebar layout, command-bar state and later spaces/folders metadata. Chrome already constructs its tab strip, profile keep-alive and browser features in `Browser`; build around those lifetimes. [Browser construction][C3]

Start with one regular profile per visible window, plus separate private windows. Additional profiles can have additional windows. This avoids Ardia's multi-profile window ambiguity during the engine milestone. Later profile paging must map each profile to a distinct real `Browser`, with explicit activation and prompt routing. Never move a live page between unrelated profile storage contexts. Ardia's source and parity notes show why window activation and profile attribution need deliberate handling. [Window activation][A2] [Parity and private windows][A6]

There is one authoritative tab model: Chrome's. The sidebar is an observed projection. A page popup, extension-created tab or restore operation inserts the actual Chrome tab. Do not recreate it by loading its URL. That would risk losing opener relationships, request bodies and navigation state. Ardia follows the live-tab approach. [Ardia architecture][A1]

### ABI and packaging boundary

Use a small C header, generated Rust `extern "C"` declarations, and a handwritten safe Rust wrapper. Do not expose Chromium C++ classes, Rust types, STL containers, exceptions or allocator ownership across the boundary.

Proposed public contract:

| Entry | Contract |
|---|---|
| `jaal_engine_get_api(abi_major, struct_size)` | Returns a function table with ABI minor, build identity and capabilities. Reject incompatible layouts before opening a profile. |
| `submit(request_id, command, bytes, length)` | Copies validated input, queues an engine command, and later emits one terminal reply. |
| `set_event_sink(callback, context)` | Installs the app's event sink. Events carry explicit profile, window and tab handles when relevant. |
| `post_ui_task(callback, context)` | Schedules host work on the browser UI sequence without a second pump. The callback context remains owned by the caller until execution or cancellation. |
| `window_host`, `surface_attach`, `surface_layout`, `surface_detach`, `surface_focus` | Typed, UI-thread-only native operations. No JSON on frame-by-frame layout paths. |
| `request_quit(reason)` | Starts cancellable browser shutdown. Does not immediately free the engine. |
| `subscription_cancel(id)` | Emits a completion barrier after already accepted callbacks have drained. |

Use schema-checked UTF-8 JSON for low-rate service payloads initially. Use fixed-layout structs for handles, rectangles, input policy and surface descriptors. Ardia shows that plain C service calls are practical, but its documented callbacks may run before the call returns. Jaal's asynchronous command API deliberately forbids that reentrancy. [Ardia C exports][A3] [Ardia public header][A7]

An illustrative result is `Reply { request_id, status, payload }`. Status distinguishes invalid arguments, stale handles, cancellation, unsupported capability, policy denial and internal failure. Do not return success with empty data for an unimplemented feature.

Opaque handles contain a slot and generation. Never reuse an invalidated generation. Keep Chrome extension IDs and Chrome tab/window IDs available as separate fields where integration requires them. Cross-window tab moves retain the tab handle; a new document gets a new document identity. Renderer replacement does not silently change the app's tab identity.

Input buffers are borrowed only during `submit`, then copied by the engine. Reply/event buffers belong to the engine and are valid only during the callback. Rust copies retained values. A large binary result uses an engine buffer handle and `buffer_release`; Rust never frees engine allocations. Secret buffers use explicit lengths and wiping in both layers. They never enter diagnostic event logs.

Native handles are borrowed capabilities, not ownership transfers. Only the engine attaches or destroys page views. A surface lease keeps its host alive; shutdown detaches the lease before destroying the platform view. GPUI owns its own render child, and destroys that child before acknowledging the host's final close.

### Threads and event order

- Chromium owns the browser UI thread and loop. GPUI's foreground work runs on that thread.
- `submit` may be called from a worker. It posts work; it never blocks on the UI thread.
- All service completions and state events are serialized on the UI sequence. GPUI copies them into a queue and applies them after its current update returns.
- Never hold a Rust borrow, GPUI mutex or C++ lock while invoking the other layer or entering a native modal operation.
- Native layout and focus calls may be synchronous on the UI thread. They must not call the app back synchronously.
- Coalesce progress, title and favicon state. Never coalesce away tab removal, prompt invalidation or command completion.
- Each tab-strip transaction has a revision and a cause request ID. Return the resulting revision on success. A stale expected revision produces a conflict plus a snapshot request path.
- Cap event queues. If state events overflow, signal resynchronization and send a fresh snapshot. Preserve terminal replies and destruction barriers.

Ardia's API exposes revisioned tab transactions and live events; use that lesson without adopting its implementation. [Ardia tab contract][A6]

### Commands and events

These are proposed logical call names. The Rust wrapper exposes typed methods with the same domains.

| Domain | Calls | Events and essential semantics |
|---|---|---|
| Profiles | `profiles.list/create/open/close/delete`, `profiles.open_private` | `profile.ready/error/closing/closed`. Deletion requires no dependent windows or pending service work. A private profile has an explicit parent and lifetime. |
| Windows | `windows.create/activate/set_bounds/request_close` | `window.created/activated/bounds_changed/close_cancelled/closed`. Extension-created and page-created windows use the same factory. |
| Tabs | `tabs.open/activate/move/pin/duplicate/request_close/reopen_closed`, `tabs.snapshot` | `tabs.transaction`, `tab.crashed`, `tab.unresponsive`. Moves are atomic. Close can be cancelled. Adopt live popup contents through the engine. |
| Navigation | `navigation.load/back/forward/reload/stop`, `navigation.view_source`, `search.suggest/accept` | `navigation.started/committed/failed`, `tab.state`. Preserve dispositions, user gestures, referrers and form submissions. Use Chrome's URL/search services, not ad hoc URL guessing. |
| Find and zoom | `find.start/next/stop`, `zoom.get/set/step/reset` | `find.result`, `zoom.changed`. Keep request IDs, selection rectangles and per-site zoom semantics. |
| Downloads | `downloads.list/pause/resume/cancel/open/reveal/remove/resolve_warning` | `download.changed/removed`, `prompt.requested`. Open only after the engine reports completion and safety state. |
| Permissions and sites | `permissions.respond`, `sites.get/set/reset/clear_data`, `browsing_data.clear` | `permission.requested/invalidated`, `site.changed`. Prompt decisions remain bound to the requesting document and origin. |
| Extensions | `extensions.list/install_unpacked/set_enabled/uninstall/set_incognito/set_pinned`, `extensions.invoke_action/open_options/respond_install`, `extensions.side_panel_open/close` | `extensions.changed/action_changed`, `extension.install_prompt`, `surface.requested`. Web Store requests go through Chrome's installer, not an app downloader. |
| Prompts and choosers | `prompts.respond`, `choosers.select/cancel` | `prompt.requested/updated/invalidated`. Use one-shot engine tokens, explicit allowed actions and cancellation on document destruction. |
| Menus | `menus.show_page_context/show_app/dismiss`, `menus.execute` | `menu.closed`. Engine context menus retain extension entries and command ownership. A command token expires with its menu. |
| DevTools | `devtools.open/close/set_dock` | `devtools.changed`, `surface.requested`. Docked tools are a separate Chrome-rendered surface; detached tools are an engine-managed native window. |
| Fullscreen and PiP | `fullscreen.request/exit`, `pip.enter_video/exit`, `media.command/set_muted` | `fullscreen.changed`, `pip.changed`, `media.changed`, `capture.changed`. Chrome's exclusive-access policy owns approval and Escape behavior. |
| Printing | `print.preview/print/save_pdf` | `print.completed/failed`. Preserve Chrome PDF/preview and native pickers. |
| Data | `history.query/delete`, `bookmarks.list/create/move/import`, `import.inspect/start` | Progress and change events. Import only user-selected copies or exported data. |
| Passwords and autofill | `passwords.list/unlock/reveal/save/update/remove`, `autofill.list/save/remove/respond` | Prompt and store events. Secret reads require the engine's authentication policy. |
| Browser settings | `settings.get/set`, `default_browser.request`, `performance.set_memory_saver/set_energy_saver` | Setting and OS integration results. Unsupported managed policies are explicit. |

The table is a design inventory, not a claim that all these are one-line Chromium calls. Ardia's service layer demonstrates many direct services, while its parity table lists incomplete or visually unverified paths. [Service implementation map][A3] [Parity][A6]

Implement service adapters against Chrome's native controllers. Do not automate hidden `chrome://` pages to read passwords, manage extensions or change settings. Ordinary visible Chrome internal pages can remain available where their UI works with the host. Ardia's migration explicitly replaced hidden-page scripting with direct service calls. [Migration API comparison][A4]

### Trust boundary

The C ABI is private to trusted code inside the browser process. It is not a website API, extension API or network service. Renderers keep Chrome's sandbox and Mojo interfaces. Do not copy Ardia's main-world page bridge by default. The current scope needs no privileged JavaScript injected into every page. If a later feature needs renderer cooperation, add a narrow Mojo interface with frame, origin and document validation. AI remains last. [Ardia renderer design, used as a comparison][A6] [Jaal order][J1]

## 3. Startup, main loop and shutdown

### Ship a runtime, not a lone library file

The engine artifact contains its launcher, engine library, helpers, resources and public SDK. A browser is still multiprocess. The UI build produces `jaal_ui.dll` or `libjaal_ui.dylib` and a bundle assembly step. The launcher loads that module only in the browser process, from a fixed verified bundle path. It supplies the engine function table to `jaal_ui_start` after browser initialization.

This lets ordinary UI work compile Rust and assemble an app from a pinned engine download. No Chromium relink is needed for a sidebar change. The module is private application code, not a third-party plugin system. Keep it loaded until process exit. Release builds reject arbitrary module paths and mismatched build manifests.

On Windows, preserve Chromium's launcher and DLL-loader ordering, including `chrome_elf`, sandbox setup and process dispatch. The checked loader initializes sandbox services before calling `ChromeMain`. Do not replace that sequence with a bare Rust executable that only calls into `chrome.dll`. Exact renamed targets are implementation work. [Windows bootstrap][C4] [Windows engine entry][C5]

On Mac, preserve the framework/helper layout and start through Chrome's entry point. Initialize Chromium before loading GPUI code that could touch AppKit. Ardia runs its host inside Chrome's loop and posts its ready callback after browser startup. [Ardia main delegate][A8]

### GPUI inside Chromium

Add an explicit GPUI embedded-runtime mode to the vendored version. Keep the patch separate from engine patches.

| Platform | Required integration |
|---|---|
| Mac | Chromium creates `BrowserCrApplication`. GPUI must neither instantiate `GPUIApplication` nor replace `NSApp.delegate` nor call `[NSApp run]`. Replace GPUI's app-ivar assumptions with an explicit platform object. Install menu, activation, open-URL and termination forwarding through a typed adapter. |
| Windows | Keep GPUI's message-only window, task dispatcher, GPU recovery handling and vsync initialization. Skip its `GetMessageW` loop. Chromium dispatches its messages. Coordinate COM/OLE and process DPI initialization before any HWND creation. |
| Both | Separate `initialize`, `attach_ui`, `begin_frame_delivery`, `request_quit` and `shutdown`. Deliver readiness once, after profile/browser infrastructure is ready and startup has unwound. Never add a periodic engine polling timer. |

The Mac requirement is concrete. Chromium checks that NSApp is `BrowserCrApplication`; this GPUI version writes its platform pointer into its own app and delegate. Merely skipping `run()` or changing the class name is insufficient. [Chromium Mac application][C6] [GPUI Mac setup][J4]

The current CEF bake-off does the reverse and uses a fallback timer. Retire its `pump.rs`, `nsapp.rs` protocol injection and CEF loader from the new-engine executable. Preserve the CEF executable as the comparison build during migration. Never load both Chromium runtimes in one process. [Current pump][J5] [Current app patch][J6] [Ardia migration lesson][A4]

### Cancellable shutdown

Use an explicit state machine: running, close requested, waiting for browser decisions, committed to close, draining, closed. A cancelled page or download prompt returns to running. Do not release the process keep-alive when a quit is merely requested.

Route titlebar close, keyboard quit, Dock quit, OS session end and API requests into the same engine path. Preserve platform-specific limits on delaying OS shutdown. After approval, cancel prompts, stop UI frame work, detach pages, dispose GPUI children, close browsers, flush services, and finally exit Chrome's loop. Keep callback state alive until the shutdown barrier. Ardia's close and termination fixes are a warning against a single irreversible `closing=true` flag. [Ardia runtime lessons][A4] [Close implementation][A9]

## 4. Native pages, windows and overlays

### Default: a Views-backed native frame with GPUI drawing the browser UI

Keep gpui-a's composition: GPUI below, native page above, transparent child windows for overlays. Change one implementation detail deliberately: let a small engine host own the physical top-level NSWindow/HWND through `views::Widget`. GPUI draws the client UI inside it. There is no Chrome tab strip, toolbar or `BrowserView`.

This is an integration extension to GPUI, not a claim that GPUI already accepts borrowed windows. Split its platform window creation from its render-child attachment. It must handle a borrowed outer window without replacing the engine's delegate or window procedure. Prove this before building service UI.

Why this default: Ardia's actual main window is Views-backed. It does not attach every Chrome bubble to an arbitrary toolkit-created window. Chromium's widget supplies native parenting, dialog ownership and activation context. Removing that host would create more Chrome adapters. Ardia still needs explicit prompt hooks despite keeping the widget. [Ardia window creation, lines 84-118][A2] [Ardia hook inventory][A10]

This preserves the chosen GPUI renderer and fix A. It changes native-window lifetime ownership from the bake-off, where `Application::open_window` creates the main window. [Current startup][J7]

If borrowed-window integration fails the native acceptance tests, keep the same Chrome-layer engine and evaluate a GPUI-owned outer window with an explicit Views child host. That alternative must pass the same dialogs, accessibility and focus tests. Do not silently switch to page textures, hide a real Chrome window offscreen, or declare a null-returning `BrowserWindow` complete.

### How a page reaches the app

| Platform | Proposed native arrangement | What the app receives |
|---|---|---|
| Mac | A frameless Views-backed NSWindow owns the root. GPUI attaches its Metal-backed NSView. The engine attaches each visible page's native NSView above that view, in the page rectangle. | A borrowed root descriptor and a page surface lease. The engine obtains the page NSView from `WebContents::GetNativeView().GetNativeNSView()`. |
| Windows | A frameless Views-backed root owns the real HWND and Aura context. GPUI attaches a render child HWND. A page host uses `views::WebView` inside an engine-owned native child widget above GPUI. That host attaches the page's Aura window. | A borrowed parent HWND and a page-host HWND plus lease. Never reinterpret an Aura pointer as an HWND. |

The Mac native extraction exists in Ardia. Chromium's WebView has native attach/detach and focus paths; its Windows desktop host resolves parent HWNDs through an Aura host. The exact Windows child-widget arrangement with GPUI is **unverified** and is the first integration gate. [Ardia page view][A11] [Chromium WebView][C7] [Windows widget host][C8] [Widget parenting contract][C9]

Keep native page input, IME, drag/drop, selection, accessibility and composition in Chromium. The app does not forward raw pointer events to renderer coordinates. Route reserved browser shortcuts before the page, and other shortcuts after the page declines them. Test AltGr, dead keys, non-Latin IME, menu accelerators and keyboard lock.

Create page hosts lazily, preferably per visible pane, rather than one expensive native widget per background tab. Switching transfers a live page view between its hidden state and a visible host. Preserve Chrome's visibility, occlusion, focus and capture semantics. A background audio or capture tab must not be discarded just because the sidebar does not show its page.

Layout uses device-independent coordinates relative to the root client area. The engine converts to screen coordinates and device pixels. Include a geometry revision and scale factor. Update native page bounds during OS resize callbacks, not only after the next GPUI paint. Account for titlebar insets, negative monitor coordinates, mixed DPI and fullscreen transitions.

### Overlay policy

Use an overlay manager shared by GPUI overlays and engine-owned popup hosts. Its record includes parent window, tab/document, anchor rectangle, focus policy, dismissal policy and z-order category.

- On Mac, attach the command bar as a child NSPanel at its parent's level. Recompute anchors on resize and monitor changes. The bake-off already uses child attachment and explicit placement. [Existing overlay][J8]
- On Windows, use an owned transparent popup above the native page host. Follow parent moves, DPI changes, minimize/restore, activation and virtual desktops. It must not appear as a separate taskbar entry.
- Transparent regions must not accidentally swallow input. Modal overlays intentionally block their page; nonmodal decoration must not.
- Opening a security prompt dismisses or suspends competing GPUI overlays. Page content cannot cover the trusted origin or action buttons.
- Closing an overlay restores focus only if its original window and document remain valid. Switching to another app must not trigger focus theft.
- Native popups, file pickers and PiP use their own native windows where appropriate. Do not force them into a single composited GPUI panel.

The current bake-off has a specific Mac key-status reentrancy fix. Carry its behavioral regression test into the embedded GPUI integration, and replace runtime method patching with a focused source fix when possible. [Key-status fix][J9]

### Which UI stays Chrome's

The engine owns every underlying decision, regardless of who draws it. This table is the default implementation split, not proof that the existing Chrome widget attaches without adaptation.

| Feature | Drawn by | Required adapter and acceptance check |
|---|---|---|
| Sidebar, URL field, command bar, browser menu | GPUI | Use authoritative tab and suggestion state. Verify keyboard traversal, IME and screen-reader labels. |
| Extension action popup | Chrome extension view inside an engine popup host | Invoke the action through Chrome's action controller, preserving user gesture and `activeTab` grants. Do not load `popup.html` as a normal tab. Test current window/tab and close-on-blur. |
| Extension side panel | Chrome extension view; GPUI owns panel layout | Implement a side-panel adapter with per-tab/per-window scope. Preserve extension context and resize its native host. |
| Extension install, changed-permission and uninstall prompts | GPUI initially, using Chrome's prompt model | Show Chrome-generated permission warnings, identity and source. Preserve allow/cancel and re-enable semantics. Never auto-grant on Store download. |
| Site permission prompts and blocked-popup controls | GPUI | Adapt the existing permission delegate. Preserve origin attribution, quiet prompts, one-time permissions and OS permission steps. Late or duplicate responses must fail. |
| Password save/update and generation offer | GPUI prompt over Chrome controller | Keep password matching, generation, store writes and authentication in the engine. Do not emulate filling by injecting page JavaScript. |
| Autofill suggestions, password dropdowns, passkeys, FedCM | Chrome native widgets | Supply anchor geometry, modal hosts, focus and color-provider context. Retain OS authentication dialogs. Test navigation and window closure while visible. |
| Address/card save bubbles | Chrome where the native bubble works with our adapter | Implement `AutofillBubbleHandler` support. If a toolbar dependency prevents reuse, draw only the view in GPUI over the same controller. Do not disable saving to avoid the dependency. |
| Downloads list and progress | GPUI | Observe DownloadManager state. Keep target selection, quarantine, dangerous-file verdicts and warning decisions in Chrome. Preserve warning detail and cancellation. |
| Find bar | GPUI | Implement the browser find-bar adapter and display exact match count/selection. Chrome performs finding and scrolling. |
| Video PiP and document PiP | Chrome native windows | Preserve sizing, focus, always-on-top, close and return-to-tab. Both kinds need real visual tests. No custom PiP rendering in the first engine. |
| Fullscreen, pointer-lock and keyboard-lock notice | Chrome controller and trusted native notice | Implement its context against the host widget. Preserve Escape and hold-Escape exit behavior. GPUI hides surrounding chrome only after engine state changes. |
| Page context menu, select dropdown, date/color chooser | Chrome/native | Retain spellcheck and extension menu items. Add app items with explicit command routing. Verify owner window and DPI placement. |
| File pickers, print preview, PDF, HTTP auth, JavaScript dialogs, beforeunload | Chrome/native | Supply per-tab modal hosts and parent handles. Test cancellation and tab navigation while a dialog is open. No orphan dialogs. |
| DevTools | Chrome frontend | Native page host for docking, engine-managed native window for detaching. Preserve inspected-tab association when moved or closed. |
| Media/device capture picker, USB/Bluetooth chooser | Chrome/native first | Reuse picker controllers and native views. Add a GPUI view only if a concrete toolbar dependency requires it. Preserve selected-frame grants and revocation. |
| Certificate errors, HTTPS-first interstitials, PDF errors | Chrome page/native UI | Retain the security decision and interstitial content. A new sidebar must not turn errors into blank pages. |

Ardia's hook list and parity table document exactly these trouble spots: side panels, install prompts, password bubbles, exclusive access, per-tab modal hosts and windows created by extensions. They guide the inventory; they do not prove jaal's hosts work. [Hook inventory][A10] [Parity UI and runtime sections][A6]

## 5. Essentials and the meaning of parity

Use the full checklist in `docs/plan.md` as a release contract. Do not reduce it to the bake-off. Every row needs a named owner, test and result on each shipped OS/CPU. Passing compilation or hidden automation alone cannot close a visual/input row. [Essentials checklist][J1]

| Checklist group | Engine plan | Pass-or-fail evidence |
|---|---|---|
| Tabs and windows | Real Chrome tabs, private profiles, native window factory and Chrome session services. App stores only product metadata alongside stable tab/session mappings. | Open/close/reopen, duplicate with back-forward history, drag between windows, extension-created windows, beforeunload cancellation, crash/restart restore. Private history and sessions never persist into normal restore. |
| Navigation | Keep omnibox/search services, navigation controllers, find/zoom, print, PDF, view source and DevTools. | Suggestions and search accept the intended input. Back/forward preserves forms and history. Test PDFs, printing, zoom persistence and dock/undock tools. |
| History, bookmarks, imports, downloads | Keep Chrome data services. Build jaal views and import adapters. Chrome/Arc import operates on authorized exported data or isolated copies. | Import fixtures retain hierarchy, URLs and expected counts. Restart preserves data. Interrupted, resumed, dangerous and cancelled downloads behave correctly. |
| Passwords, passkeys, autofill | Keep store encryption and controllers. Fix product identity before persisting user secrets. | Signed upgrade can decrypt seeded test data. Wrong/missing key fails clearly without reset. Test save/fill/update, generation, passkeys, addresses and cards with OS authentication. |
| Extensions | Preserve Chrome extension system, action dispatch, installation/update and native messaging. Implement popup, side-panel and prompt adapters. | Real Store install/update/uninstall on both OSs. Test service workers, content scripts, DNR, downloads, context menus, `tabs`, `windows`, incognito rules, native messaging, OAuth login and action grants. |
| Site controls | Preserve permissions, content settings, storage partitioning, HTTPS-first and blocked popups. | Grant/deny/one-time/revoke; navigate during a prompt; clear site data including partitioned storage; private/regular isolation; OS denial handling. |
| Media | Preserve native rendering, fullscreen, both PiP modes, media sessions and autoplay policy. | Real audio/video, controls, capture, permission revocation and GPU acceleration. DRM is separately gated below. |
| Profiles and OS integration | Keep browser data ownership. Implement default-browser registration, shortcuts, spellcheck, accessibility and performance settings. | Fresh and upgraded profiles; system URL launch; dictionary behavior; VoiceOver/Narrator through GPUI and page; memory saver, energy saver, capture exemptions and wake/sleep. |
| Content blocking | Bundle upstream uBlock Origin as a separate extension payload, subject to license review. Evaluate the BSD ungoogled MV2 patch against the pinned Chromium version. | Match Helium on an agreed corpus covering network, cosmetic, scriptlet, dynamic/user filters and CNAME cases. Test first navigation, restored tabs, private windows and updates. Any unresolved capability gap fails parity. |

The current ungoogled patch series includes an MV2 compatibility patch. Its presence is not proof that full uBlock Origin works on this exact build. Helium's custom fork adds behavior, including CNAME handling. Do not substitute uBlock Origin Lite and call it equivalent. [MV2 patch][W16] [Helium blocker description][W9]

Default to upstream uBlock Origin plus independently implemented, audited compatibility hooks where required. Keep its extension code and license separate from the engine. Do not copy Helium's GPL hooks into a supposedly permissive engine. Exact license consequences of the packaged combination remain a release review item. Upstream uBlock has its own GPL license. [uBlock license][W17]

### Features Chromium source alone cannot guarantee

The strict goal of never being worse than Chrome cannot be honestly certified until the following external-service gates pass.

| Feature | Default and unresolved question |
|---|---|
| Widevine and streaming sites | Start licensing and browser-approval work early. Keep CDM integration points and stable product identity. Shipping a CDM-capable binary does not prove service acceptance or high-resolution playback. Apply for the required rights; verify the signed final build against target services. No copied Chrome CDM or signature bypass. |
| Safe Browsing and harmful downloads | Preserve protection plumbing. Obtain supported service access or integrate a licensed equivalent with measured coverage and update delivery. Do not select `safe_browsing_mode=0` merely because another fork does. Service availability and terms for jaal are unverified. |
| Translate | Keep a provider boundary behind the browser command. Default to an authorized translation service, with page data sent only after the user invokes it. Provider and quota are unverified. |
| Cast | Keep discovery and media-router infrastructure. Qualify receiver discovery, tab/screen casting, protected content and required downloadable components. Keys alone are not proof of complete support. |
| Push notifications and geolocation | Do not disable GCM or location providers wholesale. Confirm permitted provider access and real notification/location behavior on both OSs. OS permission approval is separate from service access. |
| Sync | Do not depend on Google-account Chrome Sync. Default first to correct local profiles and restore. Design optional encrypted jaal sync later with its own protocol, provider and recovery story. If sync is part of the parity promise, release remains blocked until it exists. |
| Proprietary codecs | Decide distribution rights, build flags and platform decoder coverage. Test H.264/AAC and the required media corpus on the final artifacts. Do not infer playback rights from a GN flag. |

Evidence and limits: Chromium documents API access and quotas, while Widevine's license requires a separate agreement. Ardia's own build docs warn that enabling Widevine still needs VMP signing. These sources do not establish that jaal will be approved. [Chromium API keys][W18] [Widevine license][W19] [Widevine integration][W20] [Ardia build flags][A12]

The Safari-engine workaround in jaal's existing plan does not meet full extensions or Windows parity. Do not build it by default. If licensing remains blocked, label DRM as missing and keep the parity gate failed. An external-browser handoff would require an explicit scope decision. [Existing workaround proposal][J1]

## 6. Patches and upkeep

### Source layout

Keep maintained source in jaal, outside the generated Chromium checkout:

```text
engine/
  lock.json                 Chromium revision, toolchains, inputs and hashes
  public/jaal_engine.h      C ABI and schema version
  src/chrome/browser/jaal/  Browser/services/window adapters
  src/jaal/runtime/         Launcher integration and public ABI implementation
  patches/library/         Narrow Chromium integration hooks
  patches/privacy/         Selected privacy patches with provenance
  patches/series           Exact application order
  policy/privacy.json      Per-patch decision and feature impact
  build/                   Platform recipes and artifact manifests
  tests/                   Browser-level contract and packaging tests
crates/jaal-engine-sys/     Generated C ABI declarations
crates/jaal-engine/         Safe Rust API and state projection
third_party/gpui/           Focused embedded-runtime and render-child changes
```

This is proposed layout. Keep existing bake-off code working during migration. Factor neutral UI code from `gpui-cef` only where the new app needs it. Do not move files simply to make the tree look finished. The existing GPUI fork records its small changes in `JAAL.md`; extend that record for loop and window integration. [GPUI patch record][J10]

Use normal GN dependencies and subclasses first. Add typed callback/factory hooks only where upstream offers no extension point. A hook should preserve stock behavior when unset. For the production window factory, add a jaal-specific path rather than globally removing Chromium's `CHECK_IS_TEST` on injected windows. [Current test-only guard][C3]

Maintain explicit adapters for `BrowserWindow`, `BrowserWebContentsDelegate`, the native host, modal host, location-bar anchors, find, exclusive access, extension UI and service calls. Audit every inherited method. A null implementation is acceptable only when callers permit absence and the feature is intentionally unsupported; it is not a general strategy for satisfying the interface.

Avoid edits hidden in text-replacement shell scripts. Materialize source overlays and apply one ordered patch series. Fail on partial application, fuzz or an unknown base. Reconstruct the touched tree independently and compare it with the tree used for compilation. Ardia's exact reconstruction check is worth adopting as a requirement, while its mixed patch/apply-script history shows what to avoid. [Ardia patch checks][A13]

### Privacy selection

First bring up the library on unmodified Chromium behavior. Then add privacy patches one concern at a time. A failure must be attributable to a host change or a privacy change.

The candidate list comes from the checked ungoogled series. Individual patch applicability, dependencies and effects on Chromium 156.0.8078.88 are **unverified** until the import gate. [Current patch inventory][W21]

| Decision | Candidates or behavior | Required guard |
|---|---|---|
| Start with | Disable unsolicited domain-reliability uploads, WebRTC log uploads, profile-avatar fetches and unwanted AI defaults. Prefer supported settings when enough. | Verify idle network behavior and normal site compatibility. Keep local crash evidence. |
| Review separately | Update pings, RLZ, field trials, Safe Browsing reporting, autofill server queries, search suggestions and network-time service. | Preserve extension/component security updates, DRM identity, anti-phishing checks and user-invoked functionality. A patch name is not a complete impact analysis. |
| Reject wholesale | `disable-webstore-urls`, `disable-gcm`, `disable-download-quarantine`, disabling Safe Browsing, global domain substitution, broad default-extension removal. | Store access, notifications, file safety, PDF and other required services must keep working. |
| Add deliberately | MV2 compatibility needed by the selected blocker, plus a narrow product preference policy. | Verify security exposure, platform behavior and each Chromium rebase. Stop carrying compatibility code that cannot be maintained safely. |

Record upstream commit and checksum, SPDX information, affected files, rationale, dependencies, removed functionality, tests, owner and removal condition. Separate Chromium engine updates, extension updates and component updates. Direct-to-Google Store access must retain publisher/signature verification, permission escalation and revocation behavior. Disabling telemetry is not permission to remove security updates.

Do not adopt Ardia's performance, capture, PiP, policy-provider or sandbox-related changes merely because they are in its series. Require a jaal-specific bug or measured benefit. Its own series documents broad behavioral decisions beyond embedding. [Ardia series][A5] [Ardia hook notes][A10]

### Diff budgets

Use review budgets, not promises or estimates of finished size:

- For the first dual-platform library: aim for at most 30 upstream files touched and 1,500 added/deleted upstream lines, excluding our new files and approved privacy patches.
- Review scope again if essentials exceed 60 upstream files or 4,000 changed upstream lines. Record the feature causing the increase.
- Report our own source size, GPUI changes and privacy changes separately. Never hide carried code in generated overlays to satisfy a line budget.
- A budget overrun triggers design review, not removal of required functionality. No development-time estimate is credible before the native-host and first-rebase gates.

These numbers are proposed maintenance limits. They are not measurements of Ardia or jaal.

### Each Chromium release

1. Watch stable security releases and stage the next milestone while it is in beta. Use stable as the default channel.
2. Pin the exact Chromium revision, DEPS, toolchain, PGO inputs and privacy revisions in a new engine lock.
3. Reapply the library and privacy series in a separate build checkout. Never reset the developer's existing build tree.
4. Review semantic changes in every implemented interface, even when patches still apply. Compile both platform adapters.
5. Run upstream tests for changed subsystems and jaal's ABI, native-host and essentials gates. Retain old passing checks.
6. Produce immutable artifacts and symbols. Assemble the current UI against them and perform signed upgrade/profile tests.
7. Publish a candidate, record results, then release through reviewed PRs and CI. Ship emergency security fixes independently of UI features.

Target security triage on the day of an upstream release and an emergency release as soon as the required checks pass. Measure that operational target before making a public SLA. A monthly engine rebuild is insufficient under the current release policy. [Release cycle][W15] [Brave rebase procedure][W6]

Practice a full milestone rebase before calling the engine maintainable. Keep the previous binary artifact, but never roll an older Chromium binary back onto a profile migrated by a newer version without a proven compatibility path.

## 7. Build, package and ship

### Machines and architecture targets

The machine capacities below come from the brief. I did not measure their current free space, load or build throughput.

| Machine | Role and prerequisite |
|---|---|
| Windows laptop | Let the existing plain-Chrome build finish under its current owner. Its result is M0 evidence only after that owner verifies it. Later use a separate registered engine checkout, with conservative build concurrency and sufficient disk. The current build and `out/` remain untouched. |
| Mac mini | Use for Mac UI/native acceptance and incremental engine work once Xcode and enough storage exist. About 29 GB free is not a credible plan for source, dependencies, build outputs, symbols and retained artifacts. Add dedicated APFS storage or arrange an approved builder first. No cleanup of unrelated files. |
| Depot, optional | Suitable runner types exist for Windows and macOS. Verify exact SDK, toolchain, disk and cache capacity before selecting one. Prepare a costed job proposal; do not start cloud builds without the developer's go. |

Chromium's Windows guide recommends more than 16 GB RAM and at least 100 GB free disk. Its Mac guide requires Xcode/SDK tooling and APFS. Depot publishes distinct Mac and Windows runner types. These are planning constraints, not estimates of build time on these machines. [Windows requirements][C10] [Mac requirements][C11] [Depot runners][W22]

Initial required targets are `macos-arm64` and `windows-x64`. Add `macos-x64` as a separate artifact and acceptance lane before claiming Intel Mac support. Parameterize target CPU, Rust target, GN flags, helper architectures, PGO profile and artifact naming from the beginning. Windows ARM64 remains unverified and outside the first qualification matrix.

Do not copy the Intel fork's build scripts as a portability solution. They still select ARM64 and its existing output conventions. Chromium's supported cross-build path is a better starting point. Cross-compiling does not replace running the result on its target hardware. [Intel build config][I1] [Chromium cross-build guide][C2]

Maintain two build recipes. Development builds optimize incremental iteration and preserve useful diagnostics. Candidate/release builds use non-component configuration, appropriate PGO/ThinLTO and symbols stored outside the download. Benchmark only comparable release configurations. Do not inherit `symbol_level=0`, disabled crash reporting, disabled Safe Browsing or relaxed compiler warnings from Ardia wholesale. [Ardia build configuration][A12] [Chromium signing build requirement][C12]

Long builds must run as background jobs with captured process identity, log path and exit status. If a job stops reporting, inspect its process and log within a minute. Do not infer success from a running process. Stop only a process started by that build task.

### Immutable engine downloads

Use an engine version such as `156.0.8078.88-jaal.1-abi1`. This is an example, not a claim that this Chromium version is still current when implementation begins.

Each archive contains:

- The launcher and engine binary, platform helpers, locales, resource packs, ICU/V8 data and required graphics/media dependencies.
- The C header, schema, generated bindings metadata, example host and a smoke-test executable.
- A manifest with target OS/CPU, minimum OS, Chromium commit, source commit, patch-set hash, ABI/schema version, feature flags and toolchain identity.
- Checksums, signed release metadata, third-party notices, a dependency/license inventory and source reproduction instructions.
- A separate matching symbol archive for native crash diagnosis.

Determine the actual dependency list from GN's packaged outputs and a clean-machine launch test. Do not guess the runtime from a hand-picked list of DLLs or frameworks.

The UI repo pins archive URL, digest, engine ID and ABI in its lock file. A fetch script verifies the signed manifest and archive digest before extraction. Extract into a versioned cache, reject path traversal, and validate all required files before switching the active cache pointer. Never fetch an unpinned `latest` during a normal build.

The final app release binds the UI hash and engine hash together. An ABI match alone is not proof that every behavior is compatible. Do not hot-swap the running engine DLL. Install a whole compatible release and restart cleanly.

### Mac packaging

Package the framework, helper apps and UI module with stable bundle identifiers. Sign nested code with the correct Developer ID identity and entitlements, then sign and notarize the final app and distribution. Verify Gatekeeper launch and sandboxed renderers on a clean user account. Chromium provides packaging/signing scripts; its signing guidance distinguishes development and production identities. [Mac signing][C12]

Use the same approved identity for the engine and UI module so library validation works without a broad exception. Keep JIT and media entitlements scoped to the processes that require them. Debug-only entitlements and test switches must not appear in production.

Freeze jaal's keychain service, account, access-group and bundle identity before real data is written. Test a signed update with seeded cookies and passwords. A temporary keychain failure must not silently generate a replacement key. Ardia's current safe-storage patch documents an actual data-loss failure of that kind. [Ardia safe-storage rationale][A14]

### Windows packaging

Keep the production launcher, matching engine DLLs and sandbox broker/target setup together. Validate renderer restrictions and process mitigations in the installed application, including GPU, network and utility processes. A working `--no-sandbox` build cannot pass. The checked Chromium loader's sandbox initialization is part of the retained bootstrap. [Windows bootstrap][C4]

Default installer plan: adapt Chromium's existing installer and updater machinery for jaal's product identity, signed feed and versioned application directory. Chromium documents a `mini_installer` build. A working jaal update service and feed are new work, not something an unbranded build supplies automatically. [Installer instructions][C13]

Sign and timestamp shipped PE files and the installer with the chosen Authenticode identity. Test install, interrupted update, repair, uninstall and retained user data on a clean Windows account. Register URL/file associations and expose the supported OS default-browser flow. Do not silently make jaal the default.

Test Windows credential storage under the intended installed identity, including supported app-bound encryption or an explicitly reviewed alternative. Changing branding, installation paths or elevation helpers can affect those flows; this integration is **unverified**. Test across signed upgrades before storing user passwords.

Maintain Mark-of-the-Web/quarantine behavior for downloads and validate dangerous-download handling. Keep crash dumps local by default; uploads require consent and scrubbing. Preserve matching symbols.

Widevine is its own licensing and final-binary verification gate on Windows and Mac. Ordinary code signing does not establish Widevine approval. Do not advertise Netflix or other protected-service parity until the actual released binary passes the service tests. [Widevine license][W19] [Ardia VMP note][A12]

### Updates and recovery

Use authenticated metadata and package signatures, with versioned directories and atomic activation. Keep a last-known-good binary for installation failures. Before a profile-format change, create a scoped backup or use a tested forward-recovery path. Never downgrade a live profile merely because the updater reverted the executable.

Separate browser updates, extension updates, content-blocker payload/filter updates and Chromium component updates in telemetry and tests. Verify each channel independently. Test offline startup, failed downloads, interrupted installation, corrupt packages, revoked signatures and a full UI/engine mismatch.

## 8. Milestones and gates

Implement in this order. Investigate service licensing and build capacity during M0; do not let those discoveries wait until release. Every milestone produces logs, exact build identities, screenshots where visual behavior matters, and an explicit list of unrun checks.

Before changes, record the existing `bun run check`, shared tests and relevant crate checks. Preserve every passing check after migration. All automated runs use disposable profiles under `target/profiles/<test-name>`, never a real browser profile. On the unattended Mac, keep fixtures outside Documents, Desktop and Downloads. Use controlled prompt fixtures for automation; reserve real OS prompts for an attended qualification run. Quit each launched app by its captured process identity and leave no waiting dialogs. [Repository test and safety rules][J12]

| Milestone | Deliverable | Pass-or-fail check |
|---|---|---|
| M0. Plain Chrome baseline | Owner-verified Windows plain build; a Mac build after storage/Xcode prerequisites. Record compatible toolchains and fresh disposable profiles. | Both native builds open the test pages with sandbox enabled. Renderer/GPU processes start. Exit cleanly. The existing Windows build is not declared passed by this plan. |
| M1. Library bootstrap and SDK | Engine launcher, versioned C ABI, Rust module, browser readiness and shutdown. Keep stock Chrome UI available as a diagnostic build. | Browser process loads GPUI once; helper processes never do. Reject mismatched ABI/resources before profile access. Commands and cancellation settle once. OS quit and cancelled quit work. |
| M2. Native host on both OSs | Views-backed root, embedded GPUI rendering, live native page, fix A overlay. Minimal window factory and `BrowserWindow` adapter. | Screenshot and operate both builds. Page input, IME, keyboard shortcuts, menu, file picker and a native modal dialog work. Drag/resize/minimize/fullscreen/mixed-DPI transitions retain placement and focus. No hidden Chrome window or unsandboxed shortcut. Failure blocks service UI work. |
| M3. Exact bake-off parity | Replace CEF behind the existing gpui-a UI behavior, keeping the CEF build available for comparison. | Same 5,000 shared rows, tab list, URL entry, new/close shortcuts, click-to-switch, blurred shadowed command bar and resizable window. Run existing shared tests and check/lint commands. Screenshots show actual overlap. No extra features enabled in the timed fixture. |
| M4. Lifecycle and browser state | Profiles, private windows, real tab adoption, transfer, session restore and crash recovery. | Test popup POST/opener/noopener/COOP cases, duplicate/back-forward history, beforeunload cancel, downloads during quit, renderer crash/reload, profile teardown and restart. Every live page appears once in the app and extensions' tab/window queries. |
| M5. Extension compatibility | Store flow, extension updates, action popups, side panels, permissions, settings and native messaging. | Install from the real Store on Mac and Windows; verify real update or a controlled signed update fixture plus a separately recorded Store-update check. Test representative content, password-manager, side-panel, downloader and DNR extensions. Late prompt answers grant nothing. Unverified live Store updates stay an open gate. |
| M6. Remaining essential browser features | Navigation/search, print/PDF, history/bookmarks/import, downloads, passwords/autofill/passkeys, site controls, media and OS integration from section 5. | Each essentials row passes on each shipped target. Test real keyboards, assistive technology and OS prompts. Hidden tests alone do not close IME, accessibility, audio, capture or visual checks. |
| M7. Privacy and content blocking | Audited selected patch series, provider policy, upstream blocker payload and any independent compatibility hooks. | Replay patches without fuzz; verify their behavior and permitted network destinations. Pass Helium blocker corpus, first-navigation and private-profile tests. Rerun all earlier compatibility checks. |
| M8. Performance acceptance | Comparable release builds of migrated gpui-a, its CEF baseline, plain Chromium/Chrome and Helium where versions/configurations can be aligned. | Five alternating valid runs per target/configuration. Report medians and spread. No unresolved repeatable regression on required metrics; noisy or mismatched comparisons are inconclusive, not passes. Inspect CPU/GPU traces separately to explain a claimed difference. |
| M9. Maintainability rehearsal | Full next-milestone rebase and one security refresh using only committed recipes. | Reconstruct the engine from the lock and patch series. Build both OSs, run contracts and affected upstream tests, assemble the unchanged UI, and record the actual work needed. No undocumented tree edits. |
| M10. Product UX | Spaces, pinned tabs, folders and profile UX on the proven engine, following the decided product phase. | Product interactions preserve Chrome/extension tab semantics and session recovery. Add product metadata migrations. Repeat essential and performance gates after meaningful UI changes. |
| M11. Signed distribution | Signed/notarized Mac app, signed Windows installer, verified updater, notices and source provenance. | Clean-machine install, real service-provider checks, browser default flow, sandbox, signed upgrade with seeded data, interrupted update and mismatch rejection. External-service or blocker gaps fail the broad parity claim. |

After M11, fix real-use bugs before AI work. A public alpha may disclose missing features, but it does not satisfy the stated full-parity goal. This preserves the repository's decided order rather than treating a successful engine demo as release readiness. [Jaal order][J1]

### Performance evidence rules

The Mac bake-off file supports the team's UI selection. It does not establish performance of this engine architecture. Its startup ranges overlap, startup is marked at load-end rather than a directly observed displayed paint, and resize is scripted. The results have no Helium switch measurement. Keep these distinctions in the new report. [Recorded results][J11] [Benchmark definitions][J2]

Before timing, inspect and update the measurement script. Record machine/OS/GPU/display, build flags, engine versions, workload, cache state, failures and background load. Do not benchmark beside the current Chromium build.

Measure cold launch to visible page, ten-tab total process memory, click-to-visible switch, sidebar frames, actual live resize and idle CPU/energy. Include the browser, renderers, GPU and utility processes. Distinguish RSS/private memory/physical footprint by OS; never compare those as the same metric.

Retain the old load-end and double-animation-frame marks for continuity, but add compositor/presentation or observed-frame evidence. A blank page, failed request or skipped renderer cannot count as a faster run. Compare enabled features as well as engines. The production browser must be measured with its required security, extensions and blocking enabled, after the exact bake-off fixture passes.

There are no new benchmark results in this plan. The proposed reduction in integration layers is not a performance claim.

## 9. Ranked risks and defaults

| Rank | Risk | Default and stop condition |
|---|---|---|
| 1 | Chrome code assumes a `BrowserView`, toolbar or valid native widget. Windows Aura/GPUI composition is not proven. | Keep a Views-backed frame and real adapters. Stop at M2 if focus, modal ownership or native composition fails. No production stubs. |
| 2 | Missing licensed or external services makes the broad parity promise false. | Investigate access during M0. Keep DRM, protection, translation, notifications, Cast and sync individually visible. Missing access blocks the corresponding claim. |
| 3 | New Chromium releases break the adapters faster than the team can maintain them. | Narrow typed hooks, stable C ABI, daily release awareness and early rebase rehearsal. Do not delay security updates behind cosmetic UI work. |
| 4 | Multiple tab models or ambiguous lifetimes lose tabs, corrupt restore, or grant permissions to the wrong document. | Chrome owns tabs. Use generations, revisioned transactions, one-shot prompt tokens, one profile per initial window and explicit shutdown barriers. |
| 5 | Safe storage, signing or profile migrations destroy saved data. | Freeze identity early. Test signed upgrades and missing-key failures. Keep destructive recovery explicit. Never silently reset. |
| 6 | Content blocking falls short of Helium or introduces an unmaintainable MV2 fork. | Evaluate upstream uBlock with pinned permissive compatibility patches. Test the full blocking corpus. If parity cannot be maintained, stop the parity claim and bring a concrete blocker-engine alternative for review. |
| 7 | GPUI's embedded mode mishandles AppKit, vsync, nested loops or accessibility. | Test real input and native dialogs on both OSs before service UI. Keep the GPUI fork documented and upstreamable. |
| 8 | Local build hardware cannot sustain release cadence. | Add storage and the required SDKs; measure an initial build and incremental relink. Prepare Depot jobs, but wait for the developer's go before cloud execution. |
| 9 | Overlays cause focus theft, move/resize lag or hidden security prompts. | Centralize overlay ownership and z-order. Exercise different DPI, fullscreen, multiple windows, blur settings and inactive-app transitions. |
| 10 | A permissive base is mistaken for permission to copy all fork code. | Implement independently against Chromium contracts. Track every imported patch and bundled component. Keep the current repo license until a deliberate license decision. |

### Where to improve on Ardia

- Start with Windows and Mac behind one C contract. Avoid a public ObjC-only hosting boundary and late OS abstraction.
- Use one profile per initial window and engine-owned tab state. Add richer product organization only after correctness.
- Guarantee asynchronous command replies. Protect GPUI from synchronous callbacks and native nested-loop reentrancy.
- Keep all modifications in one reproducible series. Test compiled outputs against the recorded source inputs.
- Keep crash diagnostics, security services and a release process from the start. Do not import unrelated optimization switches.
- Require unlocked, real-input and assistive-technology evidence. Ardia's own parity notes explicitly leave some of those unproven. [Ardia parity limits][A6]

### Where Ardia saves time

Its source identifies the useful Chrome seams, the need for a real Views widget, live popup adoption, and direct service calls. Its documented failures supply test cases for beforeunload, keychain access, prompt responses, window identity and tab-modal dialog anchoring. Its historical hidden acceptance results are useful test design evidence, not jaal results. [Architecture][A1] [Window host][A2] [Hook inventory][A10] [Parity][A6]

Take the design lessons only. Do not copy its code or patches into this engine. Ardia's distribution is GPL-3.0, even though some original files carry Apache-2.0 headers. The brief explicitly preserves jaal's future license choice. [Ardia license explanation][A15]

## 10. What was not checked

- No builds, app launches, tests, benchmarks or signing operations ran for this plan.
- No file under `C:/src/chromium` was written. Its `out/` tree and running build were not inspected or controlled.
- The proposed GPUI borrowed-window mode and Windows page-host arrangement are unimplemented and unverified.
- No live CEF, Ardia, Store installation/update, DRM playback, native messaging, OS permission, accessibility or service-provider flow was exercised.
- Ardia documents contain historical results and partial checks. They were not independently reproduced.
- The Intel fork's checked main branch does not demonstrate Intel support. Unpublished/private Intel work is unknown.
- No complete audit of every Ardia function, Chromium service, privacy patch dependency or third-party license was performed. The reviewed code establishes design seams, not full compatibility.
- Depot capacity for this account, actual build costs, Mac free storage, signing credentials, Widevine approval and Google service access remain unverified.
- Vivaldi's source package and proprietary Arc/Opera hosting internals were not audited.
- The other planner's output was not opened. Only this output file was created or edited.

## Sources

Local links identify the checked snapshots listed at the start. Web links were checked on 2026-11-09; pin exact revisions before importing any source. Line links identify the relevant starting location, not a claim about unchanged future line numbers.

[J1]: C:/src/jaal-council/jaal-bakeoff/docs/plan.md
[J2]: C:/src/jaal-council/jaal-bakeoff/docs/bakeoff.md
[J3]: C:/src/jaal-council/jaal-bakeoff/third_party/gpui/src/platform/windows/platform.rs:308
[J4]: C:/src/jaal-council/jaal-bakeoff/third_party/gpui/src/platform/mac/platform.rs:474
[J5]: C:/src/jaal-council/jaal-bakeoff/bakeoff/gpui-cef/src/pump.rs:1
[J6]: C:/src/jaal-council/jaal-bakeoff/bakeoff/gpui-cef/src/nsapp.rs:1
[J7]: C:/src/jaal-council/jaal-bakeoff/bakeoff/gpui-a/src/main.rs:40
[J8]: C:/src/jaal-council/jaal-bakeoff/bakeoff/gpui-a/src/overlay.rs:45
[J9]: C:/src/jaal-council/jaal-bakeoff/bakeoff/gpui-cef/src/gpui_patch.rs:1
[J10]: C:/src/jaal-council/jaal-bakeoff/third_party/gpui/JAAL.md
[J11]: C:/src/jaal-council/jaal-bakeoff/bakeoff/results/2026-11-09-Mac.md
[J12]: C:/src/jaal-council/jaal-bakeoff/AGENTS.md
[A1]: C:/src/jaal-council/ardia-upstream/docs/agent-brief.md:12
[A2]: C:/src/jaal-council/ardia-upstream/engine/ardiacore/src/ardia/core/ac_window_host.mm:84
[A3]: C:/src/jaal-council/ardia-upstream/docs/engine-build.md:285
[A4]: C:/src/jaal-council/ardia-upstream/docs/ardiacore-spike.md
[A5]: C:/src/jaal-council/ardia-upstream/engine/patches/series
[A6]: C:/src/jaal-council/ardia-upstream/docs/ardiacore-parity.md
[A7]: C:/src/jaal-council/ardia-upstream/engine/chromium/src/chrome/browser/ardia/public/ac_engine.h:1
[A8]: C:/src/jaal-council/ardia-upstream/engine/ardiacore/src/ardia/core/ac_main_delegate.cc:158
[A9]: C:/src/jaal-council/ardia-upstream/engine/ardiacore/src/ardia/core/ac_window_host_close.mm
[A10]: C:/src/jaal-council/ardia-upstream/docs/engine-build.md:128
[A11]: C:/src/jaal-council/ardia-upstream/engine/ardiacore/src/ardia/core/ardiacore_tab.mm:236
[A12]: C:/src/jaal-council/ardia-upstream/docs/engine-build.md:406
[A13]: C:/src/jaal-council/ardia-upstream/docs/engine-build.md:319
[A14]: C:/src/jaal-council/ardia-upstream/docs/engine-build.md:44
[A15]: C:/src/jaal-council/ardia-upstream/README.md:227
[I1]: C:/src/jaal-council/ardia-intel-mac/engine/patches/build/gn_defines.sh:10
[I2]: C:/src/jaal-council/ardia-intel-mac/apps/browser/macos/Ardia.xcodeproj/project.pbxproj:381
[I3]: C:/src/jaal-council/ardia-intel-mac/README.md:85
[C1]: C:/src/chromium/src/ui/gfx/native_ui_types.h:123
[C2]: C:/src/chromium/src/docs/mac_arm64.md:1
[C3]: C:/src/chromium/src/chrome/browser/ui/browser.cc:267
[C4]: C:/src/chromium/src/chrome/app/main_dll_loader_win.cc:277
[C5]: C:/src/chromium/src/chrome/app/chrome_main.cc:103
[C6]: C:/src/chromium/src/chrome/browser/chrome_browser_application_mac.mm:40
[C7]: C:/src/chromium/src/ui/views/controls/webview/webview.cc:598
[C8]: C:/src/chromium/src/ui/views/widget/desktop_aura/desktop_window_tree_host_win.cc:250
[C9]: C:/src/chromium/src/ui/views/widget/widget.h:338
[C10]: C:/src/chromium/src/docs/windows_build_instructions.md:13
[C11]: C:/src/chromium/src/docs/mac_build_instructions.md:13
[C12]: C:/src/chromium/src/chrome/installer/mac/signing/README.md:1
[C13]: C:/src/chromium/src/docs/windows_build_instructions.md:457
[W1]: https://github.com/chromiumembedded/cef/issues/2969
[W2]: https://github.com/chromiumembedded/cef/blob/master/include/internal/cef_types_runtime.h
[W3]: https://github.com/chromiumembedded/cef/blob/master/include/views/cef_browser_view.h
[W4]: https://github.com/electron/website/blob/main/docs/latest/api/extensions.md
[W5]: https://github.com/brave/brave-core/blob/master/docs/best-practices/chromium-src-overrides.md
[W6]: https://github.com/brave/brave-browser/wiki/Chromium-rebases
[W7]: https://github.com/ungoogled-software/ungoogled-chromium/blob/master/docs/design.md
[W8]: https://github.com/ungoogled-software/ungoogled-chromium/blob/master/LICENSE
[W9]: https://github.com/imputnet/helium
[W10]: https://github.com/Alex313031/Thorium/blob/main/README.md
[W11]: https://help.vivaldi.com/desktop/privacy/is-vivaldi-open-source/
[W12]: https://arc.net/faq
[W13]: https://help.opera.com/en/latest/customization/
[W14]: https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2profile.addbrowserextensionasync?view=webview2-dotnet-1.0.3065.39
[W15]: https://chromium.googlesource.com/chromium/src/+/HEAD/docs/process/release_cycle.md
[W16]: https://raw.githubusercontent.com/ungoogled-software/ungoogled-chromium/master/patches/core/ungoogled-chromium/extensions-manifestv2.patch
[W17]: https://github.com/gorhill/uBlock/blob/master/LICENSE.txt
[W18]: https://www.chromium.org/developers/how-tos/api-keys/
[W19]: https://chromium.googlesource.com/chromium/src/third_party/+/master/widevine/LICENSE
[W20]: https://www.widevine.com/solutions/widevine-drm
[W21]: https://raw.githubusercontent.com/ungoogled-software/ungoogled-chromium/master/patches/series
[W22]: https://depot.dev/docs/github-actions/runner-types
