// #1940 A3/A5/R12: opening Reports must use the pushed service projection, including unknown states.
// Risk: navigation triggers a slow board traversal or silently renders an uncollected board empty.
const assert=require('node:assert/strict'),{create}=require('../src/reports.js');
const timings=[];global.ipc={postMessage:text=>{const message=JSON.parse(text);if(message.a==='web-observation')timings.push(message.row);}};
require('../src/ui-runtime.js').connect();
const sent=[],r=create({send:m=>sent.push(m),draw(){},defer(){},id:()=>String(sent.length+1)});
const report={n:12,title:'Retry research',sha:'a'.repeat(40),closed:40,read:false,visible:true,body:'# Question\n\nWhat works?\n\n# Recommendation\n\nReuse the request.',tasks:[{title:'Document limits'}]};
const projects=[{root:'fixture',name:'Alpha',issues:'https://github.com/o/r/issues',reports:{complete:true,observed:100,reports:[report]}}];
r.toggle(projects);
assert.equal(sent.length,0,'opening Reports must not request a board list');
r.open('fixture',12);
assert.equal(r.current().body,report.body,'report content is available in the opening frame');
assert.equal(sent.at(-1).action,'read','only a separate acknowledgement may reach the board');
assert.equal(r.current().read,false,'display is not proof of a committed read marker');
assert(JSON.stringify(r.section(projects)).includes('Recommendation'));
const read=sent.at(-1);
r.receive({id:read.id,root:'fixture',report:{...report,closed:0,read:true}});
r.section(projects);
assert.equal(r.rows()[0].closed,40,'a read receipt must not replace the authoritative close time');
r.act('make');const make=sent.at(-1);
r.receive({id:make.id,root:'fixture',reason:'Final marker readback unknown',report:{...report,closed:0,read:true,pending_decision:'make'}});
assert(JSON.stringify(r.section(projects)).includes('Retry Make tasks'),'partial creation must keep its reservation through pushed stale data');
// A late acknowledgement from the old SHA must never combine its identity with the new report's content.
const b=create({send:m=>sent.push(m),draw(){},defer(){},id:()=>String(sent.length+1)});
b.toggle(projects);b.open('fixture',12);const old=sent.at(-1);
const next=[{...projects[0],reports:{complete:true,reports:[{...report,sha:'b'.repeat(40),body:'New SHA body',comments_through:20}]}}];
b.sync(next);b.open('fixture',12);const newer=sent.at(-1);
b.receive({id:old.id,root:'fixture',report:{...report,read:true,comments_through:10}});
assert.equal(b.current().sha,'b'.repeat(40));assert.equal(b.current().body,'New SHA body');assert.equal(b.current().read,false);
b.receive({id:newer.id,root:'fixture',report:{...next[0].reports.reports[0],read:true,comments_through:21}});
b.section(next);assert.equal(b.current().read,true,'lagging collection cannot erase a committed acknowledgement');
next[0].reports.reports[0]={...next[0].reports.reports[0],read:true,pending_decision:'make',comments_through:22};
b.section(next);assert.equal(b.current().pending_decision,'make','newer collected markers supersede local read receipts');
for(const operation of ['js.reports.list.open','js.reports.card.open'])assert(timings.some(r=>r.operation===operation&&typeof r.duration_us==='number'),operation+' records load time without opt-in detail');
console.log('PASS cached report list/open and separate read acknowledgement');

// Owner expansion: both result kinds share local navigation; seeing never implies accepting or starting work.
const out=[],v=create({send:m=>out.push(m),draw(){},defer(){},id:()=>String(out.length+1)});
const ordinary={...report,n:13,kind:'task',body:'Changed: Requests recover.\nChecks: retry -- pass\nChecks: identity -- pass\nOpen: none',verdict:'Checks: A1 ok',pr:'https://github.com/invented/research/pull/20'};
const all=[{...projects[0],reports:{complete:true,reports:[ordinary,report]}}];
v.toggle(all);assert.equal(v.count(),2);v.open('fixture',13);
assert.equal(out.at(-1).action,'read');const seen=out.at(-1);
const content=JSON.stringify(v.section(all));assert(content.includes('identity -- pass'));assert(content.includes('Reopen'));assert(content.includes('Follow-up task'));assert(!content.includes('Make tasks'));
v.act('accept');const accept=out.at(-1);v.receive({id:accept.id,root:'fixture',report:{...ordinary,read:true,decision:'accepted',comments_through:30}});
v.receive({id:seen.id,root:'fixture',report:{...ordinary,read:true,decision:'none',comments_through:29}});assert.equal(v.current().decision,'accepted','late read cannot erase acceptance');
assert.equal(v.count(),1);v.move(1);assert.equal(v.current().n,12);v.move(-1);assert.equal(v.current().n,13);
assert(!out.some(m=>['list','open'].includes(m.action)),'next/previous are local');
v.act('reopen','Preserve the receipt');const reopen=out.at(-1);assert.equal(reopen.text,'Preserve the receipt');
v.receive({id:reopen.id,root:'fixture',report:{...ordinary,read:true,decision:'accepted',comments_through:30},successor:25});
assert(JSON.stringify(v.section(all)).includes('Read held draft #25'));
console.log('PASS Results: local mixed navigation, all checks, seen versus accept, held-successor readback');
const late=[],w=create({send:m=>late.push(m),draw(){},defer(){},id:()=>String(late.length+1)});
const pair=[{...projects[0],reports:{complete:true,reports:[ordinary,{...ordinary,n:14}]}}];
w.toggle(pair);w.open('fixture',13);w.act('reopen','A reason');const aOp=late.at(-1);
w.open('fixture',14);w.click({dataset:{report:'followup'}},pair);
w.receive({id:aOp.id,root:'fixture',report:{...ordinary,read:true},successor:26});
assert.equal(w.current().n,14);const bView=JSON.stringify(w.section(pair));assert(bView.includes('Follow-up reason'));assert(!bView.includes('Read held draft #26'));
w.open('fixture',13);assert(JSON.stringify(w.section(pair)).includes('Read held draft #26'),'a late receipt stays with its source for readback');
console.log('PASS late successor receipt preserves another result and its draft');
