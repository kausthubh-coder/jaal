// `bun engine/scripts/build.ts [--jobs N]`: build the engine in the Chromium
// checkout (out/Release, args from engine/args), after apply.ts.
//
// Keeps going past failures (-k 0) so one run shows every compile error.
// Windows only for now; the Mac engine builds on Depot. Output and timing go
// to <checkout>/../build.log as well as the terminal.

import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { cpus } from "node:os";
import { dirname, join } from "node:path";
import { ENGINE, chromiumSrc } from "./lib";

if (process.platform !== "win32") {
  console.error("build.ts builds the Windows engine. Mac engine builds run on Depot.");
  process.exit(1);
}

const src = chromiumSrc();
const out = join(src, "out", "Release");
const args = readFileSync(join(ENGINE, "args", "windows-x64-dev.gn"), "utf8");
// Changing args.gn makes GN regenerate and can rebuild everything, so only
// write it when it differs.
if (!existsSync(join(out, "args.gn")) || readFileSync(join(out, "args.gn"), "utf8") !== args) {
  writeFileSync(join(out, "args.gn"), args);
  console.log(`wrote ${join(out, "args.gn")}`);
}

const jobsFlag = process.argv.indexOf("--jobs");
// The laptop has 15 GB of RAM: 12 jobs left about 2 GB free in M0.
const jobs = jobsFlag > 0 ? process.argv[jobsFlag + 1] : String(Math.max(1, cpus().length - 4));

const depotTools = process.env.DEPOT_TOOLS ?? "C:\\src\\depot_tools";
const log = join(dirname(src), "build.log");
const started = new Date();
writeFileSync(log, `start ${started.toISOString()} jobs ${jobs}\n`);

const proc = Bun.spawn(["cmd", "/c", "autoninja", "-C", out, "chrome", "-j", jobs, "-k", "0"], {
  cwd: src,
  env: {
    ...process.env,
    PATH: `${depotTools};${process.env.PATH}`,
    DEPOT_TOOLS_WIN_TOOLCHAIN: "0",
    vs2026_install: "C:\\Program Files\\Microsoft Visual Studio\\18\\Community",
  },
  stdout: "pipe",
  stderr: "pipe",
});

async function pump(stream: ReadableStream<Uint8Array>) {
  const decoder = new TextDecoder();
  for await (const chunk of stream) {
    const text = decoder.decode(chunk);
    process.stdout.write(text);
    appendFileSync(log, text);
  }
}
await Promise.all([pump(proc.stdout), pump(proc.stderr)]);
const code = await proc.exited;
const minutes = ((Date.now() - started.getTime()) / 60000).toFixed(1);
appendFileSync(log, `exit ${code} after ${minutes} min\n`);
console.log(`exit ${code} after ${minutes} min`);
process.exit(code);
