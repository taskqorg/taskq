// Chain: taskq.resources/1 projection -> shared fixtures -> src/resource-controls.js states. Risk: unknown reads as
// zero or Ready, a legacy guard reads as Auto, an untouched field travels, a retry mints a new operation.
// Run: node tests/resource-controls.test.cjs. Consumers (tray, DashQ) replay the same fixture file.
const fs = require('node:fs'), crypto = require('node:crypto'), assert = require('node:assert/strict');
const rc = require('../src/resource-controls.js');
const fx = JSON.parse(fs.readFileSync('tests/fixtures/resource-controls.json', 'utf8'));
for (const c of fx.cases) {
  const checks = c.events.filter(e => e.type === 'check');
  const got = rc.replay(c, () => null);
  assert.equal(got.length, checks.length, c.name);
  checks.forEach((want, i) => {
    const g = got[i], at = c.name + ' check ' + (i + 1);
    if (want.status) assert.equal(g.status, want.status, at);
    if (want.headline) assert.equal(g.headline, want.headline, at);
    if (want.sent != null) assert.equal(g.sent.length, want.sent, at);
    for (const s of want.has || []) assert(g.html.includes(s), at + ' lacks ' + s + '\n' + g.html);
    for (const s of want.not || []) assert(!g.html.includes(s), at + ' has ' + s + '\n' + g.html);
    const last = g.sent[g.sent.length - 1];
    if (want.ops) assert.deepEqual(last.ops, want.ops, at + ': one contract-shaped change per target, untouched fields stay');
    if (want.same_op) assert.deepEqual(last, g.sent[g.sent.length - 2], at + ': a retry keeps the id and payload');
  });
  assert(rc.STATES.includes(got[got.length - 1].status));
  console.log('PASS', c.name);
}
// #954 Balancing (menu M12): selection, Mixed, one operation and receipt per selected project
for (const c of fx.work) {
  const checks = c.events.filter(e => e.type === 'check');
  const got = rc.replayWork(c, () => null);
  assert.equal(got.length, checks.length, c.name);
  checks.forEach((want, i) => {
    const g = got[i], at = c.name + ' check ' + (i + 1);
    if (want.status) assert.equal(g.status, want.status, at);
    if (want.each) assert.deepEqual(g.each, want.each, at + ': per-project outcome');
    if (want.sent != null) assert.equal(g.sent.length, want.sent, at);
    for (const s of want.has || []) assert(g.html.includes(s), at + ' lacks ' + s + '\n' + g.html);
    for (const s of want.not || []) assert(!g.html.includes(s), at + ' has ' + s + '\n' + g.html);
    if (want.order) { const at2 = want.order.map(s => g.html.indexOf('>' + s + '<')); assert(at2.every((x, k) => x >= 0 && (!k || x > at2[k - 1])), at + ': parent order ' + want.order + ' ' + at2); }
    if (want.ops) assert.deepEqual(Object.fromEntries(g.sent.map(x => [x.key, x.op.ops])), want.ops, at + ': selected projects only, untouched fields stay');
    if (want.same_op) { const s = g.sent.filter(x => x.key === want.same_op); assert.deepEqual(s[s.length - 1], s[s.length - 2], at + ': a retry keeps the id and payload'); }
  });
  console.log('PASS', c.name);
}
// A1: the child keeps the parent design's Balancing order (docs/design/menu-next-variants.html work/resourceEditor):
// project selection, then Mode, then the host maximum, then per-runtime overrides
const parent = fs.readFileSync('docs/design/menu-next-variants.html', 'utf8');
const fn = name => { const a = parent.indexOf('function ' + name + '('); return parent.slice(a, parent.indexOf('\nfunction ', a + 1)); };
const inOrder = (text, xs) => { const at = xs.map(s => text.indexOf(s)); return at.every((x, k) => x >= 0 && (!k || x > at[k - 1])); };
assert(inOrder(fn('work'), ['selected projects', '<input type="checkbox"', 'resourceEditor(s)', 'Review changes']), 'parent Balancing order changed');
assert(inOrder(fn('resourceEditor'), ['>Mode', 'Host maximum sessions', 'Runtime maximum sessions']), 'parent resource editor order changed');
console.log('PASS parent/child Balancing order');
// §7/#1841: release derives source hashes; retain stale-bundle detection and fixture replay protection.
const doc = fs.readFileSync('docs/design/shared-resource-controls.md', 'utf8');
for (const f of ['src/resource-controls.js', 'src/resource-controls.css', 'tests/fixtures/resource-controls.json']) {
  const h = crypto.createHash('sha256').update(fs.readFileSync(f, 'utf8').replace(/\r\n/g, '\n')).digest('hex');
  if(f==='src/resource-controls.js')assert.equal(JSON.parse(fs.readFileSync('build/web/task-workspace.sources.json','utf8')).files[f],h,'resource source hash must match the built release manifest');
  else assert(doc.includes('`' + f + '` SHA-256: `' + h + '`'), f + ' checksum ' + h + ' is not recorded in docs/design/shared-resource-controls.md');
}
console.log('PASS asset checksums');
// #1282 the overlay projection's observed_at is its status readback time: 61 s later applied is unknown, never applied
{
  const p = JSON.parse(fs.readFileSync('tests/fixtures/resource-policy-v1.json', 'utf8')).overlay_projection;
  assert.equal(rc.applied(p, p.observed_at), 'applied');
  assert.equal(rc.applied(p, p.observed_at + 61), 'unknown');
  console.log('PASS overlay projection readback ages from its status time');
}
