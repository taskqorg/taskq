// M8/UI02: one schema and native Settings adapter for every route; no task or operation state.
'use strict';
const {execution}=require('./ui-runtime.js');
const {normalize,create}=execution.module('tray.viewState',{
normalize:function(saved,snapshot,routes,root='home'){
 const roots=new Set((snapshot.projects||[]).map(p=>p.root)),states=new Set(Object.keys(require('./project-blocks.js').marks));
 const {entry}=execution.module('tray.viewState.normalize',{
 entry:function(value){
  const v=value&&typeof value==='object'?value:{};
  let page=typeof v.page==='string'?v.page:root;
  const project=roots.has(page)?page:null;
  if(!project&&!Object.hasOwn(routes,page))page=v.project||page.startsWith('/')?(Object.hasOwn(routes,'projects')?'projects':root):page.startsWith('settings/')&&Object.hasOwn(routes,'settings')?'settings':Object.hasOwn(routes,v.parent)?v.parent:root;
  const valid=page===v.page,open={};
  const work=valid&&v.working&&typeof v.working==='object'&&!Array.isArray(v.working)?Object.fromEntries(Object.entries(v.working).filter(([root,on])=>roots.has(root)&&on===true)):null;
  if(valid&&v.open&&typeof v.open==='object')for(const [k,b] of Object.entries(v.open))if(typeof b==='boolean'&&k.length<=240)open[k]=b;
  return {page,operation:root==='messages'&&v.operation&&typeof v.operation.operation==='string'?{operation:v.operation.operation,root:typeof v.operation.root==='string'?v.operation.root:null}:null,project:roots.has(page)?page:null,parent:routes[page]?.parent||root,
   taskFilter:valid&&Array.isArray(v.taskFilter)?[...new Set(v.taskFilter.filter(s=>states.has(s)))]:null,
   working:work&&(Object.keys(work).length||!Object.keys(v.working).length)?work:null,
   aiRoot:roots.has(v.aiRoot)||(root==='messages'&&typeof v.aiRoot==='string')?v.aiRoot:null,answer:root==='messages'&&v.answer&&typeof v.answer.board==='string'&&Number.isSafeInteger(v.answer.task)&&v.answer.task>0?{board:v.answer.board,task:v.answer.task,revision:v.answer.revision,root:v.answer.root}:null,
   ...(root==='messages'?{messageFilter:['all','pending','archived'].includes(v.messageFilter)?v.messageFilter:'all',message:typeof v.message==='string'&&/^[a-f0-9]{64}$/.test(v.message)?v.message:null}:{}),scroll:valid&&Number.isFinite(v.scroll)&&v.scroll>=0?v.scroll:0,open,
   focus:valid&&v.focus&&/^(BUTTON|A|SUMMARY|INPUT|SELECT|TEXTAREA|H1|DIV)$/.test(v.focus.tag)&&typeof v.focus.key==='string'&&v.focus.key.length<=2048&&Number.isInteger(v.focus.index)&&v.focus.index>=0&&v.focus.index<4096?v.focus:null,
   unavailable:!valid&&typeof v.page==='string'?'Requested destination is no longer available.':typeof v.unavailable==='string'?v.unavailable.slice(0,160):null};
 }
 });
 if(saved?.schema!==1)return {schema:1,current:entry(null),trail:[],...(root==='home'?{dashboard:require('./task-views.js').normalize(null)}:{})};
 return {schema:1,...(root==='home'?{dashboard:require('./task-views.js').normalize(saved.dashboard,saved.current)}:{}),current:entry(saved.current),trail:(Array.isArray(saved.trail)?saved.trail:[]).slice(-32).filter(v=>v&&entry(v).page===v.page).map(entry)};
},
create:function(view,{menu,document,send,defer,cancel,routes,trail,root='home'}){
 let hydrated=false,timer=null,last='',restoring=false;
 const api=execution.module('tray.viewState.instance',{
 groups:function(){return Object.fromEntries([...menu.querySelectorAll('details[data-view-key]:not(.shell-overflow)')].map(d=>[d.dataset.viewKey,d.open]));},
 capture:function(){const active=document.activeElement?.closest?.('.shell-overflow')?.querySelector('summary')||document.activeElement,key=active&&JSON.stringify(active.dataset),focus=active&&menu.contains?.(active)?{tag:active.tagName,key,index:[...menu.querySelectorAll(active.tagName)].filter(e=>JSON.stringify(e.dataset)===key).indexOf(active)}:null;return {page:view.page,operation:view.operation,project:view.S?.projects?.some(p=>p.root===view.page)?view.page:null,parent:routes[view.page]?.parent||root,taskFilter:view.taskFilter,working:view.working,aiRoot:view.aiRoot,answer:view.answer,messageFilter:view.messageFilter,message:view.message,scroll:(menu.querySelector('.content')||document.scrollingElement)?.scrollTop||0,open:api.groups(),focus,unavailable:view.unavailable};},
 apply:function(v){view.page=v.page;view.operation=v.operation;view.answer=v.answer;view.message=v.message;view.messageFilter=v.messageFilter;view.taskFilter=v.taskFilter;view.working=v.working;view.aiRoot=v.aiRoot;view.disclosures=v.open;view.restoreScroll=v.scroll;view.restoreFocus=v.focus;view.unavailable=v.unavailable;},
 focus:function(saved){const target=saved?[...menu.querySelectorAll(saved.tag)].filter(e=>JSON.stringify(e.dataset)===saved.key)[saved.index]:null;(target?.checkVisibility?.()&&!target.disabled?target:menu.querySelector('h1'))?.focus({preventScroll:true});},
 hydrate:function(snapshot){
  if(snapshot.page!==undefined){hydrated=true;return;}
  if(!hydrated){hydrated=true;const state=normalize(snapshot.navigation,snapshot,routes,root);api.apply(state.current);if(root==='home'){view.dashboard=state.dashboard;if(!snapshot.navigation?.dashboard&&snapshot.navigation?.current?.taskFilter?.length){const p=snapshot.projects.find(p=>p.root===state.current.page);if(p)require('./task-views.js').current(view.dashboard).working.projects=[require('./task-views.js').identity(p)];}}trail.push(...state.trail);}
  else {
   const current=api.capture(),state=normalize({schema:1,current,trail},snapshot,routes,root);
   if(state.current.page!==view.page||JSON.stringify(state.current.working)!==JSON.stringify(view.working)||JSON.stringify(state.current.taskFilter)!==JSON.stringify(view.taskFilter))api.apply(state.current);
   view.aiRoot=state.current.aiRoot;
   for(let i=trail.length-1;i>=0;i--){const clean=normalize({schema:1,current:trail[i]},snapshot,routes,root).current;if(clean.page!==trail[i].page)trail.splice(i,1);else Object.assign(trail[i],clean);}
  }
 },
 restore:function(open){
  restoring=true;
  for(const d of menu.querySelectorAll('details[data-view-key]'))if(typeof open?.[d.dataset.viewKey]==='boolean'&&d.open!==open[d.dataset.viewKey])d.open=open[d.dataset.viewKey];
  restoring=false;
 },
 flush:function(){
  cancel(timer);timer=null;if(!hydrated||view.S?.page!==undefined)return;
  if(root==='home'&&view.dashboard&&view.page==='home')require('./task-views.js').current(view.dashboard).scroll=(menu.querySelector('.content')||document.scrollingElement)?.scrollTop||0;
  const navigation=normalize({schema:1,current:api.capture(),trail,...(root==='home'?{dashboard:view.dashboard}:{})},view.S,routes,root),text=JSON.stringify(navigation);
  if(text!==last){last=text;send({a:'set',navigation});}
 },
 schedule:function(){if(restoring)return;cancel(timer);timer=defer(api.flush,100);}
 });
 return api;
}
});
module.exports={normalize,create};
