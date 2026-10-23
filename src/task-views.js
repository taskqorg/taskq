'use strict';
const {execution}=require('./ui-runtime.js'),{marks}=require('./project-blocks.js'),states=Object.keys(marks);
const builtins=[['all','All tasks'],['completed','Completed']],periods=['7d','30d','all'],outcomes=['success','not_planned','unknown'];
// MP11–MP14, P-V1, P-W1: local appearance/navigation only. Canonical board identity never uses registration paths.
const api=execution.module('taskViews',{
 text:function(v,n){return typeof v==='string'?v.slice(0,n):'';},
 identity:function(p){try{const u=new URL(p.issues);return (p.board_kind||'unknown')+'://'+u.host.toLowerCase()+u.pathname.replace(/\/(?:-\/)?issues\/?$/,'').replace(/\/$/,'');}catch{return null;}},
 criteria:function(v={}){v=v&&typeof v==='object'&&!Array.isArray(v)?v:{};return {states:states.filter(s=>Array.isArray(v.states)&&v.states.includes(s)),projects:[...new Set(Array.isArray(v.projects)?v.projects.filter(p=>typeof p==='string'&&p.length<=2048):[])],query:api.text(v.query,1024),group:v.group==='combined'?'combined':'project',period:periods.includes(v.period)?v.period:'30d',outcomes:outcomes.filter(o=>Array.isArray(v.outcomes)&&v.outcomes.includes(o))};},
 view:function(v){return {id:v.id,name:v.name,pinned:v.pinned!==false,saved:api.criteria(v.saved),working:api.criteria(v.working||v.saved),scroll:Number.isFinite(v.scroll)&&v.scroll>=0?v.scroll:0,open:api.text(v.open,4096)||null,draft:api.text(v.draft,80)};},
 pin:function(p){return p&&typeof p.ref==='string'&&p.ref.length<=4096?{ref:p.ref,title:api.text(p.title,1024),url:api.text(p.url,4096)}:null;},
 builtin:function(id){return builtins.some(([b])=>b===id);},
 normalize:function(value,legacy={}){
  const valid=v=>v&&typeof v.id==='string'&&/^[a-z0-9-]{1,64}$/.test(v.id)&&typeof v.name==='string'&&v.name.trim()&&v.name.length<=80;
  const found=Array.isArray(value?.views)?value.views.filter(valid).slice(0,64):[];
  const views=[...new Map([...builtins.map(([id,name])=>found.find(v=>v.id===id)||{id,name,pinned:true,working:id==='all'?{states:legacy.taskFilter||[]}:{},scroll:id==='all'?legacy.scroll||0:0}),...found].map(v=>[v.id,api.view({...v,name:builtins.find(([b])=>b===v.id)?.[1]||v.name})])).values()];
  // P-W1: one global list in insertion order; migration unions earlier per-view pins without duplicates.
  const pins=[...new Map([...(Array.isArray(value?.pins)?value.pins:[]),...found.flatMap(v=>Array.isArray(v.pins)?v.pins:[])].map(api.pin).filter(Boolean).map(p=>[p.ref,p])).values()].slice(0,256);
  const u=value?.undo,undo=u&&valid(u.view)&&!api.builtin(u.view.id)?{view:api.view(u.view),index:Number.isInteger(u.index)?u.index:views.length,selected:u.selected===true,fallback:typeof u.fallback==='string'?u.fallback:''}:null;
  return {schema:1,selected:views.some(v=>v.id===value?.selected)?value.selected:'all',views,pins,undo};
 },
 current:function(model){return model.views.find(v=>v.id===model.selected)||model.views[0];},
 dirty:function(view){return JSON.stringify(view.saved)!==JSON.stringify(view.working);},
 scope:function(projects,criteria){return projects.filter(p=>!criteria.projects.length||criteria.projects.includes(api.identity(p)));},
 matches:function(t,query){return !query||[t.title,t.n,t.project,t.runtime,t.host,t.result].filter(v=>v!=null).join(' ').toLocaleLowerCase().includes(query.toLocaleLowerCase());},
 // #1592: `rank` is the display model's global order (state, priority, number, board); rows without one go last.
 compare:function(a,b){return (a.rank??Infinity)-(b.rank??Infinity)||0;},
 save:function(model){const v=api.current(model);v.saved=api.criteria(v.working);},
 reset:function(model){const v=api.current(model);v.working=api.criteria(v.saved);},
 add:function(model,name,id){name=name.trim();if(!name||name.length>80||model.views.length>=64)return false;const from=api.current(model),criteria=api.criteria(from.working);from.draft='';model.views.push(api.view({id,name,saved:criteria}));model.selected=id;return true;},
 // P-V1: built-ins stay; one latest undo record; deleting the active view falls back to All tasks with its own context.
 remove:function(model,id){const index=model.views.findIndex(v=>v.id===id);if(index<0||api.builtin(id))return false;const selected=model.selected===id;if(selected)model.selected='all';model.undo={view:model.views.splice(index,1)[0],index,selected,fallback:JSON.stringify(api.current(model).working)};return true;},
 undo:function(model){const u=model.undo;if(!u)return false;model.undo=null;if(model.views.some(v=>v.id===u.view.id)||model.views.length>=64)return false;model.views.splice(Math.min(u.index,model.views.length),0,u.view);if(u.selected&&model.selected==='all'&&JSON.stringify(api.current(model).working)===u.fallback)model.selected=u.view.id;return true;},
 pinned:function(model,ref){return model.pins.some(p=>p.ref===ref);},
 toggle:function(model,pin){const i=model.pins.findIndex(p=>p.ref===pin.ref);if(i>=0)model.pins.splice(i,1);else model.pins.push(api.pin(pin));}
});
module.exports=api;
