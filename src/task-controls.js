'use strict';
const {execution}=require('./ui-runtime.js');
// Published task controls `taskq.task-controls/1` (docs/design/task-controls.md): status rows list `controls`;
// the consumer sends op through its TaskQ dispatcher and shows the CLI receipt. Close stays a board operation.
const LABELS={start:'Start',stop:'Stop',later:'Later',retry:'Retry'};
const {op,render,esc}=execution.module('task-controls',{
esc:function(s){return String(s).replace(/[&<>"']/g,c=>'&#'+c.charCodeAt(0)+';');},
op:function(row,action,prefix){return {id:prefix+'.'+action+'.'+row.n+'.'+row.revision,task:row.n,revision:row.revision,action};},
render:function(row,{key='',url='',except=[]}={}){
  const buttons=(row.controls||[]).filter(a=>LABELS[a]&&!except.includes(a)).map(a=>'<button data-a="control" data-control="'+a+'" data-key="'+esc(key)+'" aria-label="'+LABELS[a]+' #'+row.n+'">'+LABELS[a]+'</button>');
  if(url)buttons.push('<a href="'+esc(url)+'" data-url="'+esc(url)+'" aria-label="Close #'+row.n+' on the board">Close on board ↗</a>');
  return buttons.length?'<span class="row task-controls">'+buttons.join('')+'</span>':'';
}
});
module.exports={VERSION:'taskq.task-controls/1',LABELS,op,render};
