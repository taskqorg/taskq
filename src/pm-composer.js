'use strict';
const {h,render}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
const api=execution.module('pm-composer',{
 request:function(context,text,op,now){
  if(!text.trim()||new TextEncoder().encode(text).length>65536)throw Error('Write 1–65536 UTF-8 bytes');
  const target=Object.fromEntries(['installation_id','generation','project_id','runtime_id'].map(k=>[k,context.target?.[k]]));
  const request={schema:'taskq.pm/1',actor_id:context.actor_id,op_id:op,grant_id:context.grant_id,revision:context.revision,target,issued_at:now,expires_at:now+300,text};
  for(const id of [request.actor_id,op,request.grant_id,request.revision,context.target?.installation_id,context.target?.project_id,context.target?.runtime_id])if(!/^[A-Za-z0-9_-]{1,64}$/.test(id||''))throw Error('PM target or authority unavailable');
  if(!Number.isSafeInteger(context.target.generation)||context.target.generation<1||!Number.isSafeInteger(now)||now<0)throw Error('PM identity or clock unavailable');
  return JSON.parse(JSON.stringify(request));
 },
 receipt:function(value,request){
  if(!value||value.schema!=='taskq.pm/1'||!['accepted','rejected','stale','unknown'].includes(value.outcome)||['actor_id','op_id','revision'].some(k=>value[k]!==request[k]))return {outcome:'unknown',reason:'No matching authoritative receipt'};
  if(value.outcome==='accepted'&&(!/^[a-f0-9]{64}$/.test(value.request_id||'')||!value.native?.host||!value.native?.runtime))return {outcome:'unknown',reason:'Native request identity unavailable'};
  return JSON.parse(JSON.stringify(value));
 },
 handoff:function(native,local=false){
  if(!native)return null;
  const url=native.url,safe=typeof url==='string'&&(/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(url)||(local&&/^codex:\/\/threads\/[A-Za-z0-9-]+$/.test(url)));
  return h('div',{class:'pm-handoff'},h('p',{role:'status'},(native.kind||native.state||'unknown')+': '+(native.reason||native.next||(native.state==='launched'&&native.session?'Continue in the native session':'Native outcome unknown'))),h('p',{class:'meta'},(native.runtime||'Runtime unknown')+' · '+(native.host||'Host unknown')+(native.session?' · '+native.session:'')),safe&&h('a',{href:url,'data-url':url,class:'native-session'},'Open session'));
 },
 composer:function({text,disabled=false,id='pm-draft',field='text',label='Request draft',reason='',placeholder='Ask AI…',before,files,tools,after,onInput,onPaste,onDrop,onDragOver}){
  return h('div',{class:'conversation-composer',onPaste,onDrop,onDragOver},before,h('label',{for:id,class:'sr-only'},label),h('textarea',{id,'data-c-field':field,rows:3,value:text,disabled,'aria-describedby':id==='conversation-draft'?'conversation-reason':id+'-reason',placeholder,onInput}),files,tools,h('p',{id:id==='conversation-draft'?'conversation-reason':id+'-reason',class:'meta'},reason),after);
 },
 mount:function(el,adapter){
  let draft=adapter.load()||{text:''},pending=false,timer,serial=0,invalid=false;
  if(draft.request){try{const r=draft.request,expected=api.request(r,r.text,r.op_id,r.issued_at);if(r.schema!==expected.schema||r.expires_at!==expected.expires_at)throw Error('Invalid saved request');draft.request=expected;if(draft.receipt)draft.receipt=api.receipt(draft.receipt,expected);}catch(_){invalid=true;draft.receipt={outcome:'unknown',reason:'Saved request invalid; preserve it for host reconciliation'};}}
  else if(draft.receipt?.native)draft.receipt={outcome:'unknown',reason:'No bound request for this receipt'};
  const local=execution.module('pm-composer.instance',{
   paint:function(){
    render(h('section',{class:'pm-composer','aria-label':'PM request'},api.composer({text:draft.text,disabled:!!draft.request,reason:pending?'Checking delivery…':draft.receipt?.reason||adapter.context().reason||'Replies stay in the native session.',onInput:e=>{draft.text=e.currentTarget.value;adapter.save(draft);local.paint();},tools:h('div',{class:'conversation-tools'},h('button',{disabled:pending||invalid||!!draft.request||!draft.text.trim()||!!adapter.context().reason,onClick:()=>local.send(false)},'Send'),draft.request&&h('button',{disabled:pending||invalid||!adapter.reconcile,onClick:()=>local.send(true)},'Check delivery'))}),draft.receipt&&h('p',{role:'status'},draft.receipt.outcome),api.handoff(draft.receipt?.native)),el);
   },
   send:function(reconcile){
    if(invalid||pending||reconcile&&!draft.request||!reconcile&&draft.request)return;
    if(!reconcile){try{const next={...draft,request:api.request(adapter.context(),draft.text,adapter.id(),adapter.now())};adapter.save(next);draft=next;}catch(_){draft.receipt={outcome:'unknown',reason:'Request could not be persisted; draft kept'};local.paint();return;}}
    pending=true;const turn=++serial;local.paint();ui.cancel(timer);
    timer=ui.defer(()=>{if(turn!==serial)return;pending=false;draft.receipt={outcome:'unknown',reason:'Delivery unknown. Check the same request ID.'};adapter.save(draft);local.paint();},30000);
    ui.effect('pm.dispatch',()=> (reconcile?adapter.reconcile:adapter.send)(JSON.parse(JSON.stringify(draft.request))),value=>{if(turn!==serial)return;ui.cancel(timer);draft.receipt=api.receipt(value,draft.request);pending=false;adapter.save(draft);local.paint();});
   }
  });local.paint();return local;
 }
});
module.exports=api;
