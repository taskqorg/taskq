'use strict';
const ui=require('./ui-runtime.js'),views=require('./task-views.js'),{execution}=ui;
// P-W1: both lists read the exact board object and use the same closed-pin lifetime.
module.exports=execution.module('taskPins',{
 create:function({snapshot,live,send,remove,draw}){
  const reads=new Map();let requests=0;
  const api=execution.module('taskPins.instance',{
   get:function(ref){return reads.get(ref);},
   clear:function(ref){const e=reads.get(ref);ui.cancel(e?.timer);ui.cancel(e?.expiry);reads.delete(ref);},
   sync:function(pins){const refs=new Set(pins.map(p=>p.ref));for(const ref of reads.keys())if(!refs.has(ref))api.clear(ref);},
   unpin:function(ref){const e=reads.get(ref);if(e?.removing)return;if(e){e.removing=true;ui.cancel(e.expiry);}remove(ref);},
   opened:function(ref){const e=reads.get(ref);if(e){e.opened=true;if(e.value?.state==='closed')api.unpin(ref);}},
   read:function(pin,retry=false){
    if(live(pin.ref)){if(pin.closedSeen)send({a:'task-pin',ref:pin.ref,pinned:true,closedSeen:0});delete pin.closedSeen;api.clear(pin.ref);return null;}
    let e=reads.get(pin.ref);const S=snapshot(),now=Date.now();
    if(e?.value?.state==='closed'&&!e.removing){
     if(!(Number.isFinite(e.value.closed)&&e.value.closed>0)&&!pin.closedSeen){e.closedSeen??=Math.floor(S.now||now/1000);pin.closedSeen=e.closedSeen;send({a:'task-pin',ref:pin.ref,pinned:true,closedSeen:pin.closedSeen});}
     pin.closedSeen=Number.isFinite(e.value.closed)&&e.value.closed>0?e.value.closed:pin.closedSeen;
     const remaining=pin.closedSeen+86400-(S.now||now/1000);
     if(remaining<=0)api.unpin(pin.ref);
     else if(!e.expiry)e.expiry=ui.defer(()=>{const current=reads.get(pin.ref);if(current)current.expiry=null;if(!live(pin.ref)&&current?.value?.state==='closed'){api.unpin(pin.ref);draw();}},remaining*1000);
    }
    if(e?.loading||e?.removing||e&&!retry&&now-e.at<60000)return e;
    const cut=pin.ref.lastIndexOf('#'),n=Number(pin.ref.slice(cut+1)),p=S.projects.find(p=>views.identity(p)===pin.ref.slice(0,cut));
    if(!p||!Number.isSafeInteger(n)||n<=0)return {reason:'Registered board unavailable'};
    e={...e,ref:pin.ref,root:p.root,task:n,kind:p.board_kind,project:p.name,id:'pin-'+(++requests),at:now,loading:true,reason:null};reads.set(pin.ref,e);
    e.timer=ui.defer(()=>api.receive({root:e.root,id:e.id,reason:'No board response within 65 seconds'}),65000);
    send({a:'completed',root:e.root,id:e.id,task:n});return e;
   },
   receive:function(page){const e=[...reads.values()].find(e=>e.id===page.id&&e.root===page.root);if(!e?.loading)return false;ui.cancel(e.timer);e.loading=false;e.reason=page.reason;e.value=!page.reason&&page.tasks?.[0]?.n===e.task?page.tasks[0]:null;if(!e.value&&!e.reason)e.reason='Board response omitted the task';if(e.opened&&e.value?.state==='closed')api.unpin(e.ref);draw();return true;}
  });return api;
 }
});
