// `bun engine/scripts/acceptance-m2.ts [--save-screens]`: milestone M2's
// checks against the real engine on Windows (docs/engine.md › Milestones):
// GPUI inside the engine's window.
//
// Needs a built engine (build.ts) and the UI library:
//   cargo build -p jaal-ui --release
// For the stack sample it also wants a copy with symbols, and uses that copy
// for the whole run when it exists:
//   cargo build -p jaal-ui --release --target-dir target/symbols \
//     --config "profile.release.debug='line-tables-only'"
// and cdb.exe from the Windows SDK's debugging tools.
//
// It drives the real window with real mouse and key input, so it takes the
// foreground for about three minutes. Every process it starts is stopped by
// its PID. Screenshots and logs go to target/acceptance/m2/; --save-screens
// also copies the screenshots the results note shows to
// bakeoff/results/screens/.
//
// What it can't check is in the results note under "Not checked": an IME and
// a second monitor with another DPI both need a person.

import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import {
  type Rect,
  UI_LIBRARY,
  OUT,
  children,
  finish,
  height,
  launch,
  owned,
  pixel,
  profileDir,
  ps,
  psCommand,
  readLog,
  record,
  rectOf,
  row,
  sameRect,
  show,
  sleep,
  stopTree,
  topLevel,
  waitFor,
  width,
} from "./accept";
import { ENGINE, REPO } from "./lib";

const results = join(REPO, "target", "acceptance", "m2");
const screens = join(REPO, "bakeoff", "results", "screens");
const saveScreens = process.argv.includes("--save-screens");
const symbolsUi = join(REPO, "target", "symbols", "release", "jaal_ui.dll");
const plainUi = join(REPO, "target", "release", "jaal_ui.dll");
const ui = existsSync(symbolsUi) ? symbolsUi : plainUi;
const cdb = "C:\\Program Files (x86)\\Windows Kits\\10\\Debuggers\\x64\\cdb.exe";
const testPage = (name: string) => pathToFileURL(join(ENGINE, "test", name)).href;

// The UI's layout, in DIPs (crates/jaal-ui/src/theme.rs and command_bar.rs).
const SIDEBAR = 236;
const FRAME = 8;
const BAR_WIDTH = 540;
const BAR_TOP = 104;
// An empty bar lists the open tabs, and three are open whenever these checks
// measure it: the test page, the striped page and chrome://webui-browser.
// The sum is command_bar.rs's bar_height(3): the input row, the list's
// padding and three rows, the footer, the bottom padding.
const BAR_HEIGHT_THREE_TABS = 54 + (8 + 3 * 40) + 36 + 8;

mkdirSync(results, { recursive: true });
const logPath = join(results, "ui.log");
const log = () => readLog(logPath);
let mark = 0;
/// What the UI logged since the last `since()` call.
const fresh = () => log().slice(mark);
const since = () => {
  mark = log().length;
};

function shot(hwnd: string, name: string, saveAs?: string) {
  const path = join(results, `${name}.png`);
  ps("shot", hwnd, path);
  if (saveScreens && saveAs) copyFileSync(path, join(screens, `${saveAs}.png`));
  return path;
}

async function check(name: string, body: () => Promise<string>) {
  try {
    record(name, true, await body());
  } catch (error) {
    record(name, false, String(error instanceof Error ? error.message : error));
    // Whatever the check left open (a dialog, the command bar) would block
    // the checks after it.
    try {
      if (hwnd) shot(hwnd, `failed-${name.replace(/[^a-z0-9]+/gi, "-").slice(0, 40)}`);
      ps("key", "esc");
    } catch {}
  }
}

