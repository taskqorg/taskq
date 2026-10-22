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
 // Markup for node tests and the workspace template until #1526; the tray renders the vnodes themselves.
 html:function(v){
  if(v==null||typeof v==='boolean')return '';
  if(Array.isArray(v))return v.map(api.html).join('');
  if(typeof v!=='object')return api.escape(v);
  if(typeof v.type==='function')return api.html(v.type(v.props));
  const {children,dangerouslySetInnerHTML:raw,key,ref,...attributes}=v.props;
  const open='<'+v.type+Object.entries(attributes).map(([k,x])=>x==null||x===false?'':x===true?' '+k:' '+k+'="'+api.escape(x)+'"').join('')+'>';
  return ['input','img','br'].includes(v.type)?open:open+(raw?raw.__html:api.html(children))+'</'+v.type+'>';
 },
 // shortcut: the workspace still parses its templates per draw until #1526 converts it to route components.
 // Public answer/resource islands keep exclusive ownership of the descendants they mount.
 node:function(node,index){
  if(node.nodeType===3)return node.textContent;
  if(node.nodeType!==1)return null;
  const props=Object.fromEntries([...node.attributes].map(a=>[a.name,a.value]));
  if(node.localName==='textarea')props.value=node.textContent;
  props.key=props['data-answer']||props['data-resources']||props['data-card']||props['data-view-key']||props.id||index;
  for(const name of ['checked','disabled','multiple','selected','open'])if(node.hasAttribute(name))props[name]=true;
  const island=node.hasAttribute('data-answer')||node.hasAttribute('data-resources')||node.hasAttribute('data-work');
  if(node.classList.contains('tqp'))return h(node.localName,{...props,dangerouslySetInnerHTML:{__html:node.innerHTML}});
  return h(node.localName,props,...(island||node.localName==='textarea'?[]:[...node.childNodes].map(api.node)));
 },
 Frame:function({frame}){const [route]=useLocation();return h(Route,{path:frame.route},h('div',{'class':'shell','data-shell-route':route,'data-shell-depth':frame.depth},frame.nodes));},
 create:function(container){
  // A3: the recorded Wouter history is the shell's Back stack; entries[i] keeps history[i]'s scroll, disclosures and focus.
  // ponytail: entries restored from native Settings carry no path; their route comes back on the next draw.
  const location=memoryLocation({record:true}),frame=signal(null),entries=[];let template=null;
  const instance=execution.module('shell.instance',{
   entries:function(){return entries;},
   history:function(){return location.history;},
   push:function(state){entries.push(state);if(entries.length>32){entries.shift();if(location.history.length>1)location.history.shift();}location.navigate(location.history.at(-1));},
   pop:function(){const state=entries.pop();if(location.history.length>1){location.history.pop();location.navigate(location.history.at(-1),{replace:true});}return state;},
   clear:function(){entries.length=0;location.history.splice(0,location.history.length-1);},
   // Route components render under the Wouter Router; shared HTML enters only as islands (dangerouslySetInnerHTML).
   render:function(nodes,route){
    frame.value={route,depth:location.history.length,nodes};
    if(location.history.at(-1)!==route)location.navigate(route,{replace:true});
    render(h(Router,{hook:location.hook},h(api.Frame,{frame:frame.value})),container);
   },
   draw:function(html,route){
    template??=container.ownerDocument.createElement('template');template.innerHTML=html;
    instance.render([...template.content.childNodes].map(api.node),route);
   }
  });
  return instance;
 }
});
module.exports=api;
