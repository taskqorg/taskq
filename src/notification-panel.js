'use strict';
const time=require('./relative-time.js');
const {h}=require('preact'),{icon}=require('./ui-icons.js'),{execution}=require('./ui-runtime.js');
const kinds={ask:'Question',close:'Finished',failed:'Failed',gone:'Attempt ended',blocked:'Blocked'};
const api=execution.module('notificationPanel',{
 label:function(kind){return kinds[kind]||'Event';},
 // M14: result SHA owns result reads; local event markers never become queue/decision state.
 rows:function(projects=[],results=[],marks={}){
  const seen=new Set(),rows=[],ids=new Set(),byRoot=new Map(),byTask=new Map();for(const r of results){if(!byRoot.has(r.root))byRoot.set(r.root,[]);byRoot.get(r.root).push(r);byTask.set(r.root+'#'+r.n,r);}const closed=new Set(byTask.keys());
  for(const p of projects){
   const board=p.issues||p.root;if(seen.has(board))continue;seen.add(board);const tasks=new Map((p.tasks||[]).map(t=>[t.n,t]));
   const add=r=>{r.id=JSON.stringify([board,r.source,r.n,r.source==='result'?r.sha:[r.at,r.kind,r.event]]);r.read=r.source==='result'?!!r.read:marks[r.id]==='read'||marks[r.id]==='cleared';if(marks[r.id]!=='cleared'&&!ids.has(r.id)){ids.add(r.id);rows.push(r);}};
   for(const r of byRoot.get(p.root)||[])add({...r,source:'result',resultKind:r.kind,kind:'close',at:r.closed,project:p.name});
   for(const e of p.events||[]){if(!kinds[e[1]]||e[1]==='close'&&closed.has(p.root+'#'+e[3]))continue;
    const task=tasks.get(e[3]),result=byTask.get(p.root+'#'+e[3]);
    add({source:'event',root:p.root,project:p.name,n:e[3],at:e[0],kind:e[1],event:e[2],title:task?.title||result?.title||'Task #'+e[3]});
   }
  }
  return rows.sort((a,b)=>b.at-a.at||b.n-a.n||a.id.localeCompare(b.id));
 },
 groups:function(rows){
  const groups=[],active=new Map();
  for(const row of rows){
   const series=/^(.*?)\b([1-9]\d*)\/([1-9]\d*)\b/.exec(row.title),ordinal=series&&Number(series[2]);
   const key=series&&ordinal<=Number(series[3])?JSON.stringify([row.root,row.kind,series[1],series[3]]):null,prior=active.get(key);
   if(key&&prior&&prior.rows.at(-1).at-row.at<=600&&!prior.ordinals.has(ordinal)){prior.rows.push(row);prior.ordinals.add(ordinal);}
   else {const group={title:series?.[1].trim(),rows:[row],ordinals:new Set([ordinal])};groups.push(group);if(key)active.set(key,group);}
  }
  return groups;
 },
 // M14: intersecting sets of canonical source identities, independent of event/page counts.
 attention:function(projects=[],results=[],messages={},marks={},owners=[],sidebar={}){
  const fresh=new Set(),needs=new Set(),owned=new Set(owners.map(c=>c.id)),links=new Map();for(const p of projects)for(const t of p.tasks||[])if(owned.has(t.conversation?.id))links.set((p.issues||p.root)+'/'+t.n,t.conversation.id);for(const r of results){const p=projects.find(p=>p.root===r.root);if(owned.has(r.conversation))links.set((p?.issues||r.root)+'/'+r.n,r.conversation);}
  const key=(board,n)=>links.has(board+'/'+n)?'owner:'+links.get(board+'/'+n):'task:'+board+'/'+n;
  const archived=new Set(owners.filter(c=>c.archived).map(c=>'owner:'+c.id));for(const id of Object.keys(sidebar.archived||{}))archived.add(id);for(const r of messages.entries||[])if(r.archived&&r.kind==='question'&&!links.has(r.board+'/'+r.task))archived.add(key(r.board,r.task));
  const add=(set,id)=>{if(!archived.has(id))set.add(id);};
  for(const r of api.rows(projects,results,marks))if(!r.read){const p=projects.find(p=>p.root===r.root);add(fresh,key(p?.issues||r.root,r.n));}
  for(const r of messages.entries||[])if(!r.archived){const id=r.kind==='question'?key(r.board,r.task):'owner:'+r.id;if(!r.read)add(fresh,id);}
  for(const p of projects)for(const t of p.tasks||[])if(t.state==='ask')add(needs,key(p.issues||p.root,t.n));
  for(const r of results)if(r.kind!=='task'&&r.visible!==false&&!r.pending_decision&&(!r.decision||r.decision==='none'||r.decision==='later'&&!(Date.parse(r.later_until)>Date.now()))){const p=projects.find(p=>p.root===r.root);add(needs,key(p?.issues||r.root,r.n));}
  return {fresh,needs};
 },
 create:function({view,reports,send,draw}){
  let notice='';const historyViews=new Map();
  const model=execution.module('notificationPanel.view',{
   rows(){return api.rows(view.S.projects||[],reports.rows(),view.S.notify?.inbox||{});},
   reasons(){return [...new Set([...(reports.count()===null?['Results incomplete; check result collection.']:[]),...(view.S.projects||[]).flatMap(p=>p.events_error||p.alive===false?[p.name+': '+(p.events_error||'project observation stale')]:[])])];},
   attention(){const sets=api.attention(view.S.projects||[],reports.rows(),view.S.messages,view.S.notify?.inbox||{},view.S.conversations?.items||[],view.S.conversations?.sidebar||{}),reasons=[...model.reasons(),...(view.S.messages?.complete!==true?['Message collection incomplete']:[]),...(view.S.conversations?.complete!==true?['Conversation collection incomplete']:[])];return {...sets,reasons};},
   label(a=model.attention()){return 'Conversations · '+a.fresh.size+(a.reasons.length?' loaded New':' New')+' · '+a.needs.size+(a.reasons.length?' loaded Needs me':' Needs me')+(a.reasons.length?' · Partial: '+a.reasons.join('; '):'');},
   entry(){const a=model.attention(),label=model.label(a),badge=(kind,n,mark)=>h('span',{class:'communication-count '+kind,title:kind+': '+n,'aria-label':kind+': '+n},icon(mark),h('span',{'aria-hidden':'true'},a.reasons.length?(n?'≥'+(n>99?'99':n):'?'):n>99?'99+':String(n)));
    return h('button',{class:'conversations-entry',type:'button','data-a':'workspace','data-page':'conversations','data-thread':'centre',title:label,'aria-label':label},icon('messages-square'),badge('New',a.fresh.size,'inbox'),badge('Needs me',a.needs.size,'message-circle-question'),a.reasons.length>0&&h('span',{class:'notification-warning'},icon('circle-alert')));},
   bell(){return model.entry();},
   save(action,rows){if(rows.length)send({a:'notification-state',action,ids:rows.map(r=>r.id)});},
   read(rows){model.save('read',rows.filter(r=>r.source!=='result'&&!r.read));reports.markRead(rows.filter(r=>r.source==='result'&&!r.read));},
   receive(r){notice=r.reason||'';draw();},
   click(b){const action=b.dataset.notification;if(!action)return false;const rows=model.rows();if(action==='read')model.read(rows);else if(action==='clear')model.save('clear',rows.filter(r=>r.read));return true;},
   tools(){const rows=model.rows();return h('div',{class:'news-tools'},h('div',{class:'conversation-tools'},h('button',{'data-notification':'read',disabled:!rows.some(r=>!r.read)},'Mark all read'),h('button',{'data-notification':'clear',disabled:!rows.some(r=>r.read)},'Clear read')),notice&&h('p',{role:'alert'},notice),reports.readFailure()&&h('p',{role:'status'},reports.readFailure()),model.reasons().map(reason=>h('p',{role:'status',class:'meta',key:reason},reason)));},
   history(rows=[],extra,key=rows[0]?.root+'#'+rows[0]?.n){
    if(!rows.length&&!extra)return null;
    const state=historyViews.get(key)||{open:false,start:0};historyViews.delete(key);historyViews.set(key,state);while(historyViews.size>24)historyViews.delete(historyViews.keys().next().value);
    const groups=[];for(const r of rows.slice(state.start,state.start+64)){const last=groups.at(-1),same=last&&last.rows[0].kind===r.kind&&last.rows[0].n===r.n&&last.rows[0].root===r.root&&last.rows[0].event?.attempt===r.event?.attempt;if(same)last.rows.push(r);else groups.push({rows:[r]});}
    return h('details',{class:'thread-events','data-view-key':'history:'+key,open:state.open,onToggle:e=>{if(state.open!==e.currentTarget.open){state.open=e.currentTarget.open;draw();}}},h('summary',{onClick:e=>{e.preventDefault();state.open=!state.open;e.currentTarget.parentElement.open=state.open;draw();}},'Earlier history'+(rows.length?' · '+rows.length+' events':'')),state.open&&[
     rows.some(r=>!r.read)&&h('button',{'data-notification':'thread-read',onClick:()=>model.read(rows)},'Mark thread read'),
     groups.map(g=>{const r=g.rows[0],text=(r.read?'Read':'New')+' · '+(r.source==='result'&&r.resultKind!=='task'?'Report ready':kinds[r.kind])+' · '+r.project+' · #'+r.n;return h('div',{key:r.id,'data-event':r.id,class:'meta'},h('p',null,text,g.rows.length>1?' ×'+g.rows.length:'',time.stamp(time.seconds(r.at),'Event recorded')),g.rows.length>1?h('details',null,h('summary',null,'Source events'),g.rows.map(r=>h('p',{key:r.id,'data-event':r.id},r.event||r.kind,time.stamp(time.seconds(r.at),'Event recorded')))):null);}),
     h('div',{class:'event-tools'},state.start>0&&h('button',{onClick:()=>{state.start=Math.max(0,state.start-64);draw();}},'Newer events'),state.start+64<rows.length&&h('button',{onClick:()=>{state.start+=64;draw();}},'Older events')),
     typeof extra==='function'?extra():extra]);}


  });return model;
 }
});
module.exports=api;
