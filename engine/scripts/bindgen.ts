// `bun engine/scripts/bindgen.ts [--check]`: regenerate the Rust bindings for
// engine/public/jaal_engine.h into crates/jaal-engine-sys/src/bindings.rs.
//
// The bindings are committed, so building the Rust side never needs libclang.
// --check fails when the committed file is stale. Needs bindgen-cli
// (`cargo install bindgen-cli`) and libclang; set LIBCLANG_PATH if bindgen
// can't find it (on Windows, `pip install libclang` ships one).

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ENGINE, REPO } from "./lib";

const header = join(ENGINE, "public", "jaal_engine.h");
const target = join(REPO, "crates", "jaal-engine-sys", "src", "bindings.rs");

const r = Bun.spawnSync(
  [
    "bindgen",
    header,
    "--allowlist-item",
    "jaal_.*|JAAL_.*",
    "--with-derive-default",
    // Only plain-value types compare; the call table holds function pointers,
    // whose addresses aren't meaningful to compare.
    "--with-derive-custom-struct",
    "jaal_handle|jaal_rect=PartialEq,Eq,Hash",
    "--no-prepend-enum-name",
    // Newtype enums, not Rust enums: a newer engine may send a value this
    // build doesn't know, which a Rust enum can't hold.
    "--default-enum-style=newtype",
    "--use-core",
  ],
  { stdout: "pipe", stderr: "pipe" },
);
if (r.exitCode !== 0) {
  console.error(r.stderr.toString());
  process.exit(1);
}
const generated = r.stdout.toString().replaceAll("\r\n", "\n");

if (process.argv.includes("--check")) {
  const current = readFileSync(target, "utf8").replaceAll("\r\n", "\n");
  if (current !== generated) {
    console.error(`FAIL ${target} is stale. Run: bun engine/scripts/bindgen.ts`);
    process.exit(1);
  }
  console.log("ok: bindings match jaal_engine.h");
} else {
  writeFileSync(target, generated);
  console.log(`wrote ${target}`);
}
