'use strict';
const {h,render,options}=require('preact');
const {useRef,useLayoutEffect}=require('preact/hooks');
const {signal}=require('@preact/signals');
const {Router,Route,useLocation}=require('wouter-preact');
const {memoryLocation}=require('wouter-preact/memory-location');
const ui=require('./ui-runtime.js'),{execution}=ui;
// Framework-scheduled product work keeps the initiating CoreQ execution context.
options.debounceRendering=callback=>ui.defer(callback);
const api=execution.module('shell',{
 escape:function(value){return String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));},
 bar:function({title,back,crumbs='',actions='',windows=''}){
  return '<header class="head"><div class="nav-back">'+(back?'<button data-a="back" title="Back" aria-label="Back">‹</button>':'')+'</div><div class="nav-title">'+(crumbs?'<nav class="breadcrumbs" aria-label="Breadcrumb">'+crumbs+'</nav>':'')+'<h1 tabindex="-1" title="'+api.escape(title)+'">'+api.escape(title)+'</h1></div><div class="nav-actions">'+actions+'</div><div class="window-controls" aria-label="Window controls">'+windows+'</div></header>';
 },
 // shortcut: parse legacy templates per draw; replace the adapter with route components in M5.
 // Public answer/resource islands keep exclusive ownership of the descendants they mount.
 node:function(node,index){
  if(node.nodeType===3)return node.textContent;
  if(node.nodeType!==1)return null;
  const props=Object.fromEntries([...node.attributes].map(a=>[a.name,a.value]));
  if(node.localName==='textarea')props.value=node.textContent;
  props.key=props['data-answer']||props['data-resources']||props['data-card']||props['data-view-key']||props.id||index;
  for(const name of ['checked','disabled','multiple','selected','open'])if(node.hasAttribute(name))props[name]=true;
  const island=node.hasAttribute('data-answer')||node.hasAttribute('data-resources')||node.hasAttribute('data-work');
  if(node.classList.contains('tqp'))return h(api.Island,{key:props.key,tag:node.localName,attributes:props,html:node.innerHTML});
  return h(node.localName,props,...(island||node.localName==='textarea'?[]:[...node.childNodes].map(api.node)));
 },
 Island:function({tag,attributes,html}){
  const ref=useRef(null),previous=useRef(null);
  useLayoutEffect(execution.bind(()=>{if(previous.current!==html){ref.current.innerHTML=html;previous.current=html;}}),[html]);
  return h(tag,{...attributes,ref});
 },
 Frame:function({frame}){const [route]=useLocation();return h(Route,{path:frame.route},h('div',{'class':'shell','data-shell-route':route,'data-shell-depth':frame.depth},frame.nodes));},
 create:function(container){
  // A3: the recorded Wouter history is the shell's Back stack; entries[i] keeps history[i]'s scroll, disclosures and focus.
  // ponytail: entries restored from native Settings carry no path; their route comes back on the next draw.
  const location=memoryLocation({record:true}),frame=signal(null),entries=[];let template=null;
  return execution.module('shell.instance',{
   entries:function(){return entries;},
   history:function(){return location.history;},
   push:function(state){entries.push(state);if(entries.length>32){entries.shift();if(location.history.length>1)location.history.shift();}location.navigate(location.history.at(-1));},
   pop:function(){const state=entries.pop();if(location.history.length>1){location.history.pop();location.navigate(location.history.at(-1),{replace:true});}return state;},
   clear:function(){entries.length=0;location.history.splice(0,location.history.length-1);},
   draw:function(html,route){
    template??=container.ownerDocument.createElement('template');template.innerHTML=html;
    frame.value={route,depth:location.history.length,nodes:[...template.content.childNodes].map(api.node)};
    if(location.history.at(-1)!==route)location.navigate(route,{replace:true});
    render(h(Router,{hook:location.hook},h(api.Frame,{frame:frame.value})),container);
   }
  });
 }
});
module.exports=api;
