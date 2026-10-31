// Chain: §7 -> shared Reports UI -> board action receipts. Risk: opening claims read without board proof,
// Later loses a report, or Make tasks/Drop silently hide it on a failed or mismatched receipt.
const assert=require('node:assert/strict');
const {create}=require('../src/reports.js');
const sent=[],timers=[];let paints=0;
const r=create({send:m=>sent.push(m),draw:()=>paints++,defer:f=>timers.push(f),id:()=>String(sent.length+1)});
const projects=[{root:'fixture',name:'Alpha',issues:'https://github.com/o/r/issues'}];
const row={n:12,title:'Retry research',sha:'a'.repeat(40),closed:40,read:false};
projects[0].reports={complete:true,observed:100,reports:[row]};
r.sync(projects);assert.equal(sent.length,0,'list uses pushed collection');
assert.equal(r.rows()[0].read,false);
projects[0].reports.reports=[{...row,body:'# Question\nWhat works?',tasks:[{title:'Document limits'}]}];r.sync(projects);
r.open('fixture',12);let op=sent.at(-1);assert.equal(op.action,'read');
r.receive({id:'wrong',root:'fixture',report:{...row,read:true,body:'wrong',tasks:[]}});assert(r.current().body.includes('Question'));
r.receive({id:op.id,root:'fixture',report:{...row,read:true}});
assert.equal(r.rows()[0].read,true);assert(r.current().body.includes('Question'));
for(const action of ['later','make','drop']){
 r.act(action);op=sent.at(-1);assert.equal(op.action,action);assert.equal(op.sha,row.sha);
 r.receive({id:op.id,root:'fixture',reason:'Board unavailable'});assert.equal(r.rows().length,1,'failed '+action+' retains the report');
 // A later exact readback settles the unknown operation before a different decision is allowed.
 r.receive({id:op.id,root:'fixture',report:{...row,read:true}});
}
// A create POST can return an issue number before its held-state readback fails. Retain both receipts,
// and distinguish that unverified issue from the prior confirmed held draft.
r.act('make');op=sent.at(-1);
r.receive({id:op.id,root:'fixture',reason:'Draft readback unknown',receipts:[20],receipt:{task:21,outcome:'unknown'}});
const partial=JSON.stringify(r.section(projects));
assert(partial.includes('Created held drafts: #20'),'confirmed drafts remain named');
assert(partial.includes('Unverified draft: #21'),'known issue survives an unknown held-state readback');
assert.equal(r.rows().length,1,'partial creation keeps the report available');
r.receive({id:op.id,root:'fixture',report:{...row,read:true}});
const until=new Date(Date.now()+86400000).toISOString().replace(/\.\d{3}Z$/,'Z');
r.act('later',undefined,until);op=sent.at(-1);assert.equal(op.until,until,'Later carries its revisit date');
r.receive({id:op.id,root:'fixture',report:{...row,read:true,decision:'later',visible:true,later_until:until}});assert.equal(r.rows().length,1);
assert.deepEqual(r.decide(),[],'a future Later leaves Decide');assert(JSON.stringify(r.section(projects)).includes('Later: back in Decide on '+until.slice(0,10)));
r.act('make');op=sent.at(-1);r.receive({id:op.id,root:'fixture',report:{...row,decision:'made',visible:false,tasks_made:[20]}});assert.equal(r.rows().length,0);
assert(paints>0);console.log('PASS Reports: source-bound receipts, read, Later, partial/unknown protection, Make tasks and Drop dispatch');
const aliases=[],a=create({send:m=>aliases.push(m),draw(){},defer(){},id:()=>String(aliases.length+1)});
const duplicate=[...projects,{...projects[0],root:'same-board'}];
projects[0].reports.reports=[{...row,body:'Report',tasks:[]},{...row,n:13}];
a.sync(duplicate);assert.equal(aliases.length,0,'no Reports read per registration');assert.equal(a.rows().length,2);
a.open(projects[0].root,12);let aliasOp=aliases.at(-1);a.receive({id:aliasOp.id,root:aliasOp.root,report:{...row,read:true}});
a.act('drop');aliasOp=aliases.at(-1);a.receive({id:aliasOp.id,root:aliasOp.root,report:{...row,decision:'drop',visible:false}});
assert.deepEqual(a.rows().map(r=>r.n),[13],'Drop cannot leave the same report under another registration');
// Chain: #1979 R6 Decide -> app rows -> existing report card. Risk: an undecided report hides among read results,
// a past Later never returns, or a decided report keeps its Decide row.
const d=create({send(){},draw(){},defer(){},id:()=>'1'}),now=Math.floor(Date.now()/1000),iso=t=>new Date(t*1000).toISOString().replace(/\.\d{3}Z$/,'Z');
const decideProject=[{root:'decide',name:'Decide',issues:'https://github.com/o/d/issues',reports:{complete:true,reports:[
 {n:1,kind:'report',sha:'a',title:'New',closed:now-100,read:false,decision:'none',visible:true,experiment_verdict:'refuted'},
 {n:2,kind:'report',sha:'a',title:'Old',closed:now-3*86400,read:true,decision:'none',visible:true},
 {n:3,kind:'report',sha:'a',title:'Due',closed:now-200,read:true,decision:'later',later_until:iso(now-60),visible:true},
 {n:4,kind:'report',sha:'a',title:'Waiting',closed:now-300,read:true,decision:'later',later_until:iso(now+3600),visible:true},
 {n:5,kind:'task',sha:'a',title:'Task',closed:now,read:false,decision:'none',visible:true},
 {n:6,kind:'report',sha:'a',title:'Making',closed:now-400,read:true,decision:'none',pending_decision:'make',visible:true}]}}];
d.sync(decideProject);assert.deepEqual(d.decide().map(r=>r.n),[2,3,1],'undecided and due reports, oldest close first');
// Product RC1 moves the old Results list into Conversations; retain verdict/age in its shared reader.
d.open('decide',2);assert(JSON.stringify(d.section(decideProject)).includes('Verdict: unknown · 3d'));
d.open('decide',1);assert(JSON.stringify(d.section(decideProject)).includes('Verdict: refuted · 0d'));
assert.equal(d.threads().find(t=>t.report===2).marker,'Needs decision','a read but undecided report stays marked');
console.log('PASS Reports: Decide rows, verdict and age, Later revisit');
const unread=[{root:'new-project',name:'New',issues:'https://github.com/o/new/issues'}];
assert(!JSON.stringify(a.section(unread)).includes('No results waiting for review.'),'an uncollected project is unknown, not empty');
assert.equal(a.rows().length,0,'removed registrations cannot leave actionable cached reports');
const failures=[],u=create({send:m=>failures.push(m),draw(){},defer(){},id:()=>String(failures.length+1)});
const empty=[{...projects[0],reports:{complete:true,observed:100,reports:[]}}];
u.sync(empty);assert(JSON.stringify(u.section(empty)).includes('No results waiting for review.'));
const unavailable=[{...empty[0],reports:{complete:false,reason:'Board unavailable',reports:[]}}];
assert(!JSON.stringify(u.section(unavailable)).includes('No results waiting for review.'),'a failed collection cannot report a healthy empty board');
assert(JSON.stringify(u.section(unavailable)).includes('Board unavailable'));
