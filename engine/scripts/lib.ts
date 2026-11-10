// Shared paths and helpers for the engine scripts.

import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

export const ENGINE = resolve(import.meta.dir, "..");
export const REPO = resolve(ENGINE, "..");

// The Chromium checkout stays outside the repo. JAAL_CHROMIUM_SRC overrides
// the per-OS default.
export function chromiumSrc(): string {
  const fromEnv = process.env.JAAL_CHROMIUM_SRC;
  if (fromEnv) return fromEnv;
  return process.platform === "win32" ? "C:\\src\\chromium\\src" : join(homedir(), "chromium", "src");
}

export function lock(): { version: string; commit: string } {
  const text = readFileSync(join(ENGINE, "chromium.lock"), "utf8");
  const value = (key: string) => {
    const match = text.match(new RegExp(`^${key} = "([^"]+)"`, "m"));
    if (!match) throw new Error(`chromium.lock has no ${key}`);
    return match[1];
  };
  return { version: value("version"), commit: value("commit") };
}

// Patch paths relative to engine/patches, in application order.
export function series(): string[] {
  return readFileSync(join(ENGINE, "patches", "series"), "utf8")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
}

// Chromium paths a patch touches, from its `+++ b/<path>` lines.
export function touchedFiles(patch: string): string[] {
  const text = readFileSync(join(ENGINE, "patches", patch), "utf8");
  return [...text.matchAll(/^\+\+\+ b\/(.+)$/gm)].map((m) => m[1].trim());
}

export function run(cmd: string[], cwd: string, opts: { allowFail?: boolean } = {}) {
  const r = Bun.spawnSync(cmd, { cwd, stdout: "pipe", stderr: "pipe" });
  const out = r.stdout.toString();
  const err = r.stderr.toString();
  if (r.exitCode !== 0 && !opts.allowFail) {
    throw new Error(`${cmd.join(" ")} failed in ${cwd} (exit ${r.exitCode}):\n${err || out}`);
  }
  return { ok: r.exitCode === 0, out, err };
}

// Files jaal copies into the tree: engine/src/<rel> -> <src>/<rel>, and the
// public header into //jaal/public.
export function ownFiles(src: string): { from: string; to: string }[] {
  const files: { from: string; to: string }[] = [];
  const glob = new Bun.Glob("**/*");
  for (const rel of glob.scanSync({ cwd: join(ENGINE, "src"), onlyFiles: true })) {
    files.push({ from: join(ENGINE, "src", rel), to: join(src, rel) });
  }
  files.push({
    from: join(ENGINE, "public", "jaal_engine.h"),
    to: join(src, "jaal", "public", "jaal_engine.h"),
  });
  return files;
}

export function checkTreeCommit(src: string) {
  if (!existsSync(join(src, ".git"))) throw new Error(`${src} is not a Chromium checkout`);
  const head = run(["git", "rev-parse", "HEAD"], src).out.trim();
  const want = lock().commit;
  if (head !== want) {
    throw new Error(`${src} is at ${head}, chromium.lock wants ${want} (${lock().version})`);
  }
}

// Tracked files in the checkout that differ from its HEAD commit. Compared
// with HEAD, not the index: a staged edit is still an edit the build compiles.
export function modifiedTrackedFiles(src: string): string[] {
  return run(["git", "diff", "--name-only", "HEAD"], src)
    .out.split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
}
