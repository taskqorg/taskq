// MP11–MP13/O2/O3: per-view working context, exact Save/Reset, restart, canonical ties and no path-based scope.
const assert=require('node:assert/strict'),views=require('../src/task-views.js');
const a={issues:'https://forge.example/org/a/issues',board_kind:'github',root:'/one'},b={...a,root:'/two'};
assert.equal(views.identity(a),views.identity(b));
assert.equal(views.identity({...a,issues:'https://forge.example/org/issues-helper/issues'}),'github://forge.example/org/issues-helper');
assert.equal(views.identity({...a,issues:'https://forge.example/org/a/-/issues',board_kind:'gitlab'}),'gitlab://forge.example/org/a');
assert.notEqual(views.identity(a),views.identity({...a,board_kind:'gitlab'}));
const m=views.normalize(null,{taskFilter:['doing'],scroll:50}),all=views.current(m);
assert.deepEqual(all.working.states,['doing']);assert.equal(views.dirty(all),true);
all.working.projects=[views.identity(a)];all.working.query='needle';
assert(views.add(m,' Focus ','focus'));const focus=views.current(m);focus.working.states=['ask','failed'];focus.working.query='unsaved';
m.selected='all';assert.equal(views.current(m).working.query,'needle');m.selected='focus';
const restored=views.normalize(JSON.parse(JSON.stringify(m)));assert.deepEqual(restored,m);
views.reset(restored);assert.equal(views.current(restored).working.query,'needle');
focus.working.query='saved';views.save(m);assert.equal(focus.saved.query,'saved');assert.equal(views.dirty(focus),false);
assert.deepEqual(views.scope([a,b,{...a,issues:'https://other.example/org/b/issues'}],all.working),[a,b]);
// #1592: the page keeps the display model's `rank` (state, priority, number, board: display::tests); unranked rows last.
const rows=[{n:2,rank:3},{n:5},{n:2,rank:2},{n:9,rank:1},{n:12,rank:0}];
assert.deepEqual(rows.sort(views.compare).map(r=>r.rank),[0,1,2,3,undefined]);
assert(views.matches({title:'A NEEDLE',project:'Alpha'},'needle'));assert(!views.matches({title:'other'},'needle'));
assert(!views.add(m,'','bad'));assert.equal(views.normalize({views:[{id:'toString',name:'Bad'}]}).selected,'all');
// M2: old navigation defaults expanded; malformed collapse state cannot enter the native document.
assert.deepEqual(views.normalize(null).collapsed,[]);
assert.deepEqual(views.normalize({collapsed:[views.identity(a),false,views.identity(a),'x'.repeat(2049)]}).collapsed,[views.identity(a)]);
const collapsed=views.normalize({...m,collapsed:[views.identity(a)]});assert.deepEqual(views.normalize(JSON.parse(JSON.stringify(collapsed))).collapsed,collapsed.collapsed);
console.log('PASS O2/O3 canonical comparator, OR/project/search scope, migration, independent working views, Save/Reset/restart');
