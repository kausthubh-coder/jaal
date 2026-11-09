# Plan

jaal is an Arc-style browser on Chromium with full Chrome extensions and the Web Store, for Mac and Windows. It must never be worse than Chrome or Helium at anything a browser does. AI (Codex or Claude) is optional and comes last.

## How it's built

- **UI: gpui-a.** Zed's GPUI draws the window. Each page is Chrome's own native view on top of it, so input, menus and popups work as in Chrome. Anything over the page (command bar, extension popups, prompts) is a see-through child window glued to the main one. The bake-off picked it: least memory, real OS blur, no input forwarding ([bakeoff.md](bakeoff.md), [results](../bakeoff/results/2026-11-09-Mac.md)).
- **Engine.** Chromium plus our own patches, which turn it into a library: no Chrome window UI, each tab's page handed to our app, and commands like `tabs.open`. Ardia does the same on the Mac, and its docs are the best map we have. It starts from plain Chromium (see below). The full plan is [engine.md](engine.md).
- **Builds.** Each engine build is published as a download. The laptop builds Windows; Depot builds Mac and the official builds. How often we move to a new Chrome version is decided when jaal is first published. Everyday UI work only downloads it. Engine changes are made in a local copy, which rebuilds in minutes after its first long build.

## Decided: plain Chromium, not Helium

| | Helium | Plain Chromium plus picked privacy patches |
|---|---|---|
| License | GPL-3.0, so jaal must be GPL-3.0 | Chromium's BSD; jaal picks its own license |
| Privacy work | Done for us (ungoogled-chromium plus Helium's own) | We take the patches we want from ungoogled-chromium (BSD) and keep them up to date |
| Web Store and updates | Routed through Helium's servers, which Ardia turned off and sent straight to Google | Straight to Google, as Chrome does |
| Upstreams to follow | Two (Chromium and Helium) | One |

Decided on 2026-11-09: plain Chromium plus picked patches. jaal's own license can be chosen later; the repo stays GPL-3.0 until then.

## Order

1. **Bake-off** on prebuilt CEF. Done on the Mac; gpui-a is being ported to Windows so it can be tried there.
2. **Engine**: plain Chromium plus picked privacy patches and the library patches ([engine.md](engine.md)). Engine work happens on the Windows laptop, Windows first; Depot builds Mac.
3. **gpui-a on the engine**: replace CEF.
4. **Essentials, at least as good as Chrome and Helium** (checklist below).
5. **Good UX**: sidebar, tab management, profiles, spaces, pinned tabs, folders.
6. **Ship**: signed builds, auto-update, Windows installer.
7. **Fix bugs** from real use.
8. **AI.**

## Essentials checklist

Everything Chrome and Helium give a user. Most of it is Chrome's own code; our work is the UI around it.

- Tabs and windows: new, close, reopen closed, drag between windows, private windows, session restore after a crash or restart.
- Navigation: address bar with search and suggestions, back and forward, reload, find in page, zoom, print, the PDF viewer, view source, DevTools.
- Data: history, bookmarks with import from Chrome and Arc, downloads with their prompts.
- Passwords and autofill: save and fill passwords, passkeys, addresses and cards, password generation.
- Extensions: the Web Store, toolbar icons and popups, side panel extensions, install and permission prompts, extension context-menu items.
- Site controls: permission prompts (camera, mic, location, notifications), site settings, clearing cookies and data, HTTPS-first, pop-up blocking.
- Media: picture in picture, fullscreen, media controls, autoplay rules.
- Profiles, default browser, keyboard shortcuts, spellcheck, accessibility, memory and energy saving.
- Content blocking at least as good as Helium's built-in uBlock Origin.

Some Chrome features only work with Google's permission, so a small browser needs its own version:

| Chrome feature | Why we can't just turn it on | Plan |
|---|---|---|
| Sync with a Google account | Google closed Chromium sync to other browsers in 2021 | Our own sync. Ardia uses encrypted files in your own iCloud Drive folder |
| Netflix and other DRM video (Widevine) | Google licenses it per browser | Apply for the license; until then, Ardia's trick on the Mac: open those sites in Safari's engine |
| Safe Browsing, Translate, Cast | Need Google API keys or deals | Look at what Brave and Vivaldi do |

## Research

- [research/2026-10-29-ai-chromium-browser.html](research/2026-10-29-ai-chromium-browser.html): the first look at building an AI Chromium browser.
- [research/2026-11-07-how-native-apps-draw.html](research/2026-11-07-how-native-apps-draw.html): main loops, who draws what, and the three ways to draw over a Chrome page.
- [research/2026-11-09-engine-plan-fable.md](research/2026-11-09-engine-plan-fable.md) and [research/2026-11-09-engine-plan-astra.md](research/2026-11-09-engine-plan-astra.md): the two engine plans that [engine.md](engine.md) merges, with their citations.
