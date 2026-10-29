// Chain: §7 -> shared Reports UI -> board action receipts. Risk: opening claims read without board proof,
// Later loses a report, or Make tasks/Drop silently hide it on a failed or mismatched receipt.
const assert=require('node:assert/strict');
const {create}=require('../src/reports.js');
const sent=[],timers=[];let paints=0;
const r=create({send:m=>sent.push(m),draw:()=>paints++,defer:f=>timers.push(f),id:()=>String(sent.length+1)});
const projects=[{root:'fixture',name:'Alpha',issues:'https://github.com/o/r/issues'}];
r.toggle(projects);assert.equal(sent[0].a,'reports');assert.equal(sent[0].action,'list');
const row={n:12,title:'Retry research',sha:'a'.repeat(40),closed:40,read:false};
r.receive({id:sent[0].id,root:'fixture',reports:[row],more:false});
assert.equal(r.rows()[0].read,false);
r.refresh();let refresh=sent.at(-1);r.open('fixture',12);assert.equal(r.current(),null,'pending refresh must not select an unloaded card');
r.receive({id:refresh.id,root:'fixture',reports:[row],more:true,next:'40:12'});
r.click({dataset:{report:'more',root:'fixture'}},projects);let more=sent.at(-1);assert.equal(more.before,'40:12','More carries the stable close-time boundary');
r.receive({id:more.id,root:'fixture',reports:[],more:false,next:null});
r.open('fixture',12);let op=sent.at(-1);assert.equal(op.action,'open');
r.receive({id:'wrong',root:'fixture',report:{...row,read:true,body:'wrong',tasks:[]}});assert.equal(r.current().body,undefined);
r.receive({id:op.id,root:'fixture',report:{...row,read:true,body:'# Question\nWhat works?',tasks:[{title:'Document limits'}]}});
assert.equal(r.rows()[0].read,true);assert(r.current().body.includes('Question'));
for(const action of ['later','make','drop']){
 r.act(action);op=sent.at(-1);assert.equal(op.action,action);assert.equal(op.sha,row.sha);
 r.receive({id:op.id,root:'fixture',reason:'Board unavailable'});assert.equal(r.rows().length,1,'failed '+action+' retains the report');
}
// A create POST can return an issue number before its held-state readback fails. Retain both receipts,
// and distinguish that unverified issue from the prior confirmed held draft.
r.act('make');op=sent.at(-1);
r.receive({id:op.id,root:'fixture',reason:'Draft readback unknown',receipts:[20],receipt:{task:21,outcome:'unknown'}});
const partial=JSON.stringify(r.section(projects));
assert(partial.includes('Created held drafts: #20'),'confirmed drafts remain named');
assert(partial.includes('Unverified draft: #21'),'known issue survives an unknown held-state readback');
assert.equal(r.rows().length,1,'partial creation keeps the report available');
r.act('later');op=sent.at(-1);r.receive({id:op.id,root:'fixture',report:{...row,read:true,decision:'later',visible:true}});assert.equal(r.rows().length,1);
r.act('make');op=sent.at(-1);r.receive({id:op.id,root:'fixture',report:{...row,decision:'made',visible:false,tasks_made:[20]}});assert.equal(r.rows().length,0);
assert(paints>0);console.log('PASS Reports: source-bound receipts, read, Later, partial/unknown protection, Make tasks and Drop dispatch');
const aliases=[],a=create({send:m=>aliases.push(m),draw(){},defer(){},id:()=>String(aliases.length+1)});
a.toggle([...projects,{...projects[0],root:'same-board'}]);assert.equal(aliases.length,1,'one Reports read per board, not per registration');
a.receive({id:aliases[0].id,root:aliases[0].root,reports:[row,{...row,n:13}],more:false});assert.equal(a.rows().length,2);
a.open(projects[0].root,12);let aliasOp=aliases.at(-1);a.receive({id:aliasOp.id,root:aliasOp.root,report:{...row,body:'Report',tasks:[]}});
a.act('drop');aliasOp=aliases.at(-1);a.receive({id:aliasOp.id,root:aliasOp.root,report:{...row,decision:'drop',visible:false}});
assert.deepEqual(a.rows().map(r=>r.n),[13],'Drop cannot leave the same report under another registration');
const unread=[{root:'new-project',name:'New',issues:'https://github.com/o/new/issues'}];
assert(!JSON.stringify(a.section(unread)).includes('No reports waiting for a decision.'),'an uncollected project is unknown, not empty');
assert.equal(a.rows().length,0,'removed registrations cannot leave actionable cached reports');
const failures=[],u=create({send:m=>failures.push(m),draw(){},defer(){},id:()=>String(failures.length+1)});
u.toggle(projects);u.receive({id:failures.at(-1).id,root:projects[0].root,reports:[],more:false});
u.refresh();u.receive({id:failures.at(-1).id,root:projects[0].root,reason:'Board unavailable'});
assert(!JSON.stringify(u.section(projects)).includes('No reports waiting for a decision.'),'a failed refresh cannot report a healthy empty board');
