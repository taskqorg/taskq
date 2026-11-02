'use strict';
const {h}=require('preact'),{execution}=require('./ui-runtime.js'),time=require('./relative-time.js');
module.exports=execution.module('task-health',{
 warning:function(t){return ['doing','review'].includes(t?.state)?t?.heartbeat?.warning||(t?.heartbeat?null:'Execution heartbeat unknown: source unavailable'):null;},
 details:function(t){
  const b=t?.heartbeat,p=b?.progress;
  const stamp=(seconds,source)=>time.stamp(time.seconds(seconds),source,'No attributable observation');
  return h('section',{class:'task-health','aria-label':'Execution health'},h('strong',null,'Execution health'),h('dl',null,
   h('dt',null,'Host / attempt'),h('dd',null,(b?.host||t?.host||'unknown')+' / '+(b?.attempt??t?.attempt??'unknown')),
   h('dt',null,'Heartbeat source'),h('dd',null,b?.source||'unknown: source unavailable'),
   h('dt',null,'Last observation / age'),h('dd',null,stamp(b?.observed,b?.source||'Host heartbeat')),
   h('dt',null,'Freshness'),h('dd',null,(b?.freshness||'unknown')+' · '+(b?.reason||'Heartbeat unavailable')),
   h('dt',null,'Execution progress'),h('dd',null,(p?.status||'unknown')+' · '+(p?.reason||'Native progress unavailable')),
   h('dt',null,'Progress source'),h('dd',null,(p?.source||'unknown')+(p?.clock?' · '+p.clock:'')),
   h('dt',null,'Progress observation'),h('dd',null,stamp(p?.observed,'Native progress observation')),
   h('dt',null,'Last completed event'),h('dd',null,stamp(p?.last_event,'Native completed event')),
   h('dt',null,'Pending tools'),h('dd',null,p?.pending_tools??'unknown'),
   h('dt',null,'Tool deadline'),h('dd',null,p?.deadline?stamp(p.deadline,'Declared tool deadline'):p?.pending_tools?'No declared deadline; 15m progress window':'not applicable')));
 }
});
