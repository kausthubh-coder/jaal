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
| `bakeoff/shared` | Tab list, the 5,000 fake sidebar rows, test page URLs. Every variant uses it |
| `bakeoff/pages` | The 10 static test pages, written by `scripts/make-pages.ts` |
| `bakeoff/web-ui`, `bakeoff/gpui-a`, `bakeoff/gpui-b` | One crate per variant |
| `bakeoff/bench` | The scripts that run the tests and write `bakeoff/results` |
| `docs/` | Plan, bake-off rules, research |
| `docs/design/jaal.html` | The look of jaal on one page: the window and its popovers, settings, and every control, in each space, theme and platform. Its CSS variables are the color tokens |
| `scripts/` | doctor and page generator |

Shared code goes in `bakeoff/shared` only when two variants really use it.

## Commands

- Doctor: `bun run doctor` (cargo, cmake, ninja, CEF at `$CEF_PATH`, test pages). Run it first when a build fails in a way that looks like setup.
- CEF binaries: `CEF_PATH` defaults to `~/.local/share/cef` (CEF 156, from cef-rs's `export-cef-dir`). On macOS also set `DYLD_FALLBACK_LIBRARY_PATH="$CEF_PATH:$CEF_PATH/Chromium Embedded Framework.framework/Libraries"`.
- Bundle and run a variant on macOS: `bundle-cef-app <crate> -o target/bundle`, then `target/bundle/<crate>.app/Contents/MacOS/<crate>`. CEF needs the bundle with its helper apps; `cargo run` alone won't start it.
- Test: `cargo test -p <crate>`.
- Lint the crate you changed: `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo fmt -p <crate>`.
- Full check (CI runs this): `bun run check`.

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