function expect(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

copyFileSync(ui, UI_LIBRARY);
const pdb = ui.replace(/\.dll$/, ".pdb");
if (existsSync(pdb)) copyFileSync(pdb, join(OUT, "jaal_ui.pdb"));
console.log(`UI library: ${ui}`);

ps("wake");
const profile = profileDir("engine-m2", "acceptance");
freshLog();
function freshLog() {
  writeFileSync(logPath, "");
}
const browser = launch(profile, {
  // The third tab is Webium's own page, which must not load in jaal.
  JAAL_UI_URL: `${testPage("m2-page.html")}|${testPage("m2-blur.html")}|chrome://webui-browser`,
  JAAL_UI_FAKE_ROWS: "5000",
  JAAL_UI_THEME: "light",
  JAAL_UI_LOG: logPath,
});

let hwnd = "";
let scale = 1;

const client = (): Rect => rectOf(ps("client", hwnd));
const dip = (value: number) => Math.round(value * scale);
/// Where the page is, from the client area: beside the sidebar, inside the
/// frame.
const pageRect = (c: Rect): Rect => ({
  left: c.left + dip(SIDEBAR),
  top: c.top + dip(FRAME),
  right: c.right - dip(FRAME),
  bottom: c.bottom - dip(FRAME),
});
const sidebarRect = (c: Rect): Rect => ({ left: c.left, top: c.top, right: c.left + dip(SIDEBAR), bottom: c.bottom });
/// Where the command bar is: centered over the page, BAR_TOP below the top.
function barRect(c: Rect, barHeight = BAR_HEIGHT_THREE_TABS): Rect {
  const page = pageRect(c);
  const left = Math.round(page.left + (width(page) - dip(BAR_WIDTH)) / 2);
  return { left, top: c.top + dip(BAR_TOP), right: left + dip(BAR_WIDTH), bottom: c.top + dip(BAR_TOP + barHeight) };
}
/// The middle of band `n` (1 to 5) of m2-page.html.
function band(n: number): [number, number] {
  const page = pageRect(client());
  return [Math.round((page.left + page.right) / 2), Math.round(page.top + (height(page) * (n - 0.5)) / 5)];
}
const titleIs = (title: string) => new RegExp(`TabState\\(TabState \\{[^\\n]*title: "${title}"`);
const pageChild = () => children(hwnd).find((c) => c.className === "Chrome_RenderWidgetHostHWND" && c.visible);
const sidebarChild = () => children(hwnd).find((c) => c.className !== "Chrome_RenderWidgetHostHWND" && c.visible && sameRect(c.rect, sidebarRect(client()), 2));

/// The sidebar and the page are where the layout says, for the window as it
/// is now.
function layoutProblems(): string[] {
  const c = client();
  const problems: string[] = [];
  const page = pageChild();
  if (!page) problems.push("no visible page window");
  else if (!sameRect(page.rect, pageRect(c), 2)) problems.push(`page at ${show(page.rect)}, expected ${show(pageRect(c))}`);
  if (!sidebarChild()) problems.push(`no sidebar window at ${show(sidebarRect(c))}`);
  return problems;
}

/// Opens the command bar with Ctrl+K and checks it is at its place over the
/// page: smooth (blurred) inside, with sharp stripes around it. Closes it.
async function barProblems(label: string, saveAs?: string): Promise<string[]> {
  const problems: string[] = [];
  since();
  ps("key", "ctrl+k");
  try {
    await waitFor("the command bar", 6000, () => fresh().includes("ui: command bar opened"));
  } catch {
    return ["Ctrl+K did not open the command bar"];
  }
  await sleep(700);
  const c = client();
  const bar = barRect(c);
  shot(hwnd, `bar-${label}`, saveAs);
  const { inside, outside } = barRows(bar);
  if (inside.step > 12) problems.push(`inside the bar the largest pixel step is ${inside.step}, so the page is not blurred there`);
  if (outside.step < 80) problems.push(`the stripes below the bar should be sharp, largest step ${outside.step}`);
  problems.push(...layoutProblems());
  ps("key", "esc");
  try {
    await waitFor("the command bar to close", 6000, () => fresh().includes("ui: command bar closed"));
  } catch {
    problems.push("Esc did not close the command bar");
  }
  await sleep(300);
  if (problems.length === 0) console.log(`  ${label}: bar at ${show(bar)}, pixel step ${inside.step} inside, ${outside.step} outside`);
  return problems;
}

/// Two screen rows across the bar's width. The first is inside the bar, in
/// the strip above its text: under real blur the 60 px stripes of
/// m2-blur.html melt into a slow gradient there, and without it their edges
/// are full steps. The second is the page well below the bar and its shadow,
/// where the stripes must still be sharp.
function barRows(bar: Rect) {
  return {
    inside: row(bar.left + dip(24), bar.right - dip(24), bar.top + dip(5)),
    outside: row(bar.left + dip(24), bar.right - dip(24), bar.bottom + dip(150)),
  };
}

function stackSample(name: string): string {
  const r = Bun.spawnSync(
    [cdb, "-pv", "-p", String(browser.pid), "-y", `${OUT};${join(ui, "..")}`, "-c", ".symopt+0x10; ~0 kc 80; qd"],
    { stdout: "pipe", stderr: "pipe" },
  );
  const text = r.stdout.toString();
  // Only the main thread's frames: from the stack's header to the next
  // debugger line.
  const start = text.indexOf("Call Site");
  const stack = start < 0 ? text : text.slice(start).split(/\r?\nquit:/)[0];
  writeFileSync(join(results, `stack-${name}.txt`), stack);
  return stack;
}

try {
  await waitFor("the UI to start", 60_000, () => log().includes("ui: layout sidebar and page"));
  await waitFor("the test page to load", 60_000, () => titleIs("jaal M2 test").test(log()));
  hwnd = await waitFor("the window", 20_000, () => {
    const h = psCommand(`(Get-Process -Id ${browser.pid}).MainWindowHandle`);
    return h && h !== "0" ? h : undefined;
  });
  scale = Number(log().match(/scale: ([\d.]+)/)?.[1] ?? 1);
  ps("wake");
  ps("place", hwnd, 80, 60, 2200, 1500);
  ps("front", hwnd);
  await sleep(1200);

  await check("sidebar: GPUI draws in the engine's window, the page beside it", async () => {
    const problems = layoutProblems();
    expect(problems.length === 0, problems.join("; "));
    const c = client();
    // m2-page.html's background, well inside the page's rounded corner.
    const page = pageRect(c);
    const [r, g, b] = pixel(page.left + dip(30), page.top + dip(30));
    expect(Math.abs(r - 0xf4) < 4 && Math.abs(g - 0xf1) < 4 && Math.abs(b - 0xea) < 4, `the page pixel is ${r},${g},${b}, not the test page's #f4f1ea`);
    // The light chrome, at the sidebar's left edge where nothing else draws.
    const [sr, sg, sb] = pixel(c.left + dip(4), c.top + dip(300));
    expect(sr > 0xe0 && sr < 0xf2 && sg > 0xd8 && sb > 0xcc && sr > sb, `the sidebar pixel is ${sr},${sg},${sb}, not the light chrome`);
    expect(log().includes("ui: 5000 test rows"), "the sidebar did not load the 5,000 fake rows");
    shot(hwnd, "sidebar", "m2-sidebar-windows");
    return `sidebar child at ${show(sidebarRect(c))}, page at ${show(page)}, scale ${scale}`;
  });

  await check("Webium audit: chrome://webui-browser is refused, and the browser survives it", async () => {
    // Before the audit's patch, opening this page crashed the browser.
    await waitFor("the page to finish loading", 15_000, () => /url: "chrome:\/\/webui-browser\/", title: "[^"]*", loading: false/.test(log()));
    expect(browser.exitCode === null, `the browser exited with code ${browser.exitCode}`);
    return "the tab finished loading as an error page, and the browser is still running";
  });

  await check("sidebar: the 5,000-row list scrolls", async () => {
    const c = client();
    since();
    ps("wheel", c.left + dip(110), c.top + dip(520), "down", 14);
    const top = await waitFor("the list to scroll", 6000, () => {
      const rows = [...fresh().matchAll(/ui: list top row (\d+)/g)];
      return rows.length ? Number(rows[rows.length - 1][1]) : undefined;
    });
    await sleep(500);
    shot(hwnd, "sidebar-scrolled", "m2-sidebar-scrolled-windows");
    expect(top >= 10, `the list only reached row ${top}`);
    return `14 wheel notches moved the top of the list from row 0 to row ${top}`;
  });

  await check("Chromium's pump runs GPUI: stack samples", async () => {
    expect(existsSync(cdb), `no debugger at ${cdb}`);
    expect(ui === symbolsUi, "needs the UI library built with symbols (see the top of this script)");
    const idle = stackSample("idle");
    const c = client();
    // Samples taken while the wheel keeps the sidebar list scrolling.
    const wheel = Bun.spawn(
      ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(ENGINE, "scripts", "win-input.ps1"), "wheel", String(c.left + dip(110)), String(c.top + dip(520)), "down", "4000"],
      { stdout: "ignore", stderr: "ignore" },
    );
    await sleep(900);
    // The main thread sleeps in the pump between frames, so most samples
    // catch it waiting. Sampling goes on until one catches GPUI at work.
    const scrolling: string[] = [];
    for (let i = 0; i < 40 && wheel.exitCode === null; i++) {
      scrolling.push(stackSample(`scroll-${i}`));
      if (/jaal_ui!/.test(scrolling[i])) break;
    }
    wheel.kill();
    await wheel.exited;
    const all = [idle, ...scrolling];
    const pump = all.filter((s) => /base::MessagePumpForUI/.test(s)).length;
    // GPUI's own loop would be WindowsPlatform::run, with GetMessageW under it.
    const gpuiLoop = all.filter((s) => /WindowsPlatform::run\b|WindowsPlatform.*::run$/m.test(s) && !/run_embedded/.test(s)).length;
    const inGpui = scrolling.filter((s) => /jaal_ui!/.test(s)).length;
    expect(pump === all.length, `${all.length - pump} of ${all.length} samples have no base::MessagePumpForUI frame`);
    expect(gpuiLoop === 0, `${gpuiLoop} samples have a GPUI run-loop frame`);
    expect(inGpui > 0, "no sample caught GPUI's code while the list scrolled");
    return `cdb, 1 idle and ${scrolling.length} scrolling samples of the main thread: base::MessagePumpForUI in all ${all.length}, GPUI's WindowsPlatform::run in none, ${inGpui} scrolling samples inside jaal_ui.dll under the pump (target/acceptance/m2/stack-*.txt)`;
  });

  await check("page input: a click on a link navigates, Back returns", async () => {
    since();
    ps("click", ...band(1));
    await waitFor("the linked page", 10_000, () => titleIs("jaal M2 linked page").test(fresh()));
    await sleep(400);
    shot(hwnd, "link-clicked", "m2-link-clicked-windows");
    // The sidebar's Back button.
    const c = client();
    since();
    ps("click", c.left + dip(23), c.top + dip(21));
    await waitFor("the test page again", 10_000, () => titleIs("jaal M2 test").test(fresh()));
    return 'the click loaded "jaal M2 linked page"; the sidebar\'s Back button returned to "jaal M2 test"';
  });

  await check("page input: typed text appears in the page's text field", async () => {
    since();
    ps("click", ...band(2));
    await sleep(400);
    ps("type", "jaal types here");
    await waitFor("the typed text", 10_000, () => titleIs("typed:jaal types here").test(fresh()));
    await sleep(300);
    shot(hwnd, "typed", "m2-typed-windows");
    return 'the page reported its field as "jaal types here"';
  });

  await check("shortcuts: Ctrl+B is the page's and never reaches the app", async () => {
    since();
    ps("key", "ctrl+b");
    await waitFor("the page to see Ctrl+B", 6000, () => titleIs("keys:ctrl\\+b").test(fresh()));
    await sleep(600);
    expect(!/code: 66,/.test(fresh()), "the app got Ctrl+B although the page handled it");
    return "the page saw ctrl+b and called preventDefault; no key event reached the app";
  });

  await check("shortcuts: Ctrl+K goes to the page first, then comes back to the app", async () => {
    since();
    ps("key", "ctrl+k");
    await waitFor("the command bar", 6000, () => fresh().includes("ui: command bar opened (Navigate)"));
    expect(/code: 75,[^\n]*reserved: false/.test(fresh()), "Ctrl+K did not arrive as a key the page left");
    await waitFor("the page to have seen Ctrl+K", 6000, () => titleIs("keys:ctrl\\+b,ctrl\\+k").test(log()));
    ps("key", "esc");
    await waitFor("the bar to close", 6000, () => fresh().includes("ui: command bar closed"));
    return "the page saw ctrl+k and left it; the app then got it (reserved: false) and opened the command bar";
  });

  await check("shortcuts: Ctrl+T, Ctrl+L and Ctrl+W reach the app first", async () => {
    await sleep(400);
    since();
    ps("key", "ctrl+t");
    await waitFor("the command bar for a new tab", 6000, () => fresh().includes("ui: command bar opened (NewTab)"));
    expect(/code: 84,[^\n]*reserved: true/.test(fresh()), "Ctrl+T was not reported as reserved");
    await sleep(500);
    shot(hwnd, "ctrl-t", "m2-ctrl-t-windows");
    ps("key", "esc");
    await waitFor("the bar to close", 6000, () => fresh().includes("ui: command bar closed"));
    await sleep(400);

    since();
    ps("key", "ctrl+l");
    await waitFor("the address field", 6000, () => fresh().includes("ui: address focused"));
    expect(/code: 76,[^\n]*reserved: true/.test(fresh()), "Ctrl+L was not reported as reserved");
    await sleep(500);
    shot(hwnd, "ctrl-l", "m2-ctrl-l-windows");
    ps("key", "esc");
    await sleep(500);

    // The page never saw either: its list of keys is unchanged.
    const keys = [...log().matchAll(/title: "keys:([^"]*)"/g)].pop()?.[1];
    expect(keys === "ctrl+b,ctrl+k", `the page's keys are "${keys}", expected only ctrl+b,ctrl+k`);
    // Focus is back in the page: it sees the next key.
    since();
    ps("key", "ctrl+b");
    await waitFor("the page to get keys again", 6000, () => titleIs("keys:ctrl\\+b,ctrl\\+k,ctrl\\+b").test(fresh()));
    return "Ctrl+T opened the command bar and Ctrl+L focused the address (both reserved: true); the page saw neither, and got the keyboard back after each";
  });

  await check("file picker: the OS dialog is owned by our window, cancel leaves the app working", async () => {
    since();
    ps("click", ...band(3));
    // Chrome shows the dialog from a utility process, so it is found by its
    // owner, not by the browser's process.
    const dialog = await waitFor("a dialog owned by our window", 10_000, () => owned(hwnd).find((w) => w.className === "#32770"));
    await sleep(800);
    const path = join(results, "file-picker.png");
    ps("shot", dialog.hwnd, path);
    if (saveScreens) copyFileSync(path, join(screens, "m2-file-picker-windows.png"));
    ps("key", "esc");
    await waitFor("the page to hear the cancel", 10_000, () => titleIs("file:cancelled").test(fresh()));
    await sleep(500);
    // Still working: the app answers a shortcut.
    since();
    ps("key", "ctrl+k");
    await waitFor("the command bar", 6000, () => fresh().includes("ui: command bar opened"));
    ps("key", "esc");
    await waitFor("the bar to close", 6000, () => fresh().includes("ui: command bar closed"));
    return `the system's file dialog (class ${dialog.className}) opened, owned by window ${hwnd}; Esc cancelled it (the page got "cancel"), and Ctrl+K still opened the command bar`;
  });

  await check("tab-modal dialog: a JavaScript alert sits over the page, not the sidebar", async () => {
    await sleep(300);
    since();
    const page = pageRect(client());
    // Chrome draws a tab-modal dialog inside the window, not in a window of
    // its own, so its edges are read off the screen: along a row near the
    // top of the page, the dialog is whatever isn't the page's background.
    const y = page.top + dip(60);
    const spanOf = () => ps("span", page.left + dip(12), page.right - dip(12), y, 0xf4, 0xf1, 0xea);
    expect(spanOf() === "none", `the page's top band is not plain before the alert: ${spanOf()}`);
    ps("click", ...band(4));
    await waitFor("the alert", 10_000, () => titleIs("alert:open").test(fresh()));
    const span = await waitFor("the dialog", 10_000, () => (spanOf() === "none" ? undefined : spanOf()));
    await sleep(900);
    const [left, right] = spanOf().split(" ").map(Number);
    shot(hwnd, "tab-modal", "m2-tab-modal-windows");
    const offCentre = Math.abs((left + right) / 2 - (page.left + page.right) / 2);
    expect(left > page.left && right < page.right, `the dialog spans x ${left} to ${right}, outside the page (${page.left} to ${page.right})`);
    expect(offCentre <= dip(2), `the dialog is ${offCentre} px off the page's centre line`);
    ps("key", "enter");
    await waitFor("the alert to close", 10_000, () => titleIs("alert:closed").test(fresh()));
    await sleep(400);
    expect(spanOf() === "none", "the dialog is still showing after Enter");
    return `the alert spans x ${left} to ${right} (first seen as ${span}); the page spans ${page.left} to ${page.right}; their centre lines differ by ${offCentre} px, and the sidebar ends at ${page.left}`;
  });

  // The striped page, for the command bar's blur.
  await check("command bar: draws over the page with blur and a shadow", async () => {
    const c = client();
    since();
    // The second tab's row in the sidebar: below the top row, the address
    // field, the space's name, the "Today" divider, New tab and the first tab.
    ps("click", c.left + dip(110), c.top + dip(217));
    await waitFor("the striped page", 10_000, () => /title: "jaal M2 blur test", loading: false, active: true/.test(fresh()));
    await sleep(900);
    const problems = await barProblems("resting", "m2-command-bar-windows");
    expect(problems.length === 0, problems.join("; "));

    // The shadow: with the bar open, the page just below it is darker than
    // the same stripe further down.
    ps("key", "ctrl+k");
    await waitFor("the command bar", 6000, () => fresh().includes("ui: command bar opened"));
    await sleep(700);
    const bar = barRect(client());
    const light = (x: number, y: number) => pixel(x, y).reduce((a, b) => a + b, 0) / 3;
    // A light stripe near the bar's middle: stripes are 60 CSS px wide and
    // the second of every four is the light one.
    let x = bar.left + dip(200);
    while (light(x, bar.bottom + dip(150)) < 200 && x < bar.right) x += dip(15);
    const near = light(x, bar.bottom + dip(10));
    const far = light(x, bar.bottom + dip(150));
    const tasks = topLevel(browser.pid).filter((w) => w.taskbar);
    ps("key", "esc");
    await waitFor("the bar to close", 6000, () => fresh().includes("ui: command bar closed"));
    expect(far - near > 12, `no shadow: the page is ${near.toFixed(0)} just under the bar and ${far.toFixed(0)} further down`);
    expect(tasks.length === 1 && tasks[0].hwnd === hwnd, `windows with a taskbar button: ${tasks.map((w) => w.className).join(", ")}`);
    return `blurred under the bar and sharp around it; the page under the bar's edge is ${(far - near).toFixed(0)} levels darker than further down (the shadow); the only window with a taskbar button is the main one`;
  });

  await check("command bar: follows the window when it moves and resizes", async () => {
    since();
    ps("key", "ctrl+k");
    await waitFor("the command bar", 6000, () => fresh().includes("ui: command bar opened"));
    await sleep(600);
    const seen: string[] = [];
    for (const [label, x, y, w, h] of [
      ["moved", 420, 180, 2200, 1500],
      ["resized", 420, 180, 1800, 1300],
    ] as const) {
      ps("place", hwnd, x, y, w, h);
      await sleep(900);
      const bar = barRect(client());
      const { inside, outside } = barRows(bar);
      shot(hwnd, `bar-${label}`, `m2-command-bar-${label}-windows`);
      expect(inside.step <= 12 && outside.step >= 80, `after it ${label}, the bar is not at ${show(bar)}: pixel step ${inside.step} inside, ${outside.step} outside`);
      seen.push(`${label}: bar at ${show(bar)}`);
    }
    ps("key", "esc");
    await waitFor("the bar to close", 6000, () => fresh().includes("ui: command bar closed"));
    return `the open bar stayed over the page (${seen.join("; ")})`;
  });

  await check("window changes: move, resize, minimize, maximize and fullscreen keep everything in place", async () => {
    const steps: string[] = [];
    const settle = async (label: string, saveAs: string) => {
      await sleep(1200);
      ps("front", hwnd);
      await sleep(500);
      const problems = await barProblems(label, saveAs);
      expect(problems.length === 0, `after ${label}: ${problems.join("; ")}`);
      steps.push(`${label} (client ${show(client())})`);
    };

    ps("place", hwnd, 200, 120, 1800, 1300);
    await settle("move", "m2-window-move-windows");
    ps("place", hwnd, 200, 120, 2400, 1560);
    await settle("resize", "m2-window-resize-windows");

    ps("show", hwnd, "minimize");
    await sleep(1200);
    expect(ps("state", hwnd) === "minimized", "the window did not minimize");
    ps("show", hwnd, "restore");
    await settle("minimize and restore", "m2-window-restored-windows");

    ps("show", hwnd, "maximize");
    await sleep(600);
    expect(ps("state", hwnd) === "maximized", "the window did not maximize");
    await settle("maximize", "m2-window-maximized-windows");
    ps("show", hwnd, "restore");
    await settle("restore from maximized", "m2-window-unmaximized-windows");

    // F11 goes to the page, which leaves it, so the app gets it.
    since();
    ps("key", "f11");
    await waitFor("fullscreen", 8000, () => fresh().includes("ui: layout page alone"));
    await sleep(1500);
    const screen = rectOf(ps("rect", hwnd));
    const page = pageChild();
    shot(hwnd, "fullscreen", "m2-window-fullscreen-windows");
    expect(!!page && sameRect(page.rect, screen, 2), `in fullscreen the page is at ${page ? show(page.rect) : "nowhere"}, the window at ${show(screen)}`);
    expect(!children(hwnd).some((c) => c.className !== "Chrome_RenderWidgetHostHWND" && c.visible && width(c.rect) === dip(SIDEBAR)), "the sidebar is still showing in fullscreen");
    steps.push(`fullscreen (page fills ${show(screen)}, sidebar hidden)`);
    since();
    ps("key", "f11");
    await waitFor("leaving fullscreen", 8000, () => fresh().includes("ui: layout sidebar and page"));
    await settle("back from fullscreen", "m2-window-unfullscreen-windows");
    return steps.join("; ");
  });

  await check("shortcuts: Ctrl+W closes the tab", async () => {
    since();
    ps("key", "ctrl+w");
    await waitFor("the tab to close", 8000, () => /TabRemoved/.test(fresh()));
    expect(/code: 87,[^\n]*reserved: true/.test(fresh()), "Ctrl+W was not reported as reserved");
    await sleep(500);
    shot(hwnd, "ctrl-w");
    return "Ctrl+W (reserved: true) closed the active tab";
  });

  // The window's own close button: the app quits when its window closes.
  await check("the window's close button ends the process", async () => {
    const c = client();
    ps("front", hwnd);
    // m2-page.html has no beforeunload handler, so nothing asks to stay.
    ps("click", c.left + dip(SIDEBAR - 10 - 9), c.top + dip(21));
    const code = await Promise.race([browser.exited, sleep(30_000).then(() => "timeout" as const)]);
    expect(code === 0, `exit ${code}`);
    expect(log().includes("WillQuit"), "no WillQuit event");
    return "clicking the sidebar's close button closed the window; the app quit with exit code 0";
  });
} catch (error) {
  record("run", false, String(error));
} finally {
  if (browser.exitCode === null) stopTree(browser.pid);
}

process.exit(finish());
