'use strict';
const {execution}=require('./ui-runtime.js');
// #1819: the tray's shared pending operations over a status snapshot (docs/design/menu.md#shared-pending-operations-1819).
// The board stays the truth: an overlaid row is marked unconfirmed, and counts stay the snapshot's.
const PHASES={sending:'Sending',waiting:'Waiting GitHub',done:'Done',failed:'Failed',mismatch:'Not applied'};
const UNCONFIRMED=['sending','waiting'];
const api=execution.module('sync-view',{
name:function(S,state){return (S.display?.states||[]).find(x=>x.id===state)?.name||(state?state.charAt(0).toUpperCase()+state.slice(1):'Unknown');},
clock:function(ms){const d=new Date(ms);return String(d.getHours()).padStart(2,'0')+':'+String(d.getMinutes()).padStart(2,'0');},
// S with each operation's expected (or read-back) state on its row; a failed one keeps the board's state and its Retry.
overlay:function(S,ops){
  if(!S||!Array.isArray(S.projects)||!Array.isArray(ops)||!ops.length)return S;
  return {...S,projects:S.projects.map(p=>{
    const mine=ops.filter(o=>o.root===p.root);
    if(!mine.length||!Array.isArray(p.tasks))return p;
    return {...p,tasks:p.tasks.map(t=>{
      const o=mine.filter(o=>o.task===t.n).at(-1);
      if(!o)return t;
      if(o.phase==='failed'||!o.expected)return {...t,sync:o};
      return {...t,state:o.expected,state_name:api.name(S,o.expected)+(UNCONFIRMED.includes(o.phase)?' ⟳':''),sync:o};
    })};
  })};
},
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
}
});
module.exports={phases:PHASES,overlay:api.overlay,line:api.line,stale:api.stale};
