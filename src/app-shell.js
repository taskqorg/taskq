'use strict';
const {h,render,options}=require('preact');
const {signal}=require('@preact/signals');
const {Router,Route,useLocation}=require('wouter-preact');
const {memoryLocation}=require('wouter-preact/memory-location');
const ui=require('./ui-runtime.js'),{execution}=ui;
// Framework-scheduled product work keeps the initiating CoreQ execution context.
options.debounceRendering=callback=>ui.defer(callback);
const api=execution.module('shell',{
 escape:function(value){return String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));},
 bar:function({title,back,crumbs,actions,windows}){
  return h('header',{class:'head'},h('div',{class:'nav-back'},back&&h('button',{'data-a':'back',title:'Back','aria-label':'Back'},'‹')),h('div',{class:'nav-title'},crumbs&&h('nav',{class:'breadcrumbs','aria-label':'Breadcrumb'},crumbs),h('h1',{tabindex:'-1',title},title)),h('div',{class:'nav-actions'},actions),h('div',{class:'window-controls','aria-label':'Window controls'},windows));
 },
 // Markup for node tests; the tray and workspace render the vnodes themselves.
 html:function(v){
  if(v==null||typeof v==='boolean')return '';
  if(Array.isArray(v))return v.map(api.html).join('');
  if(typeof v!=='object')return api.escape(v);
  if(typeof v.type==='function')return api.html(v.type(v.props));
  const {children,dangerouslySetInnerHTML:raw,key,ref,...attributes}=v.props;
  const open='<'+v.type+Object.entries(attributes).map(([k,x])=>x==null||x===false?'':x===true?' '+k:' '+k+'="'+api.escape(x)+'"').join('')+'>';
  return ['input','img','br'].includes(v.type)?open:open+(raw?raw.__html:api.html(children))+'</'+v.type+'>';
 },
 Frame:function({frame}){const [route]=useLocation();return h(Route,{path:frame.route},h('div',{'class':'shell','data-shell-route':route,'data-shell-depth':frame.depth},frame.nodes));},
 create:function(container){
  // A3: the recorded Wouter history is the shell's Back stack; entries[i] keeps history[i]'s scroll, disclosures and focus.
  // ponytail: entries restored from native Settings carry no path; their route comes back on the next draw.
  const location=memoryLocation({record:true}),frame=signal(null),entries=[];
  const instance=execution.module('shell.instance',{
   entries:function(){return entries;},
   push:function(state){entries.push(state);if(entries.length>32){entries.shift();if(location.history.length>1)location.history.shift();}location.navigate(location.history.at(-1));},
   pop:function(){const state=entries.pop();if(location.history.length>1){location.history.pop();location.navigate(location.history.at(-1),{replace:true});}return state;},
   clear:function(){entries.length=0;location.history.splice(0,location.history.length-1);},
   // Route components render under the Wouter Router; shared HTML enters only as islands (dangerouslySetInnerHTML).
   render:function(nodes,route){
    frame.value={route,depth:location.history.length,nodes};
    if(location.history.at(-1)!==route)location.navigate(route,{replace:true});
    render(h(Router,{hook:location.hook},h(api.Frame,{frame:frame.value})),container);
   }
  });
  return instance;
 }
});
module.exports=api;
