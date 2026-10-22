'use strict';
const {execution}=require('./ui-runtime.js'),{states}=require('./project-blocks.js');
// MP11–MP13: local appearance/navigation only. Canonical board identity never uses registration paths.
const api=execution.module('taskViews',{
 identity:function(p){try{const u=new URL(p.issues);return (p.board_kind||'unknown')+'://'+u.host.toLowerCase()+u.pathname.replace(/\/(?:-\/)?issues\/?$/,'').replace(/\/$/,'');}catch{return null;}},
 criteria:function(v={}){v=v&&typeof v==='object'&&!Array.isArray(v)?v:{};return {states:states.map(([s])=>s).filter(s=>Array.isArray(v.states)&&v.states.includes(s)),projects:[...new Set(Array.isArray(v.projects)?v.projects.filter(p=>typeof p==='string'&&p.length<=2048):[])],query:typeof v.query==='string'?v.query.slice(0,1024):'',group:v.group==='combined'?'combined':'project'};},
 normalize:function(value,legacy={}){
  const views=Array.isArray(value?.views)?value.views.filter(v=>typeof v.id==='string'&&/^[a-z0-9-]{1,64}$/.test(v.id)&&typeof v.name==='string'&&v.name.trim()&&v.name.length<=80).slice(0,64).map(v=>({id:v.id,name:v.name,pinned:v.pinned===true,saved:api.criteria(v.saved),working:api.criteria(v.working||v.saved),scroll:Number.isFinite(v.scroll)&&v.scroll>=0?v.scroll:0})):[];
  if(!views.length){const saved=api.criteria();views.push({id:'all',name:'All tasks',pinned:true,saved,working:api.criteria({states:legacy.taskFilter||[]}),scroll:legacy.scroll||0});}
  const unique=[...new Map(views.map(v=>[v.id,v])).values()];
  return {schema:1,selected:unique.some(v=>v.id===value?.selected)?value.selected:unique[0].id,views:unique};
 },
 current:function(model){return model.views.find(v=>v.id===model.selected)||model.views[0];},
 dirty:function(view){return JSON.stringify(view.saved)!==JSON.stringify(view.working);},
 scope:function(projects,criteria){return projects.filter(p=>!criteria.projects.length||criteria.projects.includes(api.identity(p)));},
 matches:function(t,query){return !query||[t.title,t.n,t.project,t.runtime,t.host].filter(v=>v!=null).join(' ').toLocaleLowerCase().includes(query.toLocaleLowerCase());},
 priority:function(t){return (t.labels||[]).includes('p0')?0:(t.labels||[]).some(l=>['p1','priority-1'].includes(l))?1:2;},
 compare:function(a,b){const order=states.map(([s])=>s),x=order.indexOf(a.state),y=order.indexOf(b.state);
  return (x<0?order.length:x)-(y<0?order.length:y)||api.priority(a)-api.priority(b)||a.n-b.n||(a.boardIdentity||'').localeCompare(b.boardIdentity||'','en');},
 save:function(model){const v=api.current(model);v.saved=api.criteria(v.working);},
 reset:function(model){const v=api.current(model);v.working=api.criteria(v.saved);},
 add:function(model,name,id){name=name.trim();if(!name||name.length>80||model.views.length>=64)return false;const criteria=api.criteria(api.current(model).working);model.views.push({id,name,pinned:true,saved:criteria,working:api.criteria(criteria),scroll:0});model.selected=id;return true;}
});
module.exports=api;
