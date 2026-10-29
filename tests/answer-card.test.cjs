// Chain: canonical question (cargo test) -> shared fixtures -> src/answer-card.js states. Risk: a duplicate click sends
// twice, a stale draft sends to a new question, or a consequential choice is answerable outside the board.
// Run: node tests/answer-card.test.cjs. Consumers (tray, DashQ) run the same replay over the same fixture file.
const fs = require('node:fs'), crypto = require('node:crypto'), assert = require('node:assert/strict');
const card = require('../src/answer-card.js');
const fx = JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json', 'utf8'));
// #1816 / § 5: human replies must send without imposing the board's formatting on the writer.
for (const text of [('word '.repeat(53)+'ok'), 'word\n'.repeat(400)]) {
  assert.equal(card.textProblem({limits:{lines:4,chars:240}},text),null);
}
assert.equal(card.textProblem({},''),'Type an answer');
for (const c of fx.cases) {
  const checks = c.events.filter(e => e.type === 'check');
  const got = card.replay(c, (_, row) => row.capability || c.capability);
  assert.equal(got.length, checks.length, c.name);
  checks.forEach((want, i) => {
    const g = got[i], at = c.name + ' check ' + (i + 1);
    if (want.status) assert.equal(g.status, want.status, at);
    if (want.sent != null) assert.equal(g.sent.length, want.sent, at);
    if (want.draft != null) assert.equal(g.draft, want.draft, at);
    for (const s of want.has || []) assert(g.html.includes(s), at + ' lacks ' + s + '\n' + g.html);
    for (const s of want.not || []) assert(!g.html.includes(s), at + ' has ' + s + '\n' + g.html);
    const last = g.sent[g.sent.length - 1];
    if (want.op) for (const [k, v] of Object.entries(want.op)) assert.equal(last[k], v, at + ' op.' + k);
    if (want.same_op) assert.deepEqual(last, g.sent[g.sent.length - 2], at + ': a retry keeps the id and payload');
    if (want.new_op) assert.notEqual(last.id, g.sent[g.sent.length - 2].id, at + ': a changed answer is a new operation');
  });
  assert(card.STATES.includes(got[got.length - 1].status));
  console.log('PASS', c.name);
}
// #1782: ask images show as thumbnails outside Details and open full size; without the app's copy the link stays;
// only a data:image source renders. Risk: the owner decides a visual ask without seeing its images.
{
  const q = {parsed: true, problem: 'p', choices: [{id: 1, text: 'a', recommended: true}, {id: 2, text: 'b'}], limits: {lines: 4, chars: 240}, evidence: [
    {label: '#1', url: 'https://x/1', image: false}, {label: 'front', url: 'https://g/-/project/1/uploads/s/f.png#upload=2', image: true, src: 'data:image/png;base64,AAAA'},
    {label: 'side', url: 'https://g/s.png', image: true}, {label: 'bad', url: 'https://g/b.png', image: true, src: 'javascript:x'}]};
  const row = {n: 7, title: 'T', state: 'ask', revision: 1, url: 'https://g/7', question: q};
  const html = card.render('b', row, card.fresh(row), {answer: true});
  const img = html.indexOf('<img class="qac-thumb" src="data:image/png;base64,AAAA" alt="front">');
  assert(img > 0 && img < html.indexOf('<details'), html);
  assert(html.includes('href="https://g/-/project/1/uploads/s/f.png#upload=2"') && html.includes('side ↗</a>') && html.includes('bad ↗</a>') && !html.includes('javascript:'), html);
  assert(html.indexOf('>#1</a>') > html.indexOf('<details class="qac-context"'), 'links stay behind Details');
  console.log('PASS ask images');
}
// #1787: a labelled fixture option (the Russian card included) stays within § 2 limits, counted in characters, and
// its button shows only N. and the label. Risk: a long or cut option on a narrow surface hides what the owner picks.
for (const c of fx.cases) for (const r of c.rows) for (const ch of (r.question && r.question.choices) || []) {
  if (ch.detail == null) continue;
  assert([...ch.label].length <= 32 && [...(ch.id + '. ' + ch.text + ' ★')].length <= 160, c.name + ': option ' + ch.id + ' over § 2 limits');
  const html = card.render(c.board, r, card.fresh(r), c.capability), b = html.indexOf('data-choice="' + ch.id + '"');
  const button = html.slice(b, html.indexOf('</button>', b));
  assert(button.includes(ch.label) && !button.includes(ch.detail) && html.includes('>' + ch.detail + '</span>'), c.name + ': option ' + ch.id + ' button or detail');
}
console.log('PASS labelled options');
// A2 / section 7: derive provenance at build time, preserving exact source and released-file protection.
const manifest=JSON.parse(fs.readFileSync('docs/design/answer-card.sources.json','utf8'));
const built=JSON.parse(fs.readFileSync('build/web/answer-card.sources.json','utf8'));
assert.deepEqual(manifest.files,['src/answer-card.js','src/reply-files.js','src/answer-card.css','tests/fixtures/answer-card.json']);
assert.deepEqual(Object.keys(built.files),manifest.files);
for(const [file,hash] of Object.entries(built.files))assert.equal(crypto.createHash('sha256').update(fs.readFileSync(file,'utf8').replace(/\r\n/g,'\n')).digest('hex'),hash,file);
const release=fs.readFileSync('.github/workflows/release.yml','utf8');
for(const file of ['build/web/answer-card.js','src/answer-card.css','tests/fixtures/answer-card.json','build/web/answer-card.sources.json'])assert(release.includes('cp '+file),'missing released file: '+file);
console.log('PASS asset checksums and release inclusion');
