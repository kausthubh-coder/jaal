# gpui 0.2.2, vendored

This is gpui 0.2.2 from crates.io, used by the workspace through
`[patch.crates-io]` in the root `Cargo.toml`, so both GPUI variants build
against the same copy.

Changes from upstream, each marked `jaal:` in the code:

- `Cargo.toml`: the `[[example]]` and `[[test]]` tables are gone, since the
  examples and tests aren't vendored.
- BGRA surfaces (`Window::paint_surface_clipped`, `scene.rs`,
  `metal_renderer.rs`, `shaders.metal`): upstream's surface element only
  takes NV12 video frames and asserts on anything else. Chromium's
  offscreen pages arrive as BGRA IOSurfaces, so gpui-b needs a BGRA path,
  sampled in place with no copy, cut to a rounded clip (upstream has a TODO
  for surface corner radii), and an optional one-pass blur so the page can
  be drawn again, blurred, behind the command bar. NV12 drawing is
  unchanged.

- `src/taffy.rs`: two float literals typed as `f32`. Rust 1.99 warns about
  the untyped ones, and as a path dependency gpui's warnings now show.

- Windows child windows below child HWNDs (`directx_renderer.rs`): the
  DirectComposition target is created not topmost, so child windows of a GPUI
  window (gpui-a's CEF pages) show above GPUI's drawing instead of under it.

- Embedded mode (`Application::run_embedded`, `Platform::run_embedded`,
  Windows only so far): GPUI starts on a thread whose message loop belongs
  to a host, here Chromium's message pump in the jaal engine, and returns
  instead of running `GetMessageW`. The host's loop already dispatches this
  thread's window messages. What changes with it, all in
  `platform/windows`:
  - `platform.rs`: `run_embedded` does what `run` does (the launch callback,
    the vsync thread) without the loop, and sets `EMBEDDED`. The
    `Application` is leaked, because nothing blocks to keep it alive.
  - `events.rs`, keys: Chromium's loop calls `TranslateMessage` on every
    message before dispatching it. GPUI's own loop doesn't, and translates a
    key down itself when nothing handled it. Embedded mode never translates,
    and removes the queued `WM_CHAR` (or `WM_SYSCHAR`) when GPUI handled the
    key. Without this every typed character arrives twice.
  - `events.rs`, child windows: the engine makes a GPUI window a `WS_CHILD`
    of its own window. A child never gets `WM_ACTIVATE` or `WM_DPICHANGED`,
    so `WM_SETFOCUS` and `WM_KILLFOCUS` drive the window's active state and
    `WM_DPICHANGED_AFTERPARENT` updates its scale. Windows sends the mouse
    wheel to the window with keyboard focus, so a child with focus passes on
    a wheel whose pointer is outside it, and the host's content scrolls.
  - `window.rs`: `activate` on a child window only takes keyboard focus. The
    host places and shows it.

Everything else is upstream. Keep changes small and marked, so moving to a
newer gpui means re-applying a short list.
