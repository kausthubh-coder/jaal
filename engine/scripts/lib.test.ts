// `bun test ./engine/scripts`: the checks check.ts relies on, against a
// throwaway git repo standing in for the Chromium checkout.

import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { modifiedTrackedFiles, run } from "./lib";

function checkoutWithOneCommit(): string {
  const dir = mkdtempSync(join(tmpdir(), "jaal-lib-test-"));
  run(["git", "init", "-q"], dir);
  writeFileSync(join(dir, "chrome.cc"), "stock\n");
  writeFileSync(join(dir, "other.cc"), "stock\n");
  run(["git", "add", "."], dir);
  run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "-m", "stock"], dir);
  return dir;
}

test("a clean checkout has no modified files", () => {
  const dir = checkoutWithOneCommit();
  try {
    expect(modifiedTrackedFiles(dir)).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("an unstaged edit is reported", () => {
  const dir = checkoutWithOneCommit();
  try {
    writeFileSync(join(dir, "chrome.cc"), "edited\n");
    expect(modifiedTrackedFiles(dir)).toEqual(["chrome.cc"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// The working tree matches the index here, so a comparison with the index
// sees nothing. The build still compiles the edited file.
test("a staged edit is reported", () => {
  const dir = checkoutWithOneCommit();
  try {
    writeFileSync(join(dir, "chrome.cc"), "edited\n");
    run(["git", "add", "chrome.cc"], dir);
    expect(modifiedTrackedFiles(dir)).toEqual(["chrome.cc"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
