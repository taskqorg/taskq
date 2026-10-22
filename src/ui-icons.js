'use strict';
const {h}=require('preact'),{execution}=require('./ui-runtime.js');
const assets={...require('./project-blocks.js').assets,...require('./ui-icons.json')};
// Lucide static 1.17.0 (ISC) and bundled provider assets (CC0); see project-blocks.LICENSE.
module.exports=execution.module('uiIcons',{
 icon:function(name){return h('span',{class:'ui-icon','aria-hidden':'true',dangerouslySetInnerHTML:{__html:assets[name]||assets.circle}});},
 button:function(name,label,props={}){return h('button',{type:'button',title:label,'aria-label':label,...props},module.exports.icon(name));}
});
