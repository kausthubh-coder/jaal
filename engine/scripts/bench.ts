// `bun engine/scripts/bench.ts [--runs 5] [--tests start,memory,idle,scroll]`:
// the bake-off's tests (docs/bakeoff.md) that jaal on the engine can run
// today, on Windows. It prints one line per run and a median and range per
// number, and writes them to target/bench/engine-windows.json.
//
// Needs a built engine and `cargo build -p jaal-ui --release`.
//
// What each test measures:
// - start: from just before the process is created to the first tab's load
//   finishing (the engine's event), with bakeoff page 1 and the 5,000-row
//   sidebar. The bake-off's own start is `main` to `first_paint`, measured
//   inside the process; this one also counts process creation, and ends at
//   the load event because the engine has no first-paint event yet.
// - memory: 10 s after that, the private working set of the browser and
//   every process under it, added up. Private pages are counted once each;
//   shared ones not at all.
// - idle: CPU time the same processes use over the next 60 s, as a share of
//   one core.
// - scroll: the UI scrolls its 5,000 rows by 40 px a frame for 5 s and marks
//   every frame it draws (the bake-off's scroll run). Dropped frames are the
//   gaps longer than a frame at the display's refresh rate.
//
// Not here: tab switch (needs a page-shown event from the engine) and resize
// (the page is Chrome's to redraw, and Chrome's frames aren't marked).

import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { UI_LIBRARY, launch, profileDir, ps, psCommand, sleep, stopTree, waitFor } from "./accept";
import { REPO } from "./lib";

type Test = "start" | "memory" | "idle" | "scroll";
const arg = (name: string) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : undefined;
};
const runs = Number(arg("runs") ?? 5);
const tests = (arg("tests")?.split(",") ?? ["start", "memory", "idle", "scroll"]) as Test[];
const out = join(REPO, "target", "bench");
mkdirSync(out, { recursive: true });

const ui = join(REPO, "target", "release", "jaal_ui.dll");
copyFileSync(ui, UI_LIBRARY);
const page = pathToFileURL(join(REPO, "bakeoff", "pages", "page-1.html")).href;
if (!existsSync(join(REPO, "bakeoff", "pages", "page-1.html"))) throw new Error("no bakeoff/pages; run `bun run pages`");

const refreshHz = Number(psCommand("(Get-CimInstance Win32_VideoController | Select-Object -First 1).CurrentRefreshRate")) || 60;

type Mark = { name: string; ms: number; pid: number };
function marks(log: string): Mark[] {
  if (!existsSync(log)) return [];
  return readFileSync(log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as Mark);
}

/// CPU seconds, private working set and process count of `pid` and every
/// process under it.
function tree(pid: number): { cpu_s: number; private_mb: number; processes: number } {
  const text = psCommand(`
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
    $ids = @(${pid})
    for ($i = 0; $i -lt $ids.Count; $i++) { $ids += @($all | Where-Object { $_.ParentProcessId -eq $ids[$i] } | ForEach-Object { $_.ProcessId }) }
    $perf = Get-CimInstance Win32_PerfRawData_PerfProc_Process | Where-Object { $ids -contains $_.IDProcess }
    $cpu = (Get-Process -Id $ids -ErrorAction SilentlyContinue | Measure-Object -Property CPU -Sum).Sum
    "$cpu $(($perf | Measure-Object -Property WorkingSetPrivate -Sum).Sum / 1MB) $($ids.Count)"
  `);
  const [cpu_s, private_mb, processes] = text.split(/\s+/).map(Number);
  return { cpu_s, private_mb, processes };
}

function start(name: string, env: Record<string, string>) {
  const log = join(out, `${name}.jsonl`);
  rmSync(log, { force: true });
  // Keeps a screen saver from starting mid-run: it would stop the window
  // from being composed.
  ps("wake");
  ps("move", 400 + Math.round(Math.random() * 200), 400);
  const t0 = Date.now();
  const proc = launch(profileDir("engine-bench", name), {
    JAAL_UI_URL: page,
    JAAL_UI_FAKE_ROWS: "5000",
    JAAL_BENCH_LOG: log,
    ...env,
  });
  return { proc, log, t0 };
}

type Sample = Record<string, number>;
const samples: Sample[] = [];

for (let run = 1; run <= runs; run++) {
  const sample: Sample = {};

  if (tests.some((t) => t === "start" || t === "memory" || t === "idle")) {
    const s = start(`run-${run}`, {});
    try {
      const loaded = await waitFor("the first tab to load", 60_000, () => marks(s.log).find((m) => m.name === "first_load"));
      const uiMain = marks(s.log).find((m) => m.name === "ui_main");
      sample.start_ms = loaded.ms - s.t0;
      if (uiMain) sample.start_to_ui_ms = uiMain.ms - s.t0;
      if (tests.includes("memory") || tests.includes("idle")) {
        await sleep(10_000);
        const before = tree(s.proc.pid);
        sample.memory_mb = before.private_mb;
        sample.processes = before.processes;
        if (tests.includes("idle")) {
          const t = Date.now();
          await sleep(60_000);
          const after = tree(s.proc.pid);
          sample.idle_cpu_pct = ((after.cpu_s - before.cpu_s) / ((Date.now() - t) / 1000)) * 100;
        }
      }
      // The numbers mean nothing if the browser died on the way.
      if (s.proc.exitCode !== null) throw new Error(`the browser exited with code ${s.proc.exitCode}`);
    } finally {
      stopTree(s.proc.pid);
      await sleep(1500);
    }
  }

  if (tests.includes("scroll")) {
    const s = start(`scroll-${run}`, { JAAL_BENCH_SCROLL: "1", JAAL_BENCH_FRAMES: "1" });
    // The UI quits by itself when its 5 s are up.
    const code = await Promise.race([s.proc.exited, sleep(60_000).then(() => "timeout" as const)]);
    if (code === "timeout") stopTree(s.proc.pid);
    const frames = marks(s.log)
      .filter((m) => m.name === "frame")
      .map((m) => m.ms)
      .sort((a, b) => a - b);
    if (code !== 0 || frames.length < 60) throw new Error(`scroll run ${run}: exit ${code}, ${frames.length} frame marks`);
    const intervals = frames.slice(1).map((t, i) => t - frames[i]);
    const budget = 1000 / refreshHz;
    // A gap of 2.4 budgets is 1 missed frame, 3.4 is 2, and so on (the
    // bake-off's rule).
    sample.scroll_frames = frames.length;
    sample.scroll_dropped = intervals.reduce((n, d) => n + Math.max(0, Math.round(d / budget) - 1), 0);
    sample.scroll_p95_ms = [...intervals].sort((a, b) => a - b)[Math.floor(intervals.length * 0.95)];
    await sleep(1500);
  }

  samples.push(sample);
  console.log(`run ${run}: ${Object.entries(sample).map(([k, v]) => `${k} ${v.toFixed(1)}`).join(", ")}`);
}

const median = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};
const summary: Record<string, { median: number; min: number; max: number }> = {};
for (const key of Object.keys(samples[0] ?? {})) {
  const values = samples.map((s) => s[key]);
  summary[key] = { median: median(values), min: Math.min(...values), max: Math.max(...values) };
  console.log(`${key}: median ${summary[key].median.toFixed(1)}, range ${summary[key].min.toFixed(1)} to ${summary[key].max.toFixed(1)}`);
}
writeFileSync(join(out, "engine-windows.json"), JSON.stringify({ runs, refreshHz, samples, summary }, null, 2));
console.log(`display ${refreshHz} Hz; wrote ${join(out, "engine-windows.json")}`);
