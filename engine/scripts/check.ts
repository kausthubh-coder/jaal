// `bun engine/scripts/check.ts`: is the Chromium tree exactly what the repo
// says it should be?
//
// - The tree is at chromium.lock's commit.
// - Applying patches/series to clean copies of the files it touches gives
//   the files in the tree byte for byte (line endings aside), without fuzz.
// - No other tracked Chromium file is modified.
// - //jaal matches engine/src and engine/public byte for byte.
//
// Fails with a list of every difference. Run it before every engine PR.

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { ENGINE, checkTreeCommit, chromiumSrc, modifiedTrackedFiles, ownFiles, run, series, touchedFiles } from "./lib";

const src = chromiumSrc();
const problems: string[] = [];
checkTreeCommit(src);

const patches = series();
const touched = [...new Set(patches.flatMap(touchedFiles))];

// Clean copies from the locked commit, outside any git repo so `git apply`
// works as a plain patch tool there.
const clean = mkdtempSync(join(tmpdir(), "jaal-check-"));
try {
  for (const path of touched) {
    const original = run(["git", "show", `HEAD:${path}`], src).out;
    mkdirSync(dirname(join(clean, path)), { recursive: true });
    writeFileSync(join(clean, path), original);
  }
  for (const patch of patches) {
    const r = run(["git", "apply", join(ENGINE, "patches", patch)], clean, {
      allowFail: true,
    });
    if (!r.ok) problems.push(`${patch} doesn't apply cleanly to ${src}'s commit:\n${r.err}`);
  }
  for (const path of touched) {
    // Compared without CR: a Windows checkout with core.autocrlf has CRLF in
    // its working files, while `git show` and the patches use LF.
    const want = readFileSync(join(clean, path), "utf8").replaceAll("\r\n", "\n");
    const have = readFileSync(join(src, path), "utf8").replaceAll("\r\n", "\n");
    if (want !== have) problems.push(`${path} differs from what patches/series produces`);
  }
} finally {
  rmSync(clean, { recursive: true, force: true });
}

for (const path of modifiedTrackedFiles(src)) {
  if (!touched.includes(path)) problems.push(`${path} is modified but no patch in patches/series touches it`);
}

for (const { from, to } of ownFiles(src)) {
  if (!existsSync(to)) problems.push(`${to} is missing; run engine/scripts/apply.ts`);
  else if (!readFileSync(from).equals(readFileSync(to))) problems.push(`${to} differs from ${from}`);
}

if (problems.length) {
  console.error(problems.map((p) => `FAIL ${p}`).join("\n"));
  process.exit(1);
}
console.log(`ok: ${patches.length} patches, ${touched.length} Chromium files, //jaal matches engine/`);
