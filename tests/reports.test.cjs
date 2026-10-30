// Chain: §7 -> shared Reports UI -> board action receipts. Risk: opening claims read without board proof,
// Later loses a report, or Make tasks/Drop silently hide it on a failed or mismatched receipt.
const assert=require('node:assert/strict');
const {create}=require('../src/reports.js');
const sent=[],timers=[];let paints=0;
const r=create({send:m=>sent.push(m),draw:()=>paints++,defer:f=>timers.push(f),id:()=>String(sent.length+1)});
const projects=[{root:'fixture',name:'Alpha',issues:'https://github.com/o/r/issues'}];
const row={n:12,title:'Retry research',sha:'a'.repeat(40),closed:40,read:false};
projects[0].reports={complete:true,observed:100,reports:[row]};
r.toggle(projects);assert.equal(sent.length,0,'list uses pushed collection');
assert.equal(r.rows()[0].read,false);
projects[0].reports.reports=[{...row,body:'# Question\nWhat works?',tasks:[{title:'Document limits'}]}];r.sync(projects);
r.open('fixture',12);let op=sent.at(-1);assert.equal(op.action,'read');
r.receive({id:'wrong',root:'fixture',report:{...row,read:true,body:'wrong',tasks:[]}});assert(r.current().body.includes('Question'));
r.receive({id:op.id,root:'fixture',report:{...row,read:true}});
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
const duplicate=[...projects,{...projects[0],root:'same-board'}];
projects[0].reports.reports=[{...row,body:'Report',tasks:[]},{...row,n:13}];
a.toggle(duplicate);assert.equal(aliases.length,0,'no Reports read per registration');assert.equal(a.rows().length,2);
a.open(projects[0].root,12);let aliasOp=aliases.at(-1);a.receive({id:aliasOp.id,root:aliasOp.root,report:{...row,read:true}});
a.act('drop');aliasOp=aliases.at(-1);a.receive({id:aliasOp.id,root:aliasOp.root,report:{...row,decision:'drop',visible:false}});
assert.deepEqual(a.rows().map(r=>r.n),[13],'Drop cannot leave the same report under another registration');
const unread=[{root:'new-project',name:'New',issues:'https://github.com/o/new/issues'}];
assert(!JSON.stringify(a.section(unread)).includes('No results waiting for review.'),'an uncollected project is unknown, not empty');
assert.equal(a.rows().length,0,'removed registrations cannot leave actionable cached reports');
const failures=[],u=create({send:m=>failures.push(m),draw(){},defer(){},id:()=>String(failures.length+1)});
const empty=[{...projects[0],reports:{complete:true,observed:100,reports:[]}}];
u.toggle(empty);assert(JSON.stringify(u.section(empty)).includes('No results waiting for review.'));
const unavailable=[{...empty[0],reports:{complete:false,reason:'Board unavailable',reports:[]}}];
assert(!JSON.stringify(u.section(unavailable)).includes('No results waiting for review.'),'a failed collection cannot report a healthy empty board');
assert(JSON.stringify(u.section(unavailable)).includes('Board unavailable'));
