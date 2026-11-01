'use strict';
const time=require('./relative-time.js');
const {h}=require('preact'),{icon}=require('./ui-icons.js'),{execution}=require('./ui-runtime.js');
const kinds={ask:'Question',close:'Finished',failed:'Failed',gone:'Attempt ended',blocked:'Blocked'};
const api=execution.module('notificationPanel',{
 label:function(kind){return kinds[kind]||'Event';},
 // M14: result SHA owns result reads; local event markers never become queue/decision state.
 rows:function(projects=[],results=[],marks={}){
  const seen=new Set(),rows=[],ids=new Set(),closed=new Set(results.map(r=>r.root+'#'+r.n));
  for(const p of projects){
   const board=p.issues||p.root;if(seen.has(board))continue;seen.add(board);
   const add=r=>{r.id=JSON.stringify([board,r.source,r.n,r.source==='result'?r.sha:[r.at,r.kind,r.event]]);r.read=r.source==='result'?!!r.read:marks[r.id]==='read'||marks[r.id]==='cleared';if(marks[r.id]!=='cleared'&&!ids.has(r.id)){ids.add(r.id);rows.push(r);}};
   for(const r of results.filter(r=>r.root===p.root))add({...r,source:'result',resultKind:r.kind,kind:'close',at:r.closed,project:p.name});
   for(const e of p.events||[]){if(!kinds[e[1]]||e[1]==='close'&&closed.has(p.root+'#'+e[3]))continue;
    const task=(p.tasks||[]).find(t=>t.n===e[3]),result=results.find(r=>r.root===p.root&&r.n===e[3]);
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
 create:function({view,reports,send,draw}){
  let notice='';
  const model=execution.module('notificationPanel.view',{
   rows(){return api.rows(view.S.projects||[],reports.rows(),view.S.notify?.inbox||{});},
   reasons(){return [...new Set([...(reports.count()===null?['Results incomplete; check result collection.']:[]),...(view.S.projects||[]).flatMap(p=>p.events_error||p.alive===false?[p.name+': '+(p.events_error||'project observation stale')]:[])])];},
   label(){const n=model.rows().filter(r=>!r.read).length;return 'Notifications · '+n+(model.reasons().length?' loaded unread · history incomplete':' unread');},
   bell(){const n=model.rows().filter(r=>!r.read).length,label=model.label();return h('button',{class:'notifications-entry',type:'button','data-a':'workspace','data-page':'conversations','data-filter':'new',title:label,'aria-label':label},icon('bell'),n>0&&h('span',{class:'badge','aria-hidden':'true'},n>99?'99+':String(n)),model.reasons().length>0&&h('span',{class:'notification-warning','aria-hidden':'true'},icon('circle-alert')));},
   save(action,rows){if(rows.length)send({a:'notification-state',action,ids:rows.map(r=>r.id)});},
   read(rows){model.save('read',rows.filter(r=>r.source!=='result'&&!r.read));reports.markRead(rows.filter(r=>r.source==='result'&&!r.read));},
   receive(r){notice=r.reason||'';draw();},
   click(b){const action=b.dataset.notification;if(!action)return false;const rows=model.rows();if(action==='read')model.read(rows);else if(action==='clear')model.save('clear',rows.filter(r=>r.read));return true;},
   tools(){const rows=model.rows();return h('div',{class:'news-tools'},h('div',{class:'conversation-tools'},h('button',{'data-notification':'read',disabled:!rows.some(r=>!r.read)},'Mark all read'),h('button',{'data-notification':'clear',disabled:!rows.some(r=>r.read)},'Clear read')),notice&&h('p',{role:'alert'},notice),reports.readFailure()&&h('p',{role:'status'},reports.readFailure()),model.reasons().map(reason=>h('p',{role:'status',class:'meta',key:reason},reason)));},
   history(rows){return rows.length>0&&h('section',{class:'thread-events','aria-label':'News history'},rows.map(r=>h('p',{key:r.id,'data-event':r.id,class:'meta'},(r.read?'Read':'New')+' · '+(r.source==='result'&&r.resultKind!=='task'?'Report ready':kinds[r.kind])+' · '+r.project+' · #'+r.n,time.stamp(time.seconds(r.at),'Event recorded'),!r.read&&h('button',{'data-notification':'event-read','data-id':r.id,onClick:()=>model.read([r])},'Mark read'))));}

  });return model;
 }
});
module.exports=api;
