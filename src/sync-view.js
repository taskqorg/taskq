'use strict';
const {execution}=require('./ui-runtime.js');
const {h}=require('preact');
// #1819/#1918: the tray applies its shared pending operations to the one snapshot before any window reads it
// (docs/design/menu.md#shared-pending-operations-1819). A window keeps no overlay: a row shows only the board's state
// or its op-id readback, plus the operation's line while it is sending, failed or not applied.
const PHASES={sending:'Sending',failed:'Failed',mismatch:'Not applied'};
const api=execution.module('sync-view',{
clock:function(ms){const d=new Date(ms);return String(d.getHours()).padStart(2,'0')+':'+String(d.getMinutes()).padStart(2,'0');},
// One line for a row's operation: phase, then its reason.
line:function(o){return o?(PHASES[o.phase]||o.phase)+(o.reason?': '+o.reason:''):'';},
// 'Data from HH:MM' while a board read is older than two polls or missing; null while every read is fresh.
// The read ages by status.json `age` plus its `tasks_age_seconds`; an idle service waits 3 polls, at most 15 s while a
// window is open (taskq.md § 4 step 8).
stale:function(S){
  let oldest=null,missing=false;
  for(const p of S?.projects||[]){
    const age=p.age+p.tasks_age_seconds,interval=Math.max(p.poll,Math.min(3*p.poll,15));
    if(p.alive!==true||!Number.isFinite(age)){missing=true;continue;}
    if(Number.isFinite(interval)&&age>2*interval){const at=(S.now-age)*1000;oldest=oldest===null?at:Math.min(oldest,at);}
  }
  return oldest!==null?'Data from '+api.clock(oldest):missing?'Data from: unknown':null;
},
// A row's operation as shown in every window; only a failed one offers Retry, which resends the same operation.
row:function(o){return o?h('p',{class:'meta sync '+o.phase,role:'status'},api.line(o),o.phase==='failed'&&h('button',{class:'compact-action','data-a':'sync-retry','data-root':o.root,'data-id':o.id},'Retry')):null;},
// The window's one stale-data line, or nothing while every read is fresh.
staleRow:function(S){const t=api.stale(S);return t?h('p',{class:'meta stale',role:'status'},t):null;}
});
module.exports={phases:PHASES,line:api.line,stale:api.stale,row:api.row,staleRow:api.staleRow};
