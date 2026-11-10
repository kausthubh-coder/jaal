# jaal

jaal is an Arc-style browser on Chromium for Mac and Windows, with full Chrome extensions and the Web Store, and optional AI from Codex or Claude. Users care that it's fast, light and polished. Right now the repo holds the UI bake-off: one tiny browser built three ways on prebuilt CEF, so we can pick the UI toolkit before building our own engine. Stack: Rust (CEF through the `cef` crate, GPUI), bun for scripts.

Read `docs/plan.md` for the whole plan and `docs/bakeoff.md` for the bake-off rules.

## Words we use

- **you**: the agent reading this file.
- **me**: the developer you are working for.
- **user**: the person who uses the browser. Not me.
- **engine**: Chromium (later Helium plus our patches) built as a library. In the bake-off, prebuilt CEF stands in for it.
- **variant**: one of the three bake-off browsers (`web-ui`, `gpui-a`, `gpui-b`).
- **fix A / fix B**: the two ways a GPUI variant gets UI over the page. A is a second transparent window over the page; B draws the page as a texture inside GPUI. See `docs/research/2026-11-07-how-native-apps-draw.html`.
- **Helium**: the ungoogled Chromium browser we compare against and later build on.

## Map

| Where | What lives there |
|---|---|
| `bakeoff/shared` | Tab list, the 5,000 fake sidebar rows, test page URLs, bench marks and bench modes, URL-field parsing. Every variant uses it |
| `bakeoff/pages` | The 10 static test pages, written by `scripts/make-pages.ts` |
| `bakeoff/web-ui`, `bakeoff/gpui-a`, `bakeoff/gpui-b` | One crate per variant |
| `bakeoff/gpui-cef` | GPUI + CEF glue both GPUI variants use: NSApp patch, external message pump, the CEF client, CEF browsers as child views (gpui-a), the text field, posted NSEvents for the demos |
| `third_party/gpui` | gpui 0.2.2, vendored with a small patch both GPUI variants build against (BGRA page textures for gpui-b). What changed and why: `third_party/gpui/JAAL.md` |
| `bakeoff/bench` | The scripts that run the tests and write `bakeoff/results` |
| `docs/` | Plan, bake-off rules, research |
| `scripts/` | doctor and page generator |
| `engine/` | The engine: our layer inside Chrome (`src/jaal`), the C header (`public/jaal_engine.h`), Chrome patches (`patches/`), the Chromium lock and its scripts. Plan: `docs/engine.md` |
| `crates/jaal-engine-sys`, `crates/jaal-engine` | Rust bindings for `jaal_engine.h` (generated) and the safe API over them |
| `crates/jaal-ui-hello` | The smallest UI library (`jaal_ui.dll`): one window, one tab. The engine acceptance run drives it |

Shared code goes in `bakeoff/shared` only when two variants really use it.

## Commands

- Doctor: `bun run doctor` (cargo, cmake, ninja, CEF at `$CEF_PATH`, test pages). Run it first when a build fails in a way that looks like setup.
- CEF binaries: `CEF_PATH` defaults to `~/.local/share/cef` (CEF 156, from cef-rs's `export-cef-dir`). On macOS also set `DYLD_FALLBACK_LIBRARY_PATH="$CEF_PATH:$CEF_PATH/Chromium Embedded Framework.framework/Libraries"`.
- Bundle and run a variant on macOS: `bun scripts/bundle.ts <crate>` (add `--release` for the bench), then `target/bundle/<crate>.app/Contents/MacOS/<crate>`. CEF needs the bundle with its helper apps; `cargo run` alone won't start it. The script calls `bundle-cef-app` (in `~/.cargo/bin`, which may not be on PATH), which has no `--release` flag on macOS, and ad-hoc signs the bundle so `open` will launch it.
- Launched from a shell, the app stays inactive (macOS won't let a background process bring it to the front) and Stage Manager may park its window in the side strip. Launch with `open -n -a target/bundle/<crate>.app` to have it placed on stage. `JAAL_DEMO=1` runs a GPUI variant's scripted session for screenshots: gpui-a's (tabs, command bar, move, resize, scroll) in `bakeoff/gpui-a/src/demo.rs`, gpui-b's (command bar, wheel-scrolling the page, clicking a link and typing into the page, tabs) in `bakeoff/gpui-b/src/demo.rs`.
- Test: `cargo test -p <crate>`.
- Lint the crate you changed: `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo fmt -p <crate>`.
- Full check (CI runs this): `bun run check`.
- Engine, Windows only for now. The Chromium checkout lives outside the repo (`C:\src\chromium\src`, or `JAAL_CHROMIUM_SRC`) at `engine/chromium.lock`'s commit.
  - `bun engine/scripts/apply.ts` copies `engine/src` into the checkout and applies `engine/patches/series`. Edit in the repo, never in the checkout.
  - `bun engine/scripts/check.ts` fails if the checkout differs from what the repo says. Run it before every engine PR.
  - `bun engine/scripts/build.ts` builds `out/Release` (about 2 to 4 minutes after a change to `//jaal`; a first build takes hours).
  - `cargo build -p jaal-ui-hello --release`, plus `--features wrong-abi --target-dir target/wrong-abi`, then `bun engine/scripts/acceptance.ts` runs M1's checks against the real engine. It takes the foreground for a few seconds.
  - After changing `jaal_engine.h`, run `bun engine/scripts/bindgen.ts` (needs `bindgen-cli` and libclang; `--check` tells you if the committed bindings are stale).

## Proving a change works

- Done means you saw it work, not that it compiles. Launch the bundle and screenshot it: `screencapture -x <file>.png`, then look at the image.
- The Mac mini has no one at its screen. Never leave a dialog waiting for a click. Quit every app you launch, by the PID you started.
- A test calls the code the way its users do and asserts the exact result.
- Never edit or delete a test to make it pass. If the test is wrong, say so and explain why.
- A skipped check is not a pass. Report what you ran, what you saw, and what you did not check.
- Benchmark numbers: follow the `benchmark-checklist` skill. Same machine, nothing else heavy running, 5 runs, report median and spread.

## Working with me

- A question gets an answer, not an action.
- When I ask for a plan, plan. Don't start building until I say go.
- Extend what exists. Propose a rewrite only in a plan, and wait.

## How we write code

- Boring and explicit over clever. Names you can grep.
- Comments say why, never what. Keep the why next to weird code (CEF and GPUI have plenty).
- Every variant implements the same features listed in `docs/bakeoff.md`, no more. A variant that does extra work skews the numbers.
- Smallest change that solves the real problem.

## Rules and what enforces them

| Rule | Enforced by |
|---|---|
| Code is formatted and clippy-clean | `bun run lint` |
| Shared tab and row logic behaves like Chrome's | `cargo test -p jaal-bakeoff-shared` |
| Test pages exist and are static | `every_test_page_exists` test; nothing checks "static" yet |

When I correct you for the same thing twice, add the rule here and make something enforce it (`/correct`).

## Safety

- Never touch my real browser profiles. Each variant keeps its cache and profile under `target/profiles/<variant>`.
- Never read, list or write `~/Documents`, `~/Desktop` or `~/Downloads` on the Mac mini: the permission prompt hangs forever.
- Cloud builds (Depot) need my go.
- Stop only processes you started, by the PID you captured.
- Git: open a PR and wait for CI and my review.
