// `bun engine/scripts/acceptance.ts`: milestone M1's checks against the real
// engine on Windows (docs/engine.md › Milestones).
//
// Needs a built engine (build.ts) and the hello UI library
// (`cargo build -p jaal-ui-hello --release`, plus `--features wrong-abi
// --target-dir target/wrong-abi`). Every process it starts is stopped by its
// PID before it exits. It takes the foreground for a few seconds to click the
// page and answer the beforeunload dialog.
//
// Writes target/acceptance/m1/ (screenshot, event logs) and prints PASS/FAIL
// per check.

import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { ENGINE, REPO, chromiumSrc } from "./lib";

const out = join(chromiumSrc(), "out", "Release");
const exe = join(out, "chrome.exe");
const uiLibrary = join(out, "jaal_ui.dll");
const goodUi = join(REPO, "target", "release", "jaal_ui.dll");
const wrongAbiUi = join(REPO, "target", "wrong-abi", "release", "jaal_ui.dll");
const results = join(REPO, "target", "acceptance", "m1");
const profiles = join(REPO, "target", "profiles", "engine-m1");
const helper = join(ENGINE, "scripts", "win-input.ps1");
const page = pathToFileURL(join(ENGINE, "test", "quit-page.html")).href;

mkdirSync(results, { recursive: true });
const report: [string, boolean, string][] = [];
function record(name: string, ok: boolean, detail: string) {
  report.push([name, ok, detail]);
  console.log(`${ok ? "PASS" : "FAIL"} ${name}: ${detail}`);
}

function ps(...args: string[]): string {
  const r = Bun.spawnSync(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", helper, ...args], {
    stdout: "pipe",
    stderr: "pipe",
  });
  if (r.exitCode !== 0) throw new Error(`win-input ${args.join(" ")}: ${r.stderr.toString()}`);
  return r.stdout.toString().trim();
}

function psCommand(command: string): string {
  return Bun.spawnSync(["powershell", "-NoProfile", "-Command", command], { stdout: "pipe" }).stdout.toString().trim();
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function waitFor<T>(what: string, timeoutMs: number, probe: () => T | undefined | false): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = probe();
    if (value) return value;
    await sleep(250);
  }
  throw new Error(`timed out after ${timeoutMs} ms waiting for ${what}`);
}

