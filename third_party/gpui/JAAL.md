# gpui 0.2.2, vendored

This is gpui 0.2.2 from crates.io, used by the workspace through
`[patch.crates-io]` in the root `Cargo.toml`, so both GPUI variants build
against the same copy.

Changes from upstream, each marked `jaal:` in the code:

- `Cargo.toml`: the `[[example]]` and `[[test]]` tables are gone, since the
  examples and tests aren't vendored.

Everything else is upstream. Keep changes small and marked, so moving to a
newer gpui means re-applying a short list.
