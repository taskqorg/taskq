'use strict';
const {execution}=require('./ui-runtime.js');
// Published task controls `taskq.task-controls/1` (docs/design/task-controls.md): status rows list `controls`;
// the consumer sends op through its TaskQ dispatcher and shows the CLI receipt. Close stays a board operation.
const LABELS={start:'Start',stop:'Stop',later:'Later',retry:'Retry'},ICONS={start:'play',stop:'square',later:'clock',retry:'rotate-ccw'};
const {op,render,esc}=execution.module('task-controls',{
esc:function(s){return String(s).replace(/[&<>"']/g,c=>'&#'+c.charCodeAt(0)+';');},
op:function(row,action,prefix){return {id:prefix+'.'+action+'.'+row.n+'.'+row.revision,task:row.n,revision:row.revision,action};},
render:function(row,{key='',url='',board='unknown',except=[],project='Board'}={}){
  // #1781: icon controls with exact names, as in the approved menu details (input-scroll 05).
  const asset=require('./ui-icons.js').asset,id=esc(project+'#'+row.n);
  const buttons=(row.controls||[]).filter(a=>LABELS[a]&&!except.includes(a)).map(a=>'<button data-a="control" data-control="'+a+'" data-key="'+esc(key)+'" class="compact-action" title="'+LABELS[a]+' '+id+'" aria-label="'+LABELS[a]+' '+id+'">'+asset(ICONS[a])+'</button>');
  if(url)buttons.push('<a href="'+esc(url)+'" data-url="'+esc(url)+'" class="compact-action" title="Close '+id+' on the board" aria-label="Close '+id+' on the board">'+asset(board==='gitlab'?'gitlab':board==='github'?'github':'external-link')+'</a>');
  return buttons.length?'<span class="row task-controls">'+buttons.join('')+'</span>':'';
}
});
module.exports={VERSION:'taskq.task-controls/1',LABELS,op,render};
