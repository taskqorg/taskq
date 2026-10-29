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
r.open('fixture',12);let op=sent.at(-1);assert.equal(op.action,'open');
r.receive({id:'wrong',root:'fixture',report:{...row,read:true,body:'wrong',tasks:[]}});assert.equal(r.current().body,undefined);
r.receive({id:op.id,root:'fixture',report:{...row,read:true,body:'# Question\nWhat works?',tasks:[{title:'Document limits'}]}});
assert.equal(r.rows()[0].read,true);assert(r.current().body.includes('Question'));
for(const action of ['later','make','drop']){
 r.act(action);op=sent.at(-1);assert.equal(op.action,action);assert.equal(op.sha,row.sha);
 r.receive({id:op.id,root:'fixture',reason:'Board unavailable'});assert.equal(r.rows().length,1,'failed '+action+' retains the report');
}
r.act('later');op=sent.at(-1);r.receive({id:op.id,root:'fixture',report:{...row,read:true,decision:'later',visible:true}});assert.equal(r.rows().length,1);
r.act('make');op=sent.at(-1);r.receive({id:op.id,root:'fixture',report:{...row,decision:'made',visible:false,tasks_made:[20]}});assert.equal(r.rows().length,0);
assert(paints>0);console.log('PASS Reports: source-bound receipts, read, Later, partial/unknown protection, Make tasks and Drop dispatch');
