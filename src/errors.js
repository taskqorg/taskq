'use strict';
const {h}=require('preact'),{execution}=require('./ui-runtime.js'),icons=require('./ui-icons.js');
module.exports=execution.module('errors',{
 retain:function(S,previous){
  if(!previous)return S;
  return {...S,projects:S.projects.map(p=>{
   const prior=previous.projects?.find(q=>q.root===p.root&&(!p.issues||q.issues===p.issues));
   if(!prior||p.complete===true&&p.alive!==false)return p;
   const retained={...p};
   if(!p.issues){retained.issues=prior.issues;retained.board_kind=prior.board_kind;}
   for(const field of ['tasks','hosts']){const incoming=Array.isArray(p[field])?p[field]:[],identity=r=>field==='tasks'?r.n:r.name,seen=new Set(incoming.map(identity));retained[field]=[...incoming,...(prior[field]||[]).filter(r=>!seen.has(identity(r)))];}
   if(!Number.isFinite(p.observed)&&Number.isFinite(prior.observed)){retained.observed=prior.observed;if(!Number.isFinite(p.tasks_age_seconds))retained.tasks_age_seconds=(prior.tasks_age_seconds||0)+Math.max(0,S.now-previous.now);}
   return retained;
  })};
 },
 age:function(p){const seconds=Number.isFinite(p?.age)&&p.age>=0&&Number.isFinite(p?.tasks_age_seconds)&&p.tasks_age_seconds>=0?p.age+p.tasks_age_seconds:null;return seconds===null?'age unknown':seconds<60?Math.floor(seconds)+'s old':seconds<3600?Math.floor(seconds/60)+'m old':Math.floor(seconds/3600)+'h old';},
 cause:function(text,p){
  const limit=/rate limit: board requests paused(?: until ([\dTZ:.-]+))?/i.exec(text);
  if(limit){let server=p?.root||text;try{server=new URL(p?.issues).origin;}catch{}const provider=p?.board_kind==='github'?'GitHub':p?.board_kind==='gitlab'?'GitLab':'Board',until=limit[1],date=until&&new Date(until);
   return {key:'limit:'+provider+':'+server+':'+(until||'unknown'),text:provider+' limit until '+(date&&!Number.isNaN(+date)?date.toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'}):'unknown')};}
  return {key:text,text};
 },
 rows:function(S,extra=[]){
  const rows=new Map();
  const add=(text,p)=>{if(text&&typeof text==='object')text=typeof text.reason==='string'?text.reason:typeof text.message==='string'?text.message:'Status details unavailable; refresh or inspect the source.';if(typeof text!=='string'||!text.trim())return;const cause=module.exports.cause(text,p);if(!rows.has(cause.key))rows.set(cause.key,{...cause,projects:new Map(),details:new Set()});const row=rows.get(cause.key);if(p)row.projects.set(p.root,{name:p.name,age:module.exports.age(p)});row.details.add(text);};
  for(const p of S?.projects||[]){
   add(p.error,p);
   if(p.requests?.paused_until)add('rate limit: board requests paused until '+p.requests.paused_until,p);
   else if(Number.isFinite(p.requests?.remaining)&&p.requests.limit>0&&p.requests.remaining*5<=p.requests.limit)add('Board budget low: '+p.requests.remaining+'/'+p.requests.limit+' until '+(p.requests.reset||'unknown'),p);
   if(!p.error&&p.alive===false)add('Service observation unavailable',p);
   if(!p.error&&p.complete===false)add('Project observation incomplete',p);
   if(p.reports?.complete===false)add(p.reports.reason||(!p.error?'Reports not collected yet':''),p);
   if(!p.error)add(p.events_error,p);
   if(p.incident&&['local','unsupported-auth'].includes(p.incident.state))add(p.incident_line||'Incident report kept locally',p);
  }
  if(S?.messages?.complete===false)for(const reason of (S.messages.reason||'Message collection incomplete').split('; ')){
   const source=S.projects?.find(p=>reason===p.name+': messages source stale, incomplete or unavailable');
   if(!source?.error)add(reason,source);
  }
  for(const text of [S?.settings_error,S?.opacity_error,S?.ai?.error,S?.service?.state==='unknown'&&S.service.reason,S?.agentq?.kind==='unknown'&&S.agentq.reason,S?.conversations?.complete===false&&!S.conversations.loading&&(S.conversations.reason||'Conversation catalog coverage unknown'),...extra])add(text);
  return [...rows.values()];
 },
 entry:function(S,extra=[]){
  const rows=module.exports.rows(S,extra),label='Errors and service notices: '+rows.length;
  return h('details',{class:'errors-panel','data-view-key':'errors'},h('summary',{class:'errors-entry',title:label,'aria-label':label},icons.icon('circle-alert'),h('span',{class:'badge','aria-hidden':'true'},String(rows.length)),h('span',{class:'sr-only',role:'status','aria-live':'polite'},label+(rows.length?'. '+rows.map(r=>r.text).join('; '):''))),
   h('section',{class:'errors-popover','aria-label':'Errors and service notices'},h('strong',null,'Errors and service notices'),rows.length?h('ul',{class:'errors-list'},rows.map(r=>h('li',{key:r.key},h('details',{'data-view-key':'error:'+r.key},h('summary',null,r.text+(r.projects.size?', '+r.projects.size+' project'+(r.projects.size===1?'':'s'):'')),r.projects.size>0&&h('ul',{class:'error-projects'},[...r.projects].map(([root,p])=>h('li',{key:root},p.name+' · '+p.age))),!r.key.startsWith('limit:')&&r.text!==[...r.details].join('\n')&&h('div',{class:'meta'},[...r.details].join('\n')))))):h('p',null,'No current errors or service notices.')));
 }
});
