// Chain: status row `controls` -> shared buttons -> op for the TaskQ dispatcher. Risk: a consumer offers an action the
// row does not list, invents Close authority, or drifts op identity from the fixture argv `cargo test` checks.
const fs=require('node:fs'),assert=require('node:assert/strict'),vm=require('node:vm');
const c=require('../src/task-controls.js'),fx=require('./fixtures/task-controls.json');
assert.equal(c.VERSION,fx.version);
assert.deepEqual(Object.keys(c.LABELS),fx.controls.map(x=>x.action));
for(const [state,controls] of Object.entries(fx.states)){
  const html=c.render({n:7,revision:20,state,controls},{key:'b/7',url:'https://github.com/o/r/issues/7'});
  assert.deepEqual([...html.matchAll(/data-control="(\w+)"/g)].map(m=>m[1]),controls,state);
  assert.ok(!/data-control="close"/.test(html)&&html.includes('aria-label="Close #7 on the board"'),'Close is only a board link');
}
assert.equal(c.render({n:7,revision:20,controls:['start','stop']},{except:['start']}).match(/data-control/g).length,1);
assert.equal(c.render({n:7,revision:20}),'');
// Chain: #1928 A5 / M3. Risk: provider guessing from a self-hosted hostname gives GitLab a GitHub close mark.
for(const board of ['github','gitlab'])assert(c.render({n:7},{url:'https://forge.example/o/r/-/issues/7',board}).includes('<title>'+({github:'GitHub',gitlab:'GitLab'}[board])+'</title>'));
assert(c.render({n:7},{url:'https://forge.example/o/r/issues/7'}).includes('lucide-external-link'),'unknown provider stays neutral');
for(const x of fx.cases)assert.deepEqual(c.op(x.row,x.op.action,'dash'),x.op);
const sandbox={setTimeout,clearTimeout,crypto:require('node:crypto').webcrypto};vm.createContext(sandbox);
vm.runInContext(fs.readFileSync('build/web/task-controls.js','utf8'),sandbox);
assert.equal(sandbox.TaskqTaskControls.render({...fx.cases[1].row,controls:fx.states.doing}),c.render({...fx.cases[1].row,controls:fx.states.doing}));
assert.match(fs.readFileSync('.github/workflows/release.yml','utf8'),/task-controls-fixtures\.json/);
console.log('PASS controls follow row state, Close stays on the board, op identity matches fixture, bundle and release wiring');
