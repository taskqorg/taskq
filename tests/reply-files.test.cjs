// §5 / #1817: task reply files must survive immutable send/retry, without owner-session limits.
const assert=require('node:assert/strict'),q=require('../src/answer-card.js');
const row={n:7,state:'ask',revision:20,question:{parsed:true,choices:[],text:'Done?'}},cap={answer:true,manage:true,composer:true};
const files=[{id:'photo',name:'photo.png',mime:'image/png',hash:'a'.repeat(64),bytes:32},{id:'document',name:'report.pdf',mime:'application/pdf',hash:'b'.repeat(64),bytes:20*1024*1024}];
let local=q.step(row,q.fresh(row),cap,{type:'files',files},()=> 'reply-1').local;
let sent=q.step(row,local,cap,{type:'submit'},()=> 'reply-1');
assert.deepEqual(sent.send?.files,files,'file-only image + 20 MiB document must reach the operation');
assert.equal(sent.send.text,'');
local=q.step(row,sent.local,cap,{type:'receipt',opId:'reply-1',state:'unknown'},()=>{}).local;
assert.deepEqual(q.step(row,local,cap,{type:'retry'},()=> 'different').send,sent.send,'unknown retry preserves bytes and operation');
assert.equal(q.step({...row,revision:21},local,cap,{type:'submit'},()=> 'different').send,null,'an unknown in-flight attachment reply remains fenced');
const changed=q.step(row,q.fresh(row),cap,{type:'files',files},()=>{}).local;
const note=q.step({...row,revision:21},changed,cap,{type:'submit'},()=> 'note').send;
assert.deepEqual(note.files,files);assert.equal(note.revision,20,'changed question preserves file-only intent as a note at its original revision');
assert.equal(q.step(row,q.fresh(row),cap,{type:'submit'},()=> 'empty').send,null,'empty reply still refused');
for(const file of [{...files[0],pending:true},{...files[0],error:'Disk full; free disk space and select it again'}]){
 const blocked=q.step(row,q.fresh(row),cap,{type:'files',files:[file]},()=>{}).local;
 assert.equal(q.step(row,blocked,cap,{type:'submit'},()=> 'blocked').send,null,'a pending/refused file never sends');
 assert(q.compose(row,blocked,cap).problem.includes(file.error?'Disk full':'copying attachment'));
}
const many=q.step(row,q.fresh(row),cap,{type:'files',files:Array(10).fill(files[0])},()=>{}).local;
assert.equal(q.step(row,many,cap,{type:'submit'},()=> 'many').send.files.length,10,'no file-count cap');
console.log('PASS task reply image/document payload, file-only send, same-operation retry and revision fencing');
