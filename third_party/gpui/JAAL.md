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

Everything else is upstream. Keep changes small and marked, so moving to a
newer gpui means re-applying a short list.
