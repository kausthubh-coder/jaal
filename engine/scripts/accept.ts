// Helpers the engine acceptance scripts share: recording checks, driving the
// window through win-input.ps1, and starting and stopping the engine.

import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { ENGINE, REPO, chromiumSrc } from "./lib";

export const OUT = join(chromiumSrc(), "out", "Release");
export const EXE = join(OUT, "chrome.exe");
/// Where the engine looks for the UI library.
export const UI_LIBRARY = join(OUT, "jaal_ui.dll");
const helper = join(ENGINE, "scripts", "win-input.ps1");

export type Rect = { left: number; top: number; right: number; bottom: number };

const report: [string, boolean, string][] = [];

export function record(name: string, ok: boolean, detail: string) {
  report.push([name, ok, detail]);
  console.log(`${ok ? "PASS" : "FAIL"} ${name}: ${detail}`);
}

/// Prints the tally and returns the process exit code.
export function finish(): number {
  const failed = report.filter(([, ok]) => !ok).length;
  console.log(failed ? `${failed} of ${report.length} checks failed` : `all ${report.length} checks passed`);
  return failed ? 1 : 0;
}

/// Runs one win-input.ps1 command and returns what it printed.
export function ps(...args: (string | number)[]): string {
  const r = Bun.spawnSync(
    ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", helper, ...args.map(String)],
    { stdout: "pipe", stderr: "pipe" },
  );
  if (r.exitCode !== 0) throw new Error(`win-input ${args.join(" ")}: ${r.stderr.toString()}`);
  return r.stdout.toString().trim();
}

export function psCommand(command: string): string {
  return Bun.spawnSync(["powershell", "-NoProfile", "-Command", command], { stdout: "pipe" }).stdout.toString().trim();
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function waitFor<T>(what: string, timeoutMs: number, probe: () => T | undefined | false | null): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = probe();
    if (value) return value;
    await sleep(200);
  }
  throw new Error(`timed out after ${timeoutMs} ms waiting for ${what}`);
}

export function freshDir(dir: string): string {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  return dir;
}

export function profileDir(group: string, name: string): string {
  return freshDir(join(REPO, "target", "profiles", group, name));
}

export function launch(profile: string, env: Record<string, string>) {
  return Bun.spawn([EXE, `--user-data-dir=${profile}`], {
    env: { ...process.env, ...env },
    stdout: "ignore",
    stderr: "ignore",
  });
}

/// Stops a process tree we started, by its PID.
export function stopTree(pid: number) {
  Bun.spawnSync(["taskkill", "/T", "/F", "/PID", String(pid)], { stdout: "ignore", stderr: "ignore" });
}

export function rectOf(text: string): Rect {
  const [left, top, right, bottom] = text.trim().split(/\s+/).map(Number);
  return { left, top, right, bottom };
}

export const width = (r: Rect) => r.right - r.left;
export const height = (r: Rect) => r.bottom - r.top;
export const sameRect = (a: Rect, b: Rect, slack = 1) =>
  Math.abs(a.left - b.left) <= slack &&
  Math.abs(a.top - b.top) <= slack &&
  Math.abs(a.right - b.right) <= slack &&
  Math.abs(a.bottom - b.bottom) <= slack;
export const show = (r: Rect) => `${r.left},${r.top} ${width(r)}x${height(r)}`;

export type ChildWindow = { hwnd: string; className: string; visible: boolean; rect: Rect };

/// The direct child windows of `hwnd`.
export function children(hwnd: string): ChildWindow[] {
  return ps("children", hwnd)
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      const [child, className, visible, ...rect] = line.trim().split(/\s+/);
      return { hwnd: child, className, visible: visible === "1", rect: rectOf(rect.join(" ")) };
    });
}

export type TopLevelWindow = { hwnd: string; className: string; owner: string; taskbar: boolean; rect: Rect };

/// The visible windows `hwnd` owns, in any process.
export function owned(hwnd: string): TopLevelWindow[] {
  return parseTopLevel(ps("owned", hwnd));
}

/// The visible top-level windows of a process.
export function topLevel(pid: number): TopLevelWindow[] {
  return parseTopLevel(ps("toplevel", pid));
}

function parseTopLevel(text: string): TopLevelWindow[] {
  return text
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      const [hwnd, className, owner, taskbar, ...rect] = line.trim().split(/\s+/);
      return { hwnd, className, owner, taskbar: taskbar === "1", rect: rectOf(rect.join(" ")) };
    });
}

/// One screen row: the largest change between neighbouring pixels, and the
/// darkest and lightest pixel.
export function row(x1: number, x2: number, y: number): { step: number; min: number; max: number } {
  const [step, min, max] = ps("row", Math.round(x1), Math.round(x2), Math.round(y)).split(" ").map(Number);
  return { step, min, max };
}

export function pixel(x: number, y: number): [number, number, number] {
  return ps("pixel", Math.round(x), Math.round(y)).split(" ").map(Number) as [number, number, number];
}

export function readLog(path: string): string {
  return existsSync(path) ? readFileSync(path, "utf8") : "";
}
