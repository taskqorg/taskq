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
 Frame:function({frame}){const [route]=useLocation();return h(Route,{path:frame.route},h('div',{'class':'shell','data-shell-route':route},frame.nodes));},
 create:function(container){
  const location=memoryLocation(),frame=signal(null),template=container.ownerDocument.createElement('template');
  return execution.module('shell.instance',{
   draw:function(html,route){
    template.innerHTML=html;
    frame.value={route,nodes:[...template.content.childNodes].map(api.node)};
    location.navigate(route,{replace:true});
    render(h(Router,{hook:location.hook},h(api.Frame,{frame:frame.value})),container);
   }
  });
 }
});
module.exports=api;
