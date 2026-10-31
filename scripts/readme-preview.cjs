// README preview for docs/shots/update.sh (#2024 A5): `node scripts/readme-preview.cjs README ROOT STAGE OUT`.
// Renders README to OUT/README.html, takes full-page screenshots at 1280 and 390 px with every disclosure open and
// writes OUT/page-checks.json. docs/shots/*.png resolve to STAGE, other files to ROOT. Fails when an image does not
// load or the page overflows horizontally. An instrument (taskq.md R12): a person still reviews the images and pages.
// ponytail: a Markdown subset (HTML blocks, ### headings, fenced code, paragraphs, code spans, links, *em*, **strong**)
// covers README.md without a renderer dependency; GitHub's Markdown API would make the preview a network call.
'use strict';
const fs = require('fs');
const path = require('path');
const {pathToFileURL} = require('url');
const [readme, root, stage, out] = process.argv.slice(2);
if (!out) throw new Error('usage: readme-preview.cjs README ROOT STAGE OUT');

const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const inline = s => s.split(/(`[^`]*`)/).map((part, i) => i % 2 ? `<code>${esc(part.slice(1, -1))}</code>` :
  esc(part).replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<a href="$2">$1</a>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>').replace(/\*([^*\s][^*]*)\*/g, '<em>$1</em>')).join('');

function markdown(text) {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  const html = [];
  for (let i = 0; i < lines.length;) {
    const line = lines[i];
    let m;
    if (!line.trim()) { i++; continue; }
    if ((m = line.match(/^```(\w*)/))) {
      const code = [];
      for (i++; i < lines.length && !lines[i].startsWith('```'); i++) code.push(lines[i]);
      i++;
      html.push(`<pre${m[1] ? ` lang="${m[1]}"` : ''}><code>${esc(code.join('\n'))}\n</code></pre>`);
    } else if ((m = line.match(/^(#{1,6})\s+(.*)/))) {
      html.push(`<h${m[1].length}>${inline(m[2])}</h${m[1].length}>`);
      i++;
    } else if (line.startsWith('<')) {
      for (; i < lines.length && lines[i].trim(); i++) html.push(lines[i]);
    } else {
      const para = [];
      for (; i < lines.length && lines[i].trim() && !/^(```|#{1,6}\s|<)/.test(lines[i]); i++) para.push(lines[i]);
      html.push(`<p>${inline(para.join('\n'))}</p>`);
    }
  }
  return html.join('\n');
}

const css = 'body{margin:0;background:#fff;color:#1f2328;font:16px/1.5 -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif}' +
  'article{padding:24px;max-width:900px;margin:auto;overflow-wrap:break-word}img{max-width:100%;height:auto}' +
  'pre{padding:16px;background:#f6f8fa;overflow:auto}code{font:85% ui-monospace,SFMono-Regular,Menlo,monospace}' +
  'a{color:#0969da}h3,h4{margin-top:24px}';
const page = (body, base) => `<!doctype html><html lang="en"><meta charset="utf-8">` +
  `<meta name="viewport" content="width=device-width, initial-scale=1">${base ? `<base href="${base}">` : ''}` +
  `<title>TaskQ README preview</title><style>${css}</style><article>\n${body}\n</article></html>\n`;
const body = markdown(fs.readFileSync(readme, 'utf8'));
// Rendered copy: staged README images and the REF's other files as absolute file URLs.
const local = body.replace(/(src|href)="([^"#:]+)"/g, (all, attr, rel) => {
  const file = /^docs\/shots\/[^/]+\.png$/.test(rel) ? path.join(stage, path.basename(rel)) : path.join(root, rel);
  return fs.existsSync(file) ? `${attr}="${pathToFileURL(file).href}"` : all;
});
fs.mkdirSync(out, {recursive: true});
// Retained copy: relative to the repository root from docs/shots/evidence/<version>/.
fs.writeFileSync(path.join(out, 'README.html'), page(body, '../../../../'));
const render = path.join(out, '.render.html');
fs.writeFileSync(render, page(local));

(async () => {
  const checks = [];
  let browser;
  try {
    browser = await require('playwright').chromium.launch(require('../tests/browser.cjs')());
    for (const width of [1280, 390]) {
      const p = await browser.newPage({viewport: {width, height: 900}});
      await p.goto(pathToFileURL(render).href, {waitUntil: 'load'});
      checks.push({width, ...await p.evaluate(() => {
        document.querySelectorAll('details').forEach(d => { d.open = true; });
        const doc = document.documentElement;
        return {overflow: doc.scrollWidth > doc.clientWidth,
          images: [...document.images].map(i => ({src: i.getAttribute('src').replace(/^.*\/(docs\/)/, '$1'),
            loaded: i.complete && i.naturalWidth > 0, width: Math.round(i.getBoundingClientRect().width)}))};
      })});
      await p.screenshot({path: path.join(out, `readme-${width}.png`), fullPage: true});
      await p.close();
    }
    const version = browser.version();
    fs.writeFileSync(path.join(out, 'page-checks.json'), JSON.stringify({
      renderer: 'scripts/readme-preview.cjs Markdown subset; Playwright Chromium', browser: version, checks}, null, 1) + '\n');
  } finally {
    await browser?.close();
    fs.rmSync(render, {force: true});
  }
  const bad = checks.flatMap(c => [...(c.overflow ? [`${c.width} px: horizontal overflow`] : []),
    ...c.images.filter(i => !i.loaded).map(i => `${c.width} px: ${i.src} did not load`)]);
  if (bad.length) { console.error(bad.join('\n')); process.exit(1); }
  console.log(`readme-preview: ${checks.length} widths, ${checks[0].images.length} images, no overflow: ${out}`);
})().catch(e => { console.error(e.message); process.exit(1); });
