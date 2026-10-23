'use strict';
const {h,render,options}=require('preact');
const {signal}=require('@preact/signals');
const {useLayoutEffect,useRef}=require('preact/hooks');
const {Router,Route,useLocation}=require('wouter-preact');
const {memoryLocation}=require('wouter-preact/memory-location');
const ui=require('./ui-runtime.js'),{execution}=ui;
// Framework-scheduled product work keeps the initiating CoreQ execution context.
options.debounceRendering=callback=>ui.defer(callback);
const api=execution.module('shell',{
 // R07/P-W3: one action row per window (Home, actions, window controls) whose icons never move; Back, breadcrumbs and
 // a title that does not fit take the row below.
 bar:function({title,back,home,crumbs,actions,windows}){
  const icons=require('./ui-icons.js');
  return h('header',{class:'head'+(back||crumbs?' title-below':'')},home&&h('button',{class:'nav-home','data-a':'home',title:home,'aria-label':home},icons.icon('house')),h('div',{class:'nav-title'},back&&h('button',{class:'nav-back','data-a':'back',title:'Back','aria-label':'Back'},icons.icon('arrow-left')),crumbs&&h('nav',{class:'breadcrumbs','aria-label':'Breadcrumb'},crumbs),h('h1',{tabindex:'-1',title},title)),h('div',{class:'nav-actions'},actions),h('div',{class:'window-controls','aria-label':'Window controls'},windows));
 },
 // A short title shares the action row; one that would be cut moves below, whole words only.
 fitTitle:function(container){
  const head=container.querySelector('.head'),title=head?.querySelector('h1');
  if(!title||head.classList.contains('title-below'))return;
  head.removeAttribute('data-title-below');
  if(title.scrollWidth>title.clientWidth+1)head.setAttribute('data-title-below','');
 },
 Frame:function({frame}){
  const [route]=useLocation(),element=useRef();
  useLayoutEffect(()=>{api.fitTitle(element.current);});
  useLayoutEffect(()=>{
   const el=element.current;let width;
   // R07: native resize and font loading happen after render; measuring the pre-commit title misses both.
   const observer=new ResizeObserver(()=>{const next=el.clientWidth;if(next!==width){width=next;api.fitTitle(el);}});
   observer.observe(el);
   ui.effect('shell.fonts',()=>document.fonts.ready,()=>{if(el.isConnected)api.fitTitle(el);});
   return ()=>observer.disconnect();
  },[]);
  return h(Route,{path:frame.route},h('div',{ref:element,'class':'shell','data-shell-route':route,'data-shell-depth':frame.depth},frame.nodes));
 },
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
    api.fitTitle(container);
   }
  });
  return instance;
 }
});
module.exports=api;
