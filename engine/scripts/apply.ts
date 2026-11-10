// `bun engine/scripts/apply.ts`: put jaal into the Chromium checkout.
//
// 1. Checks the tree is at chromium.lock's commit.
// 2. Copies engine/src into the tree (//jaal), writing only files that
//    changed so the build doesn't recompile what it doesn't need to.
// 3. Applies patches/series in order. A patch already in the tree is skipped;
//    one that needs fuzz to apply fails the step.
//
// Safe to run again after editing engine/src or a patch.

import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { ENGINE, checkTreeCommit, chromiumSrc, ownFiles, run, series } from "./lib";

const src = chromiumSrc();
checkTreeCommit(src);

const files = ownFiles(src);
let copied = 0;
for (const { from, to } of files) {
  const data = readFileSync(from);
  if (existsSync(to) && readFileSync(to).equals(data)) continue;
  mkdirSync(dirname(to), { recursive: true });
  writeFileSync(to, data);
  copied++;
}

// Remove files deleted from engine/src, so the tree matches it exactly.
const wanted = new Set(files.map((f) => relative(src, f.to)));
function prune(dir: string) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) prune(full);
    else if (!wanted.has(relative(src, full))) {
      rmSync(full);
      console.log(`removed ${relative(src, full)}`);
    }
  }
}
prune(join(src, "jaal"));
console.log(`//jaal: ${copied} of ${files.length} files updated`);

for (const patch of series()) {
  const file = join(ENGINE, "patches", patch);
  if (run(["git", "apply", "--reverse", "--check", file], src, { allowFail: true }).ok) {
    console.log(`already applied  ${patch}`);
    continue;
  }
  run(["git", "apply", "--check", file], src);
  run(["git", "apply", file], src);
  console.log(`applied          ${patch}`);
}
