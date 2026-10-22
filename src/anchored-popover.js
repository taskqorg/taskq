'use strict';
const {h}=require('preact'),{execution}=require('./ui-runtime.js');
// One bounded, top-layer surface; preview can become an interactive pinned dialog without changing its anchor.
module.exports=execution.module('anchoredPopover',{
 render:function({id='taskq-popover',label,children}){return h('section',{id,popover:'manual',role:'dialog','aria-label':label,class:'anchored-popover',tabindex:-1},children);},
 place:function(element,anchor){
  if(!element||!anchor)return;
  if(element.showPopover&&!element.matches(':popover-open'))element.showPopover();
  const gap=6,pad=8,w=window.innerWidth,h=window.innerHeight;
  element.style.maxWidth=Math.max(0,w-2*pad)+'px';element.style.maxHeight=Math.max(0,h-2*pad)+'px';
  const r=element.getBoundingClientRect(),below=h-anchor.bottom-gap-pad,above=anchor.top-gap-pad;
  const top=below>=r.height||below>=above?anchor.bottom+gap:anchor.top-gap-r.height;
  element.style.left=Math.max(pad,Math.min(anchor.left,w-r.width-pad))+'px';
  element.style.top=Math.max(pad,Math.min(top,h-r.height-pad))+'px';
 }
});
