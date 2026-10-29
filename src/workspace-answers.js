'use strict';
const ui=require('./ui-runtime.js'),{execution}=ui,{h}=require('preact');
const {create}=execution.module('workspace.answers',{
create:function(view,{menu,document,send,draw,defer=ui.defer,cancel=ui.cancel,composer=false,changed,steer}){
 const answerRequests=new Map(),answerCards=new Map(),answerFeedback=new Map(),saved={},steers={};
 const api=execution.module('workspace.answers.instance',{
project:function(board,root=view.answer?.board===board?view.answer.root:null){return view.S?.projects.find(p=>p.issues===board&&(!root||p.root===root));},
answerCapability:function(board){const p=api.project(board);return {manage:true,answer:!!p&&p.alive&&p.complete===true,reason:'Project status is unavailable or stale; check the board',composer};},
// #1662 A3: the workspace's shared composer reads and drives the mounted card; the card keeps draft, op and receipt
answerCompose:function(board,row){return component().compose(row,saved[board+'#'+row.n]||component().fresh(row),answerCapability(board));},
answerInput:function(board,n,text,caret){answerCards.get(board+'/'+n)?.ui.input(board,n,text,caret);},
answerAct:function(board,n,type){answerCards.get(board+'/'+n)?.ui.act(board,n,type);},

answerReceipt:function(r){
  const key=r.root+'#'+r.id,request=answerRequests.get(key);
  const previous=answerFeedback.get(key);
  if(!request&&previous?.state!=='unknown')return;
  const item=request||previous;
  api.answerRefusal(r.state);
  answerFeedback.set(key,{board:item.board,op:item.op,about:item.about,state:r.state,reason:r.reason,next:r.next,answered:r.answered,...(r.note?{note:true}:{})});
  const local=saved[item.board+'#'+item.op.task];
  if(local?.op?.id===r.id)save(item.board,{n:item.op.task},{...local,receipt:{state:r.state,reason:r.reason,next:r.next,answered:r.answered,...(r.note?{note:true}:{})},...(r.state==='accepted'&&item.op.text?{draft:''}:{})});
  if(item.op.action==='steer'&&r.state==='accepted')steers[item.board+'#'+item.op.task]='';
  // M3: an accepted answer gets a 6 s overlay toast and joins Answered, which keeps the newest 20
  if(r.state==='accepted'){
    const answered=[...answerFeedback].filter(([,x])=>x.state==='accepted');
    if(answered.length>20)answerFeedback.delete(answered[0][0]);
    view.toast=key;defer(()=>{if(view.toast===key){view.toast=null;draw();}},6000);
  }
  if(request){
    cancel(request.timer);answerRequests.delete(key);
    request.resolve({state:r.state,reason:r.reason,next:r.next,...(r.note?{note:true}:{})});
  }else answerCards.get(item.board+'/'+item.op.task)?.ui.receiveReceipt(item.board,r.id,r);
  draw();

},
answerRefusal:function(state){
  switch(state){
   case 'rejected':execution.action('workspace.answer.refused.rejected',()=>{})();break;
   case 'failed':execution.action('workspace.answer.refused.failed',()=>{})();break;
   case 'stale':execution.action('workspace.answer.refused.stale',()=>{})();break;
   case 'partial':execution.action('workspace.answer.refused.partial',()=>{})();break;
   case 'unknown':execution.action('workspace.answer.refused.unknown',()=>{})();break;
  }
},
answerReadback:function(){
  for(const [key,r] of answerRequests){
    const p=api.project(r.board,r.root),t=p&&p.complete===true&&p.tasks.find(t=>t.n===r.op.task);
    if(r.reconciled||r.op.action||!t||(t.state==='ask'&&t.revision===r.op.revision))continue;
    r.reconciled=true;send({a:'answer',root:r.root,board:r.board,op:r.op});
  }
},
// The answer card owns the slot's descendants; Preact renders the slot without children.
answerSlot:function(t){return t.state==='ask'?h('div',{'data-answer':t.key,key:'answer:'+t.key}):null;},
answerNotices:function(board,task){
  // M3: receipts follow Attention or their project's task rows, never above them; accepted ones collapse into Answered, newest first
  // #1662 A1/A4: a receipt names its question and answer in plain words; operation id and revision stay behind a disclosure
  const {line}=execution.module("workspace.answers.notices",{line:([key,r],role)=>r.answered?h('div',{class:'body',role,key},'#'+r.op.task+' · '+won(r)+' ',h('a',{href:r.board+'/'+r.op.task,'data-url':r.board+'/'+r.op.task},'Board ↗'),h('button',{'data-a':'answer-dismiss','data-key':key,'aria-label':'Dismiss #'+r.op.task+' receipt',title:'Dismiss'},'×')):h('div',{class:'body'+(r.state==='accepted'?'':' alert'),role,key},'#'+r.op.task+(r.about?' '+r.about:'')+' · '+(STATE[r.state]||r.state)+(r.reason?': '+(r.reason===CONFLICT?'an earlier answer with this operation is still pending with different content':r.reason):'')+(r.next?' · '+r.next:'')+' ',h('a',{href:r.board+'/'+r.op.task,'data-url':r.board+'/'+r.op.task},'Board ↗'),['unknown','failed','partial'].includes(r.state)&&h('button',{'data-a':'answer-retry','data-key':key},r.state==='failed'&&r.reason!==CONFLICT?'Send again':'Check delivery'),h('details',{class:'ids','data-view-key':'receipt:'+key},h('summary',null,'Details'),'Operation '+r.op.id+' · question revision '+r.op.revision+(r.reason===CONFLICT?' · bridge: '+r.reason:'')),!answerRequests.has(key)&&h('button',{'data-a':'answer-dismiss','data-key':key,'aria-label':'Dismiss #'+r.op.task+' receipt',title:'Dismiss'},'×'))});
  // #1785: in its own thread an answer that won shows above the Steer composer, and steer receipts show in it
  const shown=[...answerFeedback].filter(([,r])=>(!board||r.board===board)&&(task==null||r.op.task===task&&!r.answered&&r.op.action!=='steer'&&(!saved[board+'#'+task]?.op||saved[board+'#'+task].op.id===r.op.id))&&!view.S.projects.some(p=>p.issues===r.board&&p.tasks.some(t=>t.n===r.op.task&&t.state==='ask')));
  if(task!=null)return shown.slice(-1).map(x=>line(x,'status'));
  const answered=shown.filter(([,r])=>r.state==='accepted').reverse();
  return [shown.filter(([,r])=>r.state!=='accepted').map(x=>line(x,'status')),answered.length>0&&h('details',{class:'group answered','data-view-key':'answered'},h('summary',null,'Answered'+counter(answered.length,'muted','Answered')),answered.map(x=>line(x,null)))];
},
// #1785 A1: a question another actor answered first names that answer, never revisions or CLI advice;
// the status row's `answered` (§ 5 late answer) names it before this client sends anything
answerWon:function(board,t){
  const r=t.state==='ask'?null:[...answerFeedback.values()].filter(r=>r.board===board&&r.op.task===t.n&&r.answered).at(-1)||(t.answered&&{answered:t.answered});
  return r?h('p',{class:'body answered',role:'status'},won(r)):null;
},
// ...and while its worker runs, the owner's unsent answer (a late receipt or a kept draft) can steer it instead, in the consumer's composer `steer`
answerSteerComposer:function(board,t){
  const key=board+'#'+t.n,late=[...answerFeedback.values()].some(r=>r.board===board&&r.op.task===t.n&&(r.answered||r.op.action==='steer'));
  if(!steer||t.state!=='doing'||!api.answerWon(board,t)||!late&&!(steers[key]??saved[key]?.draft??'').trim())return null;
  const text=steers[key]??saved[key]?.draft??'',r=[...answerFeedback.values()].filter(r=>r.board===board&&r.op.task===t.n&&r.op.action==='steer').at(-1);
  if(!text.trim()&&!r&&[...answerFeedback.values()].some(r=>r.board===board&&r.op.task===t.n&&r.answered&&r.state==='accepted'))return null;
  const sending=r?.state==='sending',reason=!r?'Your answer was not sent. Steer the working agent with it instead.':r.state==='accepted'?'Steer sent to the agent.':(STATE[r.state]||r.state)+(r.reason?': '+r.reason:'')+(r.next?' · '+r.next:'');
  return h('section',{class:'answer-composer','aria-label':'Steer #'+t.n},steer({text,disabled:sending,id:'steer-draft',field:'steer',label:'Steer the agent',placeholder:'Tell the working agent what to change',reason,
   tools:h('div',{class:'conversation-tools composer-bottom'},h('button',{'data-a':'answer-steer','data-board':board,'data-task':t.n,class:'primary-action',disabled:sending||!text.trim()},'Steer')),
   onInput:e=>{steers[key]=e.currentTarget.value;draw();}}));
},
answerSteer:function(board,t){
  const key=board+'#'+t.n,text=(steers[key]??saved[key]?.draft??'').trim();
  if(!text)return Promise.resolve(null);
  steers[key]=text;
  return answerSend(board,{id:'steer-'+t.n+'-'+Date.now().toString(36),task:t.n,revision:t.revision,action:'steer',text});
},
answerToast:function(){
  const r=view.toast&&answerFeedback.get(view.toast);
  return r?h('div',{class:'toast',role:'status'},'#'+r.op.task+' · '+(r.note?'task note saved':{start:'started',stop:'stopped',later:'parked',retry:'requeued',steer:'steer sent'}[r.op.action]||'answer accepted')+' ',h('a',{href:r.board+'/'+r.op.task,'data-url':r.board+'/'+r.op.task},'Board ↗'),h('button',{'data-a':'toast-dismiss','aria-label':'Dismiss',title:'Dismiss'},'×')):null;
},
mountAnswers:function(){
  for(const slot of menu.querySelectorAll('[data-answer]')){
    const t=unique(view.S.projects.filter(p=>!view.answer?.root||p.root===view.answer.root).flatMap(tasks)).find(t=>t.key===slot.dataset.answer);
    if(!t)continue;
    let card=answerCards.get(t.key);
    if(!card){const el=document.createElement('div');card={el,ui:component().mount(el,{capability:answerCapability,send:answerSend,load:load,save:save})};answerCards.set(t.key,card);}
    if(card.el.parentNode!==slot)slot.append(card.el);
    const p=api.project(t.key.slice(0,t.key.lastIndexOf('/')));
    card.ui.update([{board:p.issues,row:t}]);
    for(const d of slot.querySelectorAll('details')){const key='answer:'+t.key+':'+d.className;if(d.dataset.viewKey!==key)d.dataset.viewKey=key;}
  }

},
answerSend:function(board,op){
  const kept=saved[board+'#'+op.task];
  const p=api.project(board,kept?.op?.id===op.id?kept.projectRoot:undefined);
  if(!p){api.answerRefusal('rejected');return Promise.resolve({state:'rejected',reason:'Project is no longer registered',next:'Select a registered project, then send again'});}
  const key=p.root+'#'+op.id;
  if(answerRequests.has(key))return answerRequests.get(key).promise;
  let resolve;
  const promise=new Promise(r=>resolve=r);
  const timer=defer(()=>{answerReceipt({root:p.root,id:op.id,state:'unknown',reason:'No bridge receipt received',next:'Check delivery with the same operation'});},30000);
  answerRequests.set(key,{promise,resolve,timer,board,op,root:p.root});
  answerFeedback.set(key,{board,op,about:kept?.op?.id===op.id?kept.about:undefined,state:'sending',reason:''});
  send({a:'answer',root:p.root,board,op,local:saved[board+'#'+op.task]});
  return promise;
},

load:function(board,row){return saved[board+'#'+row.n];},
save:function(board,row,local){const prior=saved[board+'#'+row.n],projectRoot=prior?.op?.id&&prior.op.id===local.op?.id?prior.projectRoot:api.project(board)?.root;saved[board+'#'+row.n]=JSON.parse(JSON.stringify({...local,projectRoot}));send({a:'answer-local',key:board+'#'+row.n,local:saved[board+'#'+row.n]});changed?.();},
hydrate:function(locals){
 for(const [key,local] of Object.entries(locals||{}))if(!Object.hasOwn(saved,key)&&local&&typeof local.draft==='string'){
  saved[key]={...local,confirmClose:false};
  if(local.op&&['sending','unknown'].includes(local.receipt?.state))saved[key].receipt={state:'unknown',reason:'Workspace reopened; delivery needs readback',next:'Check delivery with the same operation'};
  const board=key.slice(0,key.lastIndexOf('#')),p=api.project(board,local.projectRoot);
  if(local.op&&p&&saved[key].receipt)answerFeedback.set(p.root+'#'+local.op.id,{board,op:local.op,about:local.about,...saved[key].receipt});
 }
},
retry:function(key){const r=answerFeedback.get(key);if(r)ui.effect('workspace.answer.reconcile',()=>answerSend(r.board,r.op),()=>{});},
dismiss:function(key){answerFeedback.delete(key);if(view.toast===key)view.toast=null;draw();}
 });
 const {answerCapability,answerReceipt,answerReadback,answerSlot,answerNotices,answerToast,mountAnswers,answerSend,load,save}=api;
 // #1699 A4: the bridge's retained correlation guard (tray.rs ActionQueue::submit), named in user terms
 const CONFLICT='Correlation id is already pending for a different request';
 const won=r=>'Answered by '+(r.answered.by||'another client')+': '+r.answered.text+(r.state==='accepted'&&r.reason?' · '+r.reason:'');
 const STATE={sending:'sending',accepted:'accepted',rejected:'not accepted',stale:'task changed; action not sent',unknown:'delivery unknown',failed:'not sent',partial:'partly applied'};
 const component=()=>require('./answer-card.js');
 const {tasks,unique,counter}=execution.module('workspace.answers.helpers',{
 tasks:p=>(p.tasks||[]).map(t=>({...t,key:p.issues+'/'+t.n,url:t.url||p.issues+'/'+t.n})),
 unique:ts=>[...new Map(ts.map(t=>[t.key,t])).values()],counter:(n)=>' · '+n
 });
 return api;
}
});
module.exports={create};
