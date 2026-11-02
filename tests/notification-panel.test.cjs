// Chain: M14 / #1964. Risk: duplicate registrations or close rows inflate the bell; similar series merge unrelated events.
const assert=require('node:assert/strict'),panel=require('../src/notification-panel.js');
const project={root:'one',name:'Project',issues:'https://example.org/issues',events:[[1,'ask','a',4],[1,'ask','a',4],[2,'close','b',5]],tasks:[{n:4,title:'A question'}]};
const result={root:'one',n:5,sha:'a'.repeat(40),closed:2,title:'Cadence v4 1/10',read:false};
const rows=panel.rows([project,{...project,root:'duplicate'}],[result]);
assert.equal(rows.length,2,'one notification per event; collected result replaces close; duplicate board registration is one source');
assert.equal(rows[0].source,'result');assert.equal(rows[1].title,'A question');
assert.equal(panel.rows([project],[result],{[rows[1].id]:'read'})[1].read,true);
assert.equal(panel.rows([project],[result],{[rows[1].id]:'cleared'}).length,1);
assert.equal(project.tasks[0].state,undefined,'presentation does not answer or mutate tasks');
assert.equal(panel.rows([project],[result,{...result,n:6}]).length,3,'one commit SHA can belong to multiple tasks');
const series=(n,at,extra={})=>({title:'Cadence v4 '+n+'/10',at,project:'Project',root:'one',kind:'close',...extra});
assert.equal(panel.groups([series(10,1000),series(9,999),series(8,400)]).length,1,'adjacent members within ten minutes group');
for(const different of [series(9,399),series(10,999),series(9,999,{root:'two'}),series(9,999,{kind:'failed'}),series(9,999,{title:'Other series 9/10'}),series(9,999,{title:'Cadence v4 9/11'})])assert.equal(panel.groups([series(10,1000),different]).length,2,'quiet gap, repeat, project, kind and series identity stay distinct');
assert.equal(panel.groups([series(10,1000),{title:'Unrelated question',at:999},series(9,998)]).length,2,'unrelated activity does not split a series burst');
assert.equal(panel.groups([series(11,1000),series(10,999)]).length,2,'invalid ordinal is never a series');
assert.equal(panel.groups([{title:'Ordinary task',at:1000},{title:'Ordinary task',at:999}]).length,2);
console.log('PASS M14 exact event/result identities, duplicate boards, read/clear and series boundaries');

// #2110 M14: thread counts are independent of raw events; New and Needs me overlap without addition.
const q={...project,tasks:[{n:4,title:'Current',state:'ask'}],events:Array.from({length:105},(_,i)=>[i+10,'blocked','run',4])};
const report={...result,kind:'report',decision:'none'};
let attention=panel.attention([q],[report],{entries:[]});assert.equal(attention.fresh.size,2);assert.equal(attention.needs.size,2);
const read=Object.fromEntries(panel.rows([q],[report]).map(r=>[r.id,'cleared']));attention=panel.attention([q],[{...report,read:true}],{entries:[]},read);assert.equal(attention.fresh.size,0);assert.equal(attention.needs.size,2,'read/clear never answers a question or decides a report');
q.tasks[0].conversation={id:'owner'};attention=panel.attention([q],[{...report,conversation:'owner'}],{entries:[]},{},[{id:'owner'}]);assert.deepEqual([...attention.fresh],['owner:owner']);assert.deepEqual([...attention.needs],['owner:owner'],'linked PM sources have one centre identity');
// #2110 M14/TW5: a locally archived task leaves the entrance's Recent scope, while board ask truth remains.
{
 const p={root:'fixture',issues:'https://example.org/issues',tasks:[{n:7,state:'ask'}],events:[]};
 const a=panel.attention([p],[],{entries:[{kind:'question',board:p.issues,task:7,archived:true,read:false}]});
 assert.equal(a.needs.size,0,'archived question cannot produce an empty Needs me entrance');
 assert.equal(p.tasks[0].state,'ask','local archive never answers the board question');
}

// Chain: #2193 A3. Risk: one task's repeated unread Question, Attempt ended or Blocked events show as many New rows.
{
 const kinds=['ask','gone','blocked'],p={root:'r',name:'R',issues:'https://example.org/i',tasks:[{n:14,title:'T'},{n:75,title:'U'}],events:[]};
 for(let i=0;i<9;i++)p.events.push([100-i,kinds[i%3],'attempt-'+i,i%2?75:14]);
 const rows=panel.rows([p],[]),groups=panel.collapse(rows);
 for(const kind of kinds)for(const n of [14,75]){const g=groups.filter(g=>g.rows[0].kind===kind&&g.rows[0].n===n);assert.ok(g.length<=1,'one unread row per task and kind: '+kind+' #'+n);}
 assert.equal(groups.reduce((a,g)=>a+g.rows.length,0),9,'every event stays inside a counted row');
 const ask=groups.find(g=>g.rows[0].kind==='ask'&&g.rows[0].n===14);assert.equal(ask.rows.length,2,'interleaved and cross-attempt events collapse');
 const marks=Object.fromEntries(ask.rows.map(r=>[r.id,'read']));
 assert.ok(panel.rows([p],[],marks).filter(r=>r.kind==='ask'&&r.n===14).every(r=>r.read),'marking the row read clears every collapsed event');
 assert.equal(panel.collapse(panel.rows([p],[],marks)).filter(g=>!g.rows[0].read).length,groups.length-1);
}
console.log('PASS #2193 unread task events collapse per kind and mark read together');
