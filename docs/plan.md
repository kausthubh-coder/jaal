# Plan

jaal is an Arc-style browser on Chromium with full Chrome extensions and the Web Store, for Mac and Windows. AI (Codex or Claude) is optional and comes after the browser works.

## How it's built

- **Engine.** Helium (an ungoogled Chromium) plus our own patches, which turn Chromium into a library. The engine has no Chrome window UI, hands each tab's page to our app, and takes commands like `tabs.open`. Ardia does the same thing on the Mac, and its docs are the best map we have.
- **App.** Our own window, sidebar and command bar, drawn by whichever toolkit wins the bake-off.
- **Builds.** Depot builds the engine for Mac and Windows about once a month, when Chrome updates, and publishes it as a download. Everyday UI work only downloads that engine. Engine changes are made in a local copy, which rebuilds in minutes after its first long build.

## Order

1. UI bake-off on prebuilt CEF ([bakeoff.md](bakeoff.md)). Mac first, then Windows.
2. Engine: Helium plus the library patches, built on the Mac mini first, then on Depot.
3. Rebuild the winning UI on our engine.
4. The Arc features: spaces, pinned tabs, split view, command bar, sync.
5. AI.

## Research

- [research/2026-10-29-ai-chromium-browser.html](research/2026-10-29-ai-chromium-browser.html): the first look at building an AI Chromium browser.
- [research/2026-11-07-how-native-apps-draw.html](research/2026-11-07-how-native-apps-draw.html): main loops, who draws what, and the three ways to draw over a Chrome page.
