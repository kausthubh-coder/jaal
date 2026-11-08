// Runs the bake-off tests (docs/bakeoff.md) and writes the results to
// bakeoff/results/<date>-<host>.{json,md}.
//
//   bun bakeoff/bench/run.ts                       every test, every variant, 5 runs
//   bun bakeoff/bench/run.ts --variants web-ui,helium --tests start,memory --runs 3
//
// Variants are release bundles at target/bundle/<variant>.app (build them
// first with bundle-cef-app --release). Helium is target/helium/Helium.app.
// Runs alternate between variants (A, B, C, A, B, C, ...) so warmup and
// background noise favor none of them.

import { spawn, spawnSync, type Subprocess } from "bun";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { hostname, tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const pagesDir = join(root, "bakeoff", "pages");
const resultsDir = join(root, "bakeoff", "results");

type Variant = "web-ui" | "gpui-a" | "gpui-b" | "helium";
type Test = "start" | "memory" | "switch" | "scroll" | "resize" | "idle";
const ALL_VARIANTS: Variant[] = ["web-ui", "gpui-a", "gpui-b", "helium"];
const ALL_TESTS: Test[] = ["start", "memory", "idle", "switch", "scroll", "resize"];
// Helium has no bench marks and no 5,000-row sidebar; see docs/bakeoff.md.
const HELIUM_TESTS: Test[] = ["start", "memory", "idle"];

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : undefined;
}
const runs = Number(arg("runs") ?? 5);
const variants = (arg("variants")?.split(",") ?? ALL_VARIANTS) as Variant[];
const tests = (arg("tests")?.split(",") ?? ALL_TESTS) as Test[];

const pageUrl = (n: number) => `file://${join(pagesDir, `page-${n}.html`)}`;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function executable(v: Variant): string {
  if (v === "helium") return join(root, "target/helium/Helium.app/Contents/MacOS/Helium");
  return join(root, `target/bundle/${v}.app/Contents/MacOS/${v}`);
}

// ---- processes ----------------------------------------------------------

/** pid plus every descendant (CEF and Chromium helpers are children). */
function processTree(pid: number): number[] {
  const out = spawnSync(["ps", "-axo", "pid=,ppid="]).stdout.toString();
  const children = new Map<number, number[]>();
  for (const line of out.trim().split("\n")) {
    const [p, pp] = line.trim().split(/\s+/).map(Number);
    if (!children.has(pp)) children.set(pp, []);
    children.get(pp)!.push(p);
  }
  const tree = [pid];
  for (let i = 0; i < tree.length; i++) tree.push(...(children.get(tree[i]) ?? []));
  return tree;
}

/** Memory of a process tree: footprint counts shared pages once; RSS double-counts them. */
function memory(pid: number): { footprintMB: number; rssMB: number; processes: number } {
  const pids = processTree(pid);
  const json = join(tmpdir(), `jaal-fp-${pid}.json`);
  spawnSync(["footprint", "-f", "bytes", "-j", json, ...pids.flatMap((p) => ["-p", String(p)])]);
  const fp = JSON.parse(readFileSync(json, "utf8"));
  rmSync(json);
  const ps = spawnSync(["ps", "-o", "rss=", "-p", pids.join(",")]).stdout.toString();
  const rssKB = ps.trim().split("\n").reduce((s, l) => s + Number(l.trim() || 0), 0);
  return { footprintMB: fp["total footprint"] / 2 ** 20, rssMB: rssKB / 1024, processes: pids.length };
}

/** Total CPU seconds used so far by a process tree. ps prints [hh:]mm:ss.ss */
function cpuSeconds(pid: number): number {
  const out = spawnSync(["ps", "-o", "time=", "-p", processTree(pid).join(",")]).stdout.toString();
  return out
    .trim()
    .split("\n")
    .reduce((sum, t) => sum + t.trim().split(":").reduce((acc, part) => acc * 60 + Number(part), 0), 0);
}

async function stop(proc: Subprocess) {
  proc.kill("SIGTERM");
  const exited = await Promise.race([proc.exited.then(() => true), sleep(5000).then(() => false)]);
  if (!exited) proc.kill("SIGKILL");
  await sleep(1000); // let helpers go before the next run
}

// ---- marks --------------------------------------------------------------

type Mark = { name: string; ms: number; pid: number };

function readMarks(log: string): Mark[] {
  if (!existsSync(log)) return [];
  return readFileSync(log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l) as Mark);
}

async function waitForMark(log: string, name: string, timeoutMs: number, proc?: Subprocess): Promise<Mark> {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    const m = readMarks(log).find((m) => m.name === name);
    if (m) return m;
    if (proc && proc.exitCode !== null) throw new Error(`exited (${proc.exitCode}) before mark ${name}`);
    await sleep(20);
  }
  throw new Error(`no ${name} mark within ${timeoutMs} ms`);
}

// ---- launching ------------------------------------------------------------

type Session = { proc: Subprocess; log: string; t0: number; profile: string; cdpPort?: number };

