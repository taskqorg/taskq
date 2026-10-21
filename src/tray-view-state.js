// M8/UI02: one schema and native Settings adapter for every route; no task or operation state.
'use strict';
const {execution}=require('./ui-runtime.js');
const {normalize,create}=execution.module('tray.viewState',{
normalize:function(saved,snapshot,routes){
 const roots=new Set((snapshot.projects||[]).map(p=>p.root)),states=new Set(require('./project-blocks.js').states.map(([s])=>s));
 const {entry}=execution.module('tray.viewState.normalize',{
 entry:function(value){
  const v=value&&typeof value==='object'?value:{};
  let page=typeof v.page==='string'?v.page:'home';
  const project=roots.has(page)?page:null;
  if(!project&&!Object.hasOwn(routes,page))page=v.project||page.startsWith('/')?'projects':page.startsWith('settings/')?'settings':Object.hasOwn(routes,v.parent)?v.parent:'home';
  const valid=page===v.page,open={};
  const work=valid&&v.working&&typeof v.working==='object'&&!Array.isArray(v.working)?Object.fromEntries(Object.entries(v.working).filter(([root,on])=>roots.has(root)&&on===true)):null;
  if(valid&&v.open&&typeof v.open==='object')for(const [k,b] of Object.entries(v.open))if(typeof b==='boolean'&&k.length<=240)open[k]=b;
  return {page,project:roots.has(page)?page:null,parent:routes[page]?.parent||'home',
   taskFilter:valid&&Array.isArray(v.taskFilter)?[...new Set(v.taskFilter.filter(s=>states.has(s)))]:null,
   working:work&&(Object.keys(work).length||!Object.keys(v.working).length)?work:null,
   aiRoot:roots.has(v.aiRoot)?v.aiRoot:null,scroll:valid&&Number.isFinite(v.scroll)&&v.scroll>=0?v.scroll:0,open};
 }
 });
 if(saved?.schema!==1)return {schema:1,current:entry(null),trail:[]};
 return {schema:1,current:entry(saved.current),trail:(Array.isArray(saved.trail)?saved.trail:[]).slice(-32).filter(v=>v&&entry(v).page===v.page).map(entry)};
},
create:function(view,{menu,document,send,defer,cancel,routes,trail}){
 let hydrated=false,timer=null,last='',restoring=false;
 const api=execution.module('tray.viewState.instance',{
 groups:function(){return Object.fromEntries([...menu.querySelectorAll('details[data-view-key]')].map(d=>[d.dataset.viewKey,d.open]));},
 capture:function(){return {page:view.page,project:view.S?.projects?.some(p=>p.root===view.page)?view.page:null,parent:routes[view.page]?.parent||'home',taskFilter:view.taskFilter,working:view.working,aiRoot:view.aiRoot,scroll:(menu.querySelector('.content')||document.scrollingElement)?.scrollTop||0,open:api.groups()};},
 apply:function(v){view.page=v.page;view.taskFilter=v.taskFilter;view.working=v.working;view.aiRoot=v.aiRoot;view.disclosures=v.open;view.restoreScroll=v.scroll;},
 hydrate:function(snapshot){
  if(snapshot.page!==undefined){hydrated=true;return;}
  if(!hydrated){hydrated=true;const state=normalize(snapshot.navigation,snapshot,routes);api.apply(state.current);trail.push(...state.trail);}
  else {
   const current=api.capture(),state=normalize({schema:1,current,trail},snapshot,routes);
   if(state.current.page!==view.page||JSON.stringify(state.current.working)!==JSON.stringify(view.working)||JSON.stringify(state.current.taskFilter)!==JSON.stringify(view.taskFilter))api.apply(state.current);
   view.aiRoot=state.current.aiRoot;
   for(let i=trail.length-1;i>=0;i--){const clean=normalize({schema:1,current:trail[i]},snapshot,routes).current;if(clean.page!==trail[i].page)trail.splice(i,1);else Object.assign(trail[i],clean);}
  }
 },
 restore:function(open){
  restoring=true;
  for(const d of menu.querySelectorAll('details[data-view-key]'))if(typeof open?.[d.dataset.viewKey]==='boolean'&&d.open!==open[d.dataset.viewKey])d.open=open[d.dataset.viewKey];
  restoring=false;
 },
 flush:function(){
  cancel(timer);timer=null;if(!hydrated||view.S?.page!==undefined)return;
  const navigation=normalize({schema:1,current:api.capture(),trail},view.S,routes),text=JSON.stringify(navigation);
  if(text!==last){last=text;send({a:'set',navigation});}
 },
 schedule:function(){if(restoring)return;cancel(timer);timer=defer(api.flush,100);}
 });
 return api;
}
});
module.exports={normalize,create};
