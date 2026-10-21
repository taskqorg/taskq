'use strict';
const ui=require('./ui-runtime.js'),{execution}=ui,TaskqAnswerCard=require('./answer-card.js');
const {create}=execution.module('workspace.answers',{
create:function(view,{menu,document,send,draw,defer=ui.defer,cancel=ui.cancel}){
 const answerRequests=new Map(),answerCards=new Map(),answerFeedback=new Map(),saved={};
 const esc=require('./app-shell.js').escape,CSS=document.defaultView?.CSS||globalThis.CSS;
 const api=execution.module('workspace.answers.instance',{
answerCapability:function(board){const p=view.S&&view.S.projects.find(p=>p.issues===board);return {manage:true,answer:!!p&&p.alive&&p.complete===true,reason:'Project status is unavailable or stale; check the board'};},

answerReceipt:function(r){
  const key=r.root+'#'+r.id,request=answerRequests.get(key);
  const previous=answerFeedback.get(key);
  if(!request&&previous?.state!=='unknown')return;
  const item=request||previous;
  answerFeedback.set(key,{board:item.board,op:item.op,state:r.state,reason:r.reason,next:r.next});
  const local=saved[item.board+'#'+item.op.task];
  if(local?.op?.id===r.id)save(item.board,{n:item.op.task},{...local,receipt:{state:r.state,reason:r.reason,next:r.next},...(r.state==='accepted'&&item.op.text?{draft:''}:{})});
  // M3: an accepted answer gets a 6 s overlay toast and joins Answered, which keeps the newest 20
  if(r.state==='accepted'){
    const answered=[...answerFeedback].filter(([,x])=>x.state==='accepted');
    if(answered.length>20)answerFeedback.delete(answered[0][0]);
    view.toast=key;defer(()=>{if(view.toast===key){view.toast=null;draw();}},6000);
  }
  if(request){
    cancel(request.timer);answerRequests.delete(key);
    request.resolve({state:r.state,reason:r.reason,next:r.next});
  }else answerCards.get(item.board+'/'+item.op.task)?.ui.receiveReceipt(item.board,r.id,r);
  draw();
  
},
answerReadback:function(){
  for(const [key,r] of answerRequests){
    const p=view.S.projects.find(p=>p.issues===r.board),t=p&&p.complete===true&&p.tasks.find(t=>t.n===r.op.task);
    if(r.reconciled||r.op.action||!t||(t.state==='ask'&&t.revision===r.op.revision))continue;
    r.reconciled=true;send({a:'answer',root:p.root,board:r.board,op:r.op});
  }
},
answerSlot:function(t){return t.state==='ask'?'<div data-answer="'+esc(t.key)+'"></div>':'';},
answerNotices:function(board){
  // M3: receipts follow Attention or their project's task rows, never above them; accepted ones collapse into Answered, newest first
  const {line}=execution.module("workspace.answers.notices",{line:([key,r],role)=>'<div class="body'+(r.state==='accepted'?'':' alert')+'"'+role+'>#'+r.op.task+' · '+esc(r.state)+(r.reason?': '+esc(r.reason):'')+(r.next?' · '+esc(r.next):'')+' <a href="'+esc(r.board+'/'+r.op.task)+'" data-url="'+esc(r.board+'/'+r.op.task)+'">Board ↗</a>'+(r.state==='unknown'?'<button data-a="answer-retry" data-key="'+esc(key)+'">Check delivery</button>':'')+(answerRequests.has(key)?'':'<button data-a="answer-dismiss" data-key="'+esc(key)+'" aria-label="Dismiss #'+r.op.task+' receipt" title="Dismiss">×</button>')+'</div>'});
  const shown=[...answerFeedback].filter(([,r])=>(!board||r.board===board)&&!view.S.projects.some(p=>p.issues===r.board&&p.tasks.some(t=>t.n===r.op.task&&t.state==='ask')));
  const answered=shown.filter(([,r])=>r.state==='accepted').reverse();
  return shown.filter(([,r])=>r.state!=='accepted').map(x=>line(x,' role="status"')).join('')+(answered.length?'<details class="group answered" data-view-key="answered"><summary>Answered'+counter(answered.length,'muted','Answered')+'</summary>'+answered.map(x=>line(x,'')).join('')+'</details>':'');
},
answerToast:function(){
  const r=view.toast&&answerFeedback.get(view.toast);
  return r?'<div class="toast" role="status">#'+r.op.task+' · '+(r.op.action==='start'?'started':'answer accepted')+' <a href="'+esc(r.board+'/'+r.op.task)+'" data-url="'+esc(r.board+'/'+r.op.task)+'">Board ↗</a><button data-a="toast-dismiss" aria-label="Dismiss" title="Dismiss">×</button></div>':'';
},
mountAnswers:function(){
  for(const slot of menu.querySelectorAll('[data-answer]')){
    const t=unique(view.S.projects.flatMap(tasks)).find(t=>t.key===slot.dataset.answer);
    if(!t)continue;
    let card=answerCards.get(t.key);
    if(!card){const el=document.createElement('div');card={el,ui:TaskqAnswerCard.mount(el,{capability:answerCapability,send:answerSend,load:load,save:save})};answerCards.set(t.key,card);}
    if(card.el.parentNode!==slot)slot.append(card.el);
    const p=view.S.projects.find(p=>p.issues+'/'+t.n===t.key);
    card.ui.update([{board:p.issues,row:t}]);
    for(const d of slot.querySelectorAll('details')){const key='answer:'+t.key+':'+d.className;if(d.dataset.viewKey!==key)d.dataset.viewKey=key;}
  }

},
answerSend:function(board,op){
  const p=view.S&&view.S.projects.find(p=>p.issues===board);
  if(!p)return Promise.resolve({state:'rejected',reason:'Project is no longer registered'});
  const key=p.root+'#'+op.id;
  if(answerRequests.has(key))return answerRequests.get(key).promise;
  let resolve;
  const promise=new Promise(r=>resolve=r);
  const timer=defer(()=>{answerReceipt({root:p.root,id:op.id,state:'unknown',reason:'No bridge receipt received',next:'Check delivery with the same operation'});},30000);
  answerRequests.set(key,{promise,resolve,timer,board,op});
  answerFeedback.set(key,{board,op,state:'sending',reason:''});
  send({a:'answer',root:p.root,board,op,local:saved[board+'#'+op.task]});
  return promise;
},

load:function(board,row){return saved[board+'#'+row.n];},
save:function(board,row,local){saved[board+'#'+row.n]=JSON.parse(JSON.stringify(local));send({a:'answer-local',key:board+'#'+row.n,local:saved[board+'#'+row.n]});},
hydrate:function(locals){
 for(const [key,local] of Object.entries(locals||{}))if(!Object.hasOwn(saved,key)&&local&&typeof local.draft==='string'){
  saved[key]={...local,confirmClose:false};
  if(local.op&&['sending','unknown'].includes(local.receipt?.state))saved[key].receipt={state:'unknown',reason:'Workspace reopened; delivery needs readback',next:'Check delivery with the same operation'};
  const board=key.slice(0,key.lastIndexOf('#')),p=view.S.projects.find(p=>p.issues===board);
  if(local.op&&p&&saved[key].receipt)answerFeedback.set(p.root+'#'+local.op.id,{board,op:local.op,...saved[key].receipt});
 }
},
retry:function(key){const r=answerFeedback.get(key);if(r)ui.effect('workspace.answer.reconcile',()=>answerSend(r.board,r.op),()=>{});},
dismiss:function(key){answerFeedback.delete(key);if(view.toast===key)view.toast=null;draw();}
 });
 const {answerCapability,answerReceipt,answerReadback,answerSlot,answerNotices,answerToast,mountAnswers,answerSend,load,save}=api;
 const {tasks,unique,counter}=execution.module('workspace.answers.helpers',{
 tasks:p=>(p.tasks||[]).map(t=>({...t,key:p.issues+'/'+t.n,url:t.url||p.issues+'/'+t.n})),
 unique:ts=>[...new Map(ts.map(t=>[t.key,t])).values()],counter:(n)=>' · '+n
 });
 return api;
}
});
module.exports={create};