let nextPort = 9400;

/** Starts a variant with a fresh profile, so every run is a cold start for the app (the OS file cache stays warm). */
function launch(v: Variant, env: Record<string, string>, urls: string[] = []): Session {
  const profile = join(root, "target/bench-profiles", v);
  rmSync(profile, { recursive: true, force: true });
  mkdirSync(profile, { recursive: true });
  const log = join(profile, "..", `${v}.marks.jsonl`);
  rmSync(log, { force: true });
  let cmd: string[];
  let cdpPort: number | undefined;
  if (v === "helium") {
    cdpPort = nextPort++;
    cmd = [
      executable(v),
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      `--remote-debugging-port=${cdpPort}`,
      ...urls,
    ];
  } else {
    cmd = [executable(v), ...urls.slice(0, 1)];
  }
  const t0 = Date.now();
  const proc = spawn(cmd, {
    env: { ...process.env, JAAL_BENCH_LOG: log, JAAL_PROFILE_DIR: profile, ...env },
    stdout: "ignore",
    stderr: "ignore",
  });
  return { proc, log, t0, profile, cdpPort };
}

// ---- Helium through the DevTools protocol ---------------------------------

async function cdpPages(port: number): Promise<{ webSocketDebuggerUrl: string; url: string }[]> {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/json/list`);
    return ((await r.json()) as any[]).filter((t) => t.type === "page");
  } catch {
    return [];
  }
}

function cdpEval(wsUrl: string, expression: string): Promise<any> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    const timer = setTimeout(() => (ws.close(), reject(new Error("cdp timeout"))), 5000);
    ws.onopen = () =>
      ws.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: { expression, returnByValue: true } }));
    ws.onmessage = (e) => {
      const msg = JSON.parse(String(e.data));
      if (msg.id !== 1) return;
      clearTimeout(timer);
      ws.close();
      resolve(msg.result?.result?.value);
    };
    ws.onerror = () => (clearTimeout(timer), reject(new Error("cdp error")));
  });
}

/** Waits until `count` test pages have finished loading; returns the latest loadEventEnd (epoch ms). */
async function heliumLoaded(port: number, count: number, timeoutMs: number): Promise<number> {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    const pages = (await cdpPages(port)).filter((p) => p.url.startsWith("file://"));
    if (pages.length >= count) {
      const ends = await Promise.all(
        pages.map((p) => cdpEval(p.webSocketDebuggerUrl, "performance.timing.loadEventEnd").catch(() => 0)),
      );
      if (ends.every((e) => e > 0)) return Math.max(...ends);
    }
    await sleep(50);
  }
  throw new Error(`helium: ${count} pages not loaded within ${timeoutMs} ms`);
}

// ---- tests ----------------------------------------------------------------

type Sample = Record<string, number>;

async function testStart(v: Variant): Promise<Sample> {
  const s = launch(v, {}, [pageUrl(1)]);
  try {
    const painted =
      v === "helium" ? await heliumLoaded(s.cdpPort!, 1, 30000) : (await waitForMark(s.log, "first_paint", 30000, s.proc)).ms;
    return { start_ms: painted - s.t0 };
  } finally {
    await stop(s.proc);
  }
}

/** Memory after 10 tabs load and settle, then idle CPU over the next 60 s. */
async function testMemoryIdle(v: Variant, wantIdle: boolean): Promise<Sample> {
  const urls = Array.from({ length: 10 }, (_, i) => pageUrl(i + 1));
  const s = launch(v, { JAAL_BENCH_TABS: "10" }, urls);
  try {
    if (v === "helium") await heliumLoaded(s.cdpPort!, 10, 60000);
    else await waitForMark(s.log, "tabs_loaded", 60000, s.proc);
    await sleep(10000); // let startup work and GC settle
    const mem = memory(s.proc.pid);
    const out: Sample = { footprint_mb: mem.footprintMB, rss_mb: mem.rssMB, processes: mem.processes };
    if (wantIdle) {
      const c0 = cpuSeconds(s.proc.pid);
      await sleep(60000);
      out.idle_cpu_pct = ((cpuSeconds(s.proc.pid) - c0) / 60) * 100;
    }
    return out;
  } finally {
    await stop(s.proc);
  }
}

async function testSwitch(v: Variant): Promise<Sample> {
  const s = launch(v, { JAAL_BENCH_TABS: "10", JAAL_BENCH_SWITCH: "1" });
  await Promise.race([s.proc.exited, sleep(90000)]);
  if (s.proc.exitCode === null) await stop(s.proc);
  const marks = readMarks(s.log);
  const starts = marks.filter((m) => m.name === "tab_switch_start");
  const dones = marks.filter((m) => m.name === "tab_switch_done");
  if (starts.length !== 20 || dones.length !== 20)
    throw new Error(`expected 20 switch pairs, got ${starts.length} starts and ${dones.length} dones`);
  const lat = starts.map((st, i) => dones[i].ms - st.ms);
  return { switch_median_ms: median(lat), switch_max_ms: Math.max(...lat) };
}

/** Frame pacing during the variant's own 5 s auto-scroll or auto-resize. */
async function testFrames(v: Variant, mode: "scroll" | "resize"): Promise<Sample> {
  const env = { JAAL_BENCH_FRAMES: "1", [mode === "scroll" ? "JAAL_BENCH_SCROLL" : "JAAL_BENCH_RESIZE"]: "1" };
  const s = launch(v, env);
  await Promise.race([s.proc.exited, sleep(60000)]);
  if (s.proc.exitCode === null) await stop(s.proc);
  const frames = readMarks(s.log)
    .filter((m) => m.name === "frame")
    .map((m) => m.ms)
    .sort((a, b) => a - b);
  if (frames.length < 60) throw new Error(`only ${frames.length} frame marks`);
  const intervals = frames.slice(1).map((t, i) => t - frames[i]);
  const budget = 1000 / refreshHz;
  // A gap of 2.4 budgets means 1 missed frame, 3.4 means 2, and so on.
  const dropped = intervals.reduce((n, d) => n + Math.max(0, Math.round(d / budget) - 1), 0);
  return {
    [`${mode}_frames`]: frames.length,
    [`${mode}_dropped`]: dropped,
    [`${mode}_p95_ms`]: percentile(intervals, 0.95),
  };
}

// ---- stats and output -------------------------------------------------------

function median(xs: number[]): number {
  return percentile(xs, 0.5);
}
function percentile(xs: number[], p: number): number {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(p * s.length))];
}

const refreshHz = (() => {
  const out = spawnSync(["system_profiler", "SPDisplaysDataType"]).stdout.toString();
  return Number(out.match(/@ ([\d.]+)Hz/)?.[1] ?? 60);
})();

async function main() {
  const load = spawnSync(["sysctl", "-n", "vm.loadavg"]).stdout.toString().trim();
  console.log(`load average ${load}, display ${refreshHz} Hz, ${runs} runs`);
  for (const v of variants) if (!existsSync(executable(v))) throw new Error(`missing ${executable(v)}`);

  const samples: Record<string, Sample[]> = Object.fromEntries(variants.map((v) => [v, []]));
  const errors: string[] = [];
  for (let run = 0; run < runs; run++) {
    // Rotate the order each run so no variant always goes first.
    const order = variants.map((_, i) => variants[(i + run) % variants.length]);
    for (const v of order) {
      const sample: Sample = {};
      const vt = tests.filter((t) => v !== "helium" || HELIUM_TESTS.includes(t));
      const steps: [string, () => Promise<Sample>][] = [];
      if (vt.includes("start")) steps.push(["start", () => testStart(v)]);
      if (vt.includes("memory") || vt.includes("idle"))
        steps.push(["memory", () => testMemoryIdle(v, vt.includes("idle"))]);
      if (vt.includes("switch")) steps.push(["switch", () => testSwitch(v)]);
      if (vt.includes("scroll")) steps.push(["scroll", () => testFrames(v, "scroll")]);
      if (vt.includes("resize")) steps.push(["resize", () => testFrames(v, "resize")]);
      for (const [name, step] of steps) {
        try {
          Object.assign(sample, await step());
        } catch (e) {
          errors.push(`run ${run + 1} ${v} ${name}: ${(e as Error).message}`);
        }
      }
      samples[v].push(sample);
      console.log(`run ${run + 1} ${v}`, JSON.stringify(sample));
    }
  }

  const metrics = [...new Set(Object.values(samples).flat().flatMap((s) => Object.keys(s)))];
  const rows = metrics.map((m) => {
    const cells = variants.map((v) => {
      const xs = samples[v].map((s) => s[m]).filter((x) => x !== undefined);
      if (!xs.length) return "n/a";
      return `${median(xs).toFixed(1)} (${Math.min(...xs).toFixed(1)}–${Math.max(...xs).toFixed(1)}, n=${xs.length})`;
    });
    return `| ${m} | ${cells.join(" | ")} |`;
  });
  const date = new Date().toISOString().slice(0, 10);
  const table = [
    `# Bake-off results, ${date}, ${hostname()}`,
    "",
    `Median (min–max, runs) per metric. Load average at start: ${load}. Display: ${refreshHz} Hz.`,
    "",
    `| metric | ${variants.join(" | ")} |`,
    `|---|${variants.map(() => "---").join("|")}|`,
    ...rows,
    "",
    errors.length ? `## Errors\n\n${errors.map((e) => `- ${e}`).join("\n")}` : "No errors.",
    "",
  ].join("\n");

  mkdirSync(resultsDir, { recursive: true });
  const base = join(resultsDir, `${date}-${hostname().split(".")[0]}`);
  writeFileSync(`${base}.json`, JSON.stringify({ date, load, refreshHz, runs, samples, errors }, null, 2));
  writeFileSync(`${base}.md`, table);
  console.log(`\n${table}\nwrote ${base}.md and .json`);
}

await main();
