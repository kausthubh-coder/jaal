// `bun run doctor`: is this checkout ready to build the bake-off?
// Run it first when a build fails in a way that looks like setup.

import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const results: [string, boolean, string][] = [];

function tool(name: string, args: string[]) {
  const r = Bun.spawnSync([name, ...args], { stderr: "pipe" });
  const out = (r.stdout.toString() || r.stderr.toString()).split("\n")[0].trim();
  results.push([name, r.exitCode === 0, r.exitCode === 0 ? out : `missing (${out || "not on PATH"})`]);
}

tool("cargo", ["--version"]);
tool("cmake", ["--version"]);
tool("ninja", ["--version"]);

const cefPath = process.env.CEF_PATH ?? join(homedir(), ".local/share/cef");
const cefOk = existsSync(join(cefPath, "archive.json"));
results.push([
  "CEF",
  cefOk,
  cefOk ? cefPath : `not found at ${cefPath}. Run: cargo run -p export-cef-dir -- --force ${cefPath} in a cef-rs checkout`,
]);

const pagesOk = existsSync(join(import.meta.dir, "..", "bakeoff", "pages", "page-10.html"));
results.push(["test pages", pagesOk, pagesOk ? "bakeoff/pages" : "missing. Run: bun run pages"]);

let failed = false;
for (const [name, ok, detail] of results) {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}: ${detail}`);
  if (!ok) failed = true;
}
process.exit(failed ? 1 : 0);