function freshProfile(name: string): string {
  const dir = join(profiles, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  return dir;
}

function launch(profile: string, log?: string) {
  return Bun.spawn([exe, `--user-data-dir=${profile}`], {
    env: { ...process.env, JAAL_HELLO_URL: page, ...(log ? { JAAL_HELLO_LOG: log } : {}) },
    stdout: "ignore",
    stderr: "ignore",
  });
}

// Stops a process tree we started, by its PID.
function stopTree(pid: number) {
  Bun.spawnSync(["taskkill", "/T", "/F", "/PID", String(pid)], { stdout: "ignore", stderr: "ignore" });
}

async function exitCodeWithin(proc: ReturnType<typeof launch>, ms: number): Promise<number | "timeout"> {
  const code = await Promise.race([proc.exited, sleep(ms).then(() => "timeout" as const)]);
  if (code === "timeout") stopTree(proc.pid);
  return code;
}

// 1. A UI library built for another ABI is refused before any profile opens.
copyFileSync(wrongAbiUi, uiLibrary);
{
  const profile = freshProfile("wrong-abi");
  const code = await exitCodeWithin(launch(profile), 60_000);
  const opened = existsSync(join(profile, "Default"));
  record("wrong ABI refused", code === 91 && !opened, `exit ${code}, profile folder Default ${opened ? "created" : "not created"}`);
}

// 2. No UI library: refused the same way.
rmSync(uiLibrary, { force: true });
{
  const profile = freshProfile("missing-ui");
  const code = await exitCodeWithin(launch(profile), 60_000);
  const opened = existsSync(join(profile, "Default"));
  record("missing UI library refused", code === 90 && !opened, `exit ${code}, profile folder Default ${opened ? "created" : "not created"}`);
}

// 3-6. One window, one tab, through the hello UI.
copyFileSync(goodUi, uiLibrary);
const profile = freshProfile("main");
const log = join(results, "events.log");
rmSync(log, { force: true });
const browser = launch(profile, log);
const events = () => (existsSync(log) ? readFileSync(log, "utf8") : "");

try {
  await waitFor("the test page to load", 60_000, () =>
    /TabState\(TabState \{[^\n]*title: "jaal quit test", loading: false/.test(events()),
  );
  record("window and tab through the C API", true, `events: ${events().split("\n").filter(Boolean).length} lines in ${log}`);

  const hwnd = await waitFor("the window", 20_000, () => {
    const h = psCommand(`(Get-Process -Id ${browser.pid}).MainWindowHandle`);
    return h && h !== "0" ? h : undefined;
  });
  const title = psCommand(`(Get-Process -Id ${browser.pid}).MainWindowTitle`);
  ps("front", hwnd);
  await sleep(800);
  const shot = join(results, "window.png");
  ps("shot", hwnd, shot);
  record("window has no Chrome toolbar or tab strip", true, `title "${title}"; look at ${shot}`);

  // Helper processes: none may load jaal_ui.dll. As a control, every helper
  // that runs ChromeMain must be seen loading chrome.dll, proving tasklist can
  // see its modules. Crashpad's handler runs from chrome.exe before ChromeMain
  // and never loads chrome.dll, so it has no control.
  const children = psCommand(
    `Get-CimInstance Win32_Process -Filter "ParentProcessId=${browser.pid}" | ForEach-Object { "$($_.ProcessId) $(if ($_.CommandLine -match '--type=(\\S+)') { $matches[1] } else { '?' })" }`,
  )
    .split(/\r?\n/)
    .map((line) => line.trim().split(" "))
    .filter(([pid]) => pid)
    .map(([pid, type]) => ({ pid: Number(pid), type }));
  const pidsWith = (dll: string) =>
    new Set(
      psCommand(`tasklist /m ${dll} /fo csv /nh`)
        .split(/\r?\n/)
        .map((line) => Number(line.split(",")[1]?.replaceAll('"', "")))
        .filter((pid) => pid > 0),
    );
  const withChrome = pidsWith("chrome.dll");
  const withUi = pidsWith("jaal_ui.dll");
  const unseen = children.filter((c) => c.type !== "crashpad-handler" && !withChrome.has(c.pid));
  const helperWithUi = children.filter((c) => withUi.has(c.pid));
  record(
    "helpers never load the UI library",
    children.length > 1 && unseen.length === 0 && helperWithUi.length === 0 && withUi.has(browser.pid),
    `${children.length} helpers (${children.map((c) => c.type).join(", ")}); unseen: ${unseen.length}; helpers with jaal_ui.dll: ${helperWithUi.length}; browser has it: ${withUi.has(browser.pid)}`,
  );

  // Cancelled quit: the page asks to stay, Escape answers "Cancel".
  const [left, top, right, bottom] = ps("rect", hwnd).split(" ").map(Number);
  const centre = [String(Math.round((left + right) / 2)), String(Math.round((top + bottom) / 2))];
  ps("front", hwnd);
  ps("click", ...centre);
  await waitFor("the first quit", 10_000, () => events().includes('title: "jaal:quit:1"'));
  await sleep(1500);
  ps("shot", hwnd, join(results, "beforeunload.png"));
  ps("key", "esc");
  await waitFor("QuitCancelled", 10_000, () => events().includes("QuitCancelled"));
  await sleep(1000);
  const alive = browser.exitCode === null;
  record("beforeunload cancels a quit", alive && !events().includes("WillQuit"), `QuitCancelled logged, browser still running: ${alive}`);

  // Committed quit: Enter answers "Leave", and the process ends cleanly.
  ps("front", hwnd);
  ps("click", ...centre);
  await waitFor("the second quit", 10_000, () => events().includes('title: "jaal:quit:2"'));
  await sleep(1500);
  ps("key", "enter");
  const code = await exitCodeWithin(browser, 30_000);
  record("quit ends the process", code === 0 && events().includes("WillQuit"), `exit ${code}, WillQuit logged: ${events().includes("WillQuit")}`);
} catch (error) {
  record("run", false, String(error));
} finally {
  if (browser.exitCode === null) stopTree(browser.pid);
}

const failed = report.filter(([, ok]) => !ok).length;
console.log(failed ? `${failed} of ${report.length} checks failed` : `all ${report.length} checks passed`);
process.exit(failed ? 1 : 0);
