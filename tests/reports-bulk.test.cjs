// Chain: M14. Risk: Mark all read overflows the native 16-command queue or marks a later arrival read.
const assert=require('node:assert/strict'),{create}=require('../src/reports.js');
let next=0;const sent=[],timers=[];
const reports=create({send:m=>sent.push(m),draw(){},defer:f=>timers.push(f),id:()=>String(++next)});
const rows=Array.from({length:30},(_,i)=>({n:i+1,sha:String(i).padStart(40,'a'),read:false,visible:true,kind:'task'}));
const project={root:'fixture',issues:'https://example.org/issues',reports:{complete:true,reports:rows}};
reports.sync([project]);reports.markRead(reports.rows());reports.markRead(reports.rows());
assert.equal(sent.length,1,'only one read is submitted, including repeated clicks');
const newcomer={n:99,sha:'b'.repeat(40),read:false,visible:true,kind:'task'};
project.reports.reports=[...rows,newcomer];reports.sync([project]);
for(let i=0;i<30;i++){
 const m=sent[i];assert(m,'next read continues after its prior receipt');
 reports.receive({...m,report:{...rows.find(r=>r.n===m.n),read:true}});
 assert.equal(sent.length,Math.min(i+2,30),'receipt-driven serialization');
}
assert.equal(reports.count(),1,'new arrival was outside the bulk snapshot');
assert.equal(reports.unread()[0].n,99);
reports.markRead(reports.unread());reports.receive({...sent.at(-1),reason:'Board unavailable'});
assert.equal(reports.count(),1);assert.equal(reports.readFailure(),'Board unavailable','failed read stays explicit');
reports.markRead(reports.unread());timers.at(-1)();assert.match(reports.readFailure(),/unknown/,'lost receipt is not a successful read');
console.log('PASS M14 serial bulk read, repeated clicks, exact snapshot, failed and unknown receipts');
