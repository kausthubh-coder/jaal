// `bun scripts/bundle.ts <crate> [--release]`: builds the macOS app bundle
// at target/bundle/<crate>.app, the path bakeoff/bench/run.ts launches.
//
// bundle-cef-app on macOS has no --release flag: it always runs
// `cargo build` with the dev profile and bundles target/debug. For
// --release we give the dev profile the release profile's settings and point
// it at its own target dir, so the debug build isn't overwritten.
//
// The bundle is then ad-hoc signed: LaunchServices (`open`) refuses to start
// an unsigned bundle whose binaries changed after linking.

import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const crate = process.argv[2];
const release = process.argv.includes("--release");
if (!crate || crate.startsWith("-")) {
  console.error("usage: bun scripts/bundle.ts <crate> [--release]");
  process.exit(2);
}

const root = join(import.meta.dir, "..");
const cefPath = process.env.CEF_PATH ?? join(homedir(), ".local/share/cef");
const onPath = Bun.which("bundle-cef-app");
const bundler = onPath ?? join(homedir(), ".cargo/bin/bundle-cef-app");
if (!existsSync(bundler)) {
  console.error("bundle-cef-app not found. Run: cargo install cef --features build-util");
  process.exit(1);
}

const env: Record<string, string> = { ...process.env, CEF_PATH: cefPath } as Record<string, string>;
if (release) {
  Object.assign(env, {
    CARGO_TARGET_DIR: join(root, "target/release-bundle"),
    CARGO_PROFILE_DEV_OPT_LEVEL: "3",
    CARGO_PROFILE_DEV_DEBUG: "false",
    CARGO_PROFILE_DEV_STRIP: "debuginfo",
    CARGO_PROFILE_DEV_DEBUG_ASSERTIONS: "false",
    CARGO_PROFILE_DEV_OVERFLOW_CHECKS: "false",
    CARGO_PROFILE_DEV_INCREMENTAL: "false",
    CARGO_PROFILE_DEV_CODEGEN_UNITS: "16",
  });
}

function run(cmd: string[]) {
  const r = Bun.spawnSync(cmd, { cwd: root, env, stdout: "inherit", stderr: "inherit" });
  if (r.exitCode !== 0) {
    console.error(`failed: ${cmd.join(" ")}`);
    process.exit(r.exitCode ?? 1);
  }
}

const out = join(root, "target/bundle");
run([bundler, crate, "-o", out]);
const app = join(out, `${crate}.app`);
run(["codesign", "--force", "--deep", "--sign", "-", app]);
console.log(`${release ? "release" : "debug"} bundle: ${app}`);
