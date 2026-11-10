// `bun scripts/demo-windows.ts <exe> <out-dir> [prefix]`: runs a GPUI
// variant's scripted demo (JAAL_DEMO=1) on Windows and saves a screenshot
// of the window after each step as <out-dir>/<prefix>-<step>.png. The demo
// prints `DEMO <step> ...` on stderr and waits for this script's ack before
// it goes on. It sends real keys and clicks, so leave the laptop alone while
// it runs (about a minute).

import { appendFileSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const [exe, outDir, prefix = "demo"] = process.argv.slice(2);
if (!exe || !outDir) {
  console.error("usage: bun scripts/demo-windows.ts <exe> <out-dir> [prefix]");
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });
const ackDir = mkdtempSync(join(tmpdir(), "jaal-demo-"));
const shot = join(import.meta.dir, "window-shot.ps1");

const app = Bun.spawn([resolve(exe)], {
  env: { ...process.env, JAAL_DEMO: "1", JAAL_DEMO_ACK_DIR: ackDir },
  stdout: "ignore",
  stderr: "pipe",
});
console.log(`pid ${app.pid}`);

// Quit the app we started if the demo hangs.
const deadline = setTimeout(() => {
  console.error("demo timed out after 180 s, killing it");
  app.kill();
}, 180_000);

const logFile = join(outDir, `${prefix}-stderr.log`);
writeFileSync(logFile, "");
// CEF's helper processes inherit stderr, so the pipe can outlive the app.
// Stop reading once the app itself has exited.
let buffered = "";
const decoder = new TextDecoder();
const reader = app.stderr.getReader();
while (true) {
  const next = await Promise.race([
    reader.read(),
    app.exited.then(() => Bun.sleep(1000)).then(() => ({ done: true, value: undefined })),
  ]);
  if (next.done || !next.value) break;
  const chunk = next.value;
  buffered += decoder.decode(chunk);
  let newline: number;
  while ((newline = buffered.indexOf("\n")) >= 0) {
    const line = buffered.slice(0, newline).trimEnd();
    buffered = buffered.slice(newline + 1);
    appendFileSync(logFile, `${line}\n`);
    if (!line.startsWith("DEMO ")) continue;
    console.log(line);
    const step = line.split(" ")[1];
    // Step lines carry state; "DEMO <what>: ok after..." lines don't.
    if (step.endsWith(":") || !line.includes(" tabs=")) continue;
    const out = join(outDir, `${prefix}-${step}.png`);
    // Hidden: a console window would take the foreground from the app.
    const r = Bun.spawnSync(
      ["powershell", "-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-File", shot, String(app.pid), out],
      { windowsHide: true },
    );
    console.log(r.exitCode === 0 ? `  shot ${out}` : `  shot failed: ${r.stderr.toString().trim()}`);
    writeFileSync(join(ackDir, step), "");
  }
}
const code = await app.exited;
clearTimeout(deadline);
console.log(`exited with ${code}`);
