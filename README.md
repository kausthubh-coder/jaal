# jaal

An Arc-style browser on Chromium for Mac and Windows, with full Chrome extensions and optional AI.

It's early. This repo holds the UI bake-off: the same tiny browser built three ways on prebuilt CEF, measured against Helium. The winner becomes jaal's UI. See [docs/plan.md](docs/plan.md) and [docs/bakeoff.md](docs/bakeoff.md).

## Build

You need Rust, cmake, ninja and bun.

```sh
bun run doctor
cargo test --workspace
```

## License

GPL-3.0. jaal will build on Helium, which is GPL-3.0.
