// Writes the 10 local test pages in bakeoff/pages. Run it again after
// changing this file: `bun scripts/make-pages.ts`.
// The pages are static on purpose (no timers, no animation), so the idle
// CPU test measures the browser, not the page.

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const dir = join(import.meta.dir, "..", "bakeoff", "pages");
mkdirSync(dir, { recursive: true });

const words =
  "the browser draws each frame after the page changes and the compositor stacks every layer before the display shows it".split(
    " ",
  );

function paragraph(seed: number): string {
  const out: string[] = [];
  for (let i = 0; i < 60; i++) out.push(words[(seed * 7 + i * 3) % words.length]);
  return out.join(" ");
}

function bars(seed: number): string {
  const rects = Array.from({ length: 24 }, (_, i) => {
    const h = 20 + ((seed * 13 + i * 29) % 140);
    return `<rect x="${i * 25}" y="${160 - h}" width="18" height="${h}" rx="3"/>`;
  }).join("");
  return `<svg viewBox="0 0 600 160" width="600" height="160" fill="#4f7cff">${rects}</svg>`;
}

for (let n = 1; n <= 10; n++) {
  const sections = Array.from({ length: 12 }, (_, s) => {
    const rows = Array.from(
      { length: 8 },
      (_, r) => `<tr><td>Item ${s}.${r}</td><td>${(n * 31 + s * 7 + r) % 997}</td><td>${paragraph(n + s + r).slice(0, 40)}</td></tr>`,
    ).join("");
    return `<section>
<h2>Section ${s + 1}</h2>
<p>${paragraph(n * 10 + s)}</p>
<p>${paragraph(n * 20 + s)}</p>
${s % 3 === 0 ? bars(n + s) : ""}
<table><thead><tr><th>Name</th><th>Value</th><th>Note</th></tr></thead><tbody>${rows}</tbody></table>
</section>`;
  }).join("\n");

  const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<title>Test page ${n}</title>
<style>
  body { font: 16px/1.6 -apple-system, "Segoe UI", sans-serif; margin: 0; color: #1b1c1f; background: #fafafa; }
  header { background: linear-gradient(135deg, hsl(${n * 36}, 70%, 55%), hsl(${n * 36 + 40}, 70%, 45%)); color: white; padding: 48px 32px; }
  main { max-width: 760px; margin: 0 auto; padding: 24px 32px; }
  table { border-collapse: collapse; width: 100%; margin: 16px 0; }
  td, th { border-bottom: 1px solid #ddd; padding: 6px 8px; text-align: left; }
  section { margin-bottom: 40px; }
</style></head>
<body>
<header><h1>Test page ${n}</h1><p>A static page for jaal's UI bake-off.</p></header>
<main>
${sections}
</main>
</body></html>
`;
  writeFileSync(join(dir, `page-${n}.html`), html);
}

console.log(`wrote 10 pages to ${dir}`);
