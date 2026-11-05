// #2366 A2: source-emitted event/owner keys must survive the same validator as other source keys.
const assert=require('node:assert/strict'),anchor=require('../src/reading-anchor.js');
for(const [kind,parts]of [['event',[7,83]],['owner-request',['invented-uuid']],['question',[7,71]],['answer',[7,83]],['report',['task-abc-7-71-brief-0',2]]]){
 const key=anchor.key(kind,...parts);assert(anchor.valid(key),key);assert(anchor.position({key,y:-3.5}));
}
for(const key of ['event:7:foreign','owner-request:../escape','report:<script>','task:7\n','unknown:7'])assert(!anchor.position({key,y:0}),key);
assert(anchor.position({at:100,i:0,part:32768,y:1}));assert(!anchor.position({at:-1,i:0,y:1}));
console.log('PASS shared source-anchor grammar');
