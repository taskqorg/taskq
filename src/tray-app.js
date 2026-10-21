'use strict';
const ui=require('./ui-runtime.js');
const execution=ui.execution;

const {createState,create}=execution.module("tray",{
createState:function(snapshot=null){return {manualSize:null,aiText:null,aiMenu:false,aiConfirm:null,addConfirm:false,resourceNotice:null,toast:null,receivedAt:0,S:snapshot,page:'home',taskFilter:null,working:null,workCard:null,ask:null,noted:null,pending:{},hostCommand:null,recovery:{},confirming:null,updating:null,draft:null,disclosures:[],pressedAt:null,stale:false,resizing:null};},
create:function(view,ports){
const api=execution.module("tray.create",{
gutter:function(){if(ports.measureScrollbar)return ports.measureScrollbar();const d=document.createElement('div');d.style.cssText='position:absolute;top:-99px;width:60px;height:60px;overflow:scroll;scrollbar-width:thin';document.body.append(d);const g=d.offsetWidth-d.clientWidth;d.remove();return Math.ceil(g);},
fit:function(){const size=view.manualSize||view.S.size,fixed=(view.S.size_anchor||(size?'fixed':'top'))==='fixed',w=view.page==='recovery'?Math.min(720,window.screen?.availWidth||720):size?size[0]:view.S.font>=18?360:320;document.body.style.width='100%';menu.style.width=w+'px';send({a:'size',d:window.devicePixelRatio,g:fixed&&size?0:gutter(),w,h:fixed&&size?size[1]:Math.ceil(menu.querySelector('.content')?menu.querySelector('.content-body').getBoundingClientRect().height+12+menu.querySelector('.head').offsetHeight:menu.getBoundingClientRect().height)});menu.style.width='100%';},
actionReceipt:function(r){if(!view.S?.projects.some(p=>p.root===r.root))return;actionErrors.set(r.root,{text:r.text});present();},
actionNotices:function(){const r=view.hostCommand||view.S?.lifecycle;return view.S.projects.filter(p=>actionErrors.has(p.root)).map(p=>'<div class="body alert" role="alert">'+esc(p.name)+': '+esc(actionErrors.get(p.root).text)+'</div>').join('')+(r?.action==='uninstall'?'<div class="body meta" role="status">Uninstall '+esc(r.id||'')+': '+esc(r.state||'unknown')+(r.reason?' · '+esc(r.reason):'')+'</div>':'');},
lifecycleStart:function(root,action='restart',mode='drain'){
  if(view.hostCommand&&!view.hostCommand.unknown&&view.hostCommand.state!=='observed')return;
  const review=action==='uninstall'?view.S.projects.find(p=>p.root===root)?.uninstall_review:action==='stop'?(mode==='full'?view.S.full_stop_review:view.S.stop_review):view.S.restart_review;
  if(!review||typeof review.instance!=='string'||!Array.isArray(review.roots)){actionErrors.set(root,{text:review?.reason||'Lifecycle review unavailable; check host service status.'});present();return;}
  const id=Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');
  const item={id,root,project:review.project,action,mode,stops:action==='uninstall'&&review.roots.length===1,instance:review.instance,state:'sending',unknown:false};view.hostCommand=item;actionErrors.delete(root);
  defer(()=>{if(view.hostCommand!==item||item.state==='observed')return;item.unknown=true;item.state='unknown';item.reason=({stop:'Stop',uninstall:'Uninstall'}[action]||'Restart')+' confirmation unknown; check service status for '+id+'.';present();},30000);
  send({...review,id,a:action,root});present();
},
lifecycleReceipt:function(r){
  const item=view.hostCommand;if(!item||r.id!==item.id)return;
  if(!['queued','applied','observed'].includes(r.state)){item.unknown=true;item.state='unknown';item.reason=r.reason||'Lifecycle outcome unknown; check service status.';}
  else if(r.state==='observed'&&!api.lifecycleObserved(r,item.id,item.instance,item.action,item.project,item.stops)){item.unknown=true;item.state='unknown';item.reason='Lifecycle receipt is incomplete; check service status.';}
  else {item.state=r.state;item.reason=r.reason||'';}
  api.lifecycleReadback(r);present();
},
lifecycleObserved:function(r,id,origin,action='restart',project=null,stops=false){
  const receipts=(!r?.action||r.action===action)&&r?.id===id&&r.state==='observed'&&['received_receipt','applied_receipt','observed_receipt'].every(k=>typeof r[k]==='string'&&/^[a-f0-9]{32}$/.test(r[k]));
  if(!receipts)return false;
  if(action==='uninstall'&&(r.action!=='uninstall'||r.origin_instance!==origin||r.project!==project||r.unregistered!==true))return false;
  if(action==='uninstall'&&!stops)return typeof r.service_instance==='string'&&/^[A-Za-z0-9_-]{1,64}$/.test(r.service_instance);
  if(action==='stop'||action==='uninstall')return r.action===action&&r.origin_instance===origin&&(action!=='uninstall'||r.unit_removed===true)&&r.native_exit===true&&Number.isInteger(r.native_pid)&&r.native_pid>0&&typeof r.native_started==='string'&&/^[A-Za-z0-9-]{1,128}$/.test(r.native_started)&&typeof r.unit_revision==='string'&&/^[a-f0-9]{64}$/.test(r.unit_revision);
  return typeof r.service_instance==='string'&&/^[A-Za-z0-9_-]{1,64}$/.test(r.service_instance)&&typeof origin==='string'&&r.service_instance!==origin;
},
lifecycleReadback:function(r=view.S?.lifecycle){
  const item=view.hostCommand;
  if(item&&r?.id===item.id&&['queued','applied'].includes(r.state)){item.state=r.state;item.reason=r.reason||'';}
  if(item&&api.lifecycleObserved(r,item.id,item.instance,item.action,item.project,item.stops)){
    item.state='observed';item.unknown=false;item.reason=item.action==='uninstall'?'Project removal observed.'+(item.stops?' Host unit removal and exact exit observed.':' Other projects continue.'):item.action==='stop'?'Recorded host exit observed; restart: '+(r.restart_condition||'check native autostart conditions')+'.':'New service generation observed.';
  }
},
lifecycleWaits:function(){
  if(!Array.isArray(view.S?.projects))return '<div class="body meta">Project registry unknown</div>';
  return view.S.projects.map(p=>{
    const current=view.S.service?.state==='running'&&typeof p.service_pid==='number'&&typeof p.host_instance==='string'&&typeof p.age==='number'&&p.age>=0&&p.age<=60&&p.service_pid===view.S.service?.pid&&p.host_instance===view.S.service?.instance;
    const text=!current?'waiting for current project observation':p.error?p.error:Array.isArray(p.restart_waits)?p.restart_waits.length?'waits for '+p.restart_waits.join(', '):'prepared':'waiting for recovery observation';
    return '<div class="body meta">'+esc(p.name)+': '+esc(text)+'</div>';
  }).join('');
},
lifecycleControl:function(p,action='restart'){
  const title={stop:'Stop',uninstall:'Uninstall'}[action]||'Restart',review=action==='uninstall'?p.uninstall_review:action==='stop'?view.S?.stop_review:view.S?.restart_review;
  const all=view.hostCommand||view.S?.lifecycle,r=(all?.action||'restart')===action&&(action!=='uninstall'||(all.root||all.project)===p.root)?all:null;
  const pending=view.hostCommand&&!view.hostCommand.unknown&&view.hostCommand.state!=='observed'||['queued','applied'].includes(view.S?.lifecycle?.state);
  const projects=Array.isArray(view.S?.projects)?view.S.projects:[];
  const targets=Array.isArray(review?.roots)&&review.roots.every(root=>typeof root==='string')?review.roots.map(root=>projects.find(p=>p.root===root)?.name||root):null;
  const effect=action==='uninstall'?'Waits for this project’s owned work to end; files and other projects stay.'+(targets?.length===1?' Removes the last host unit after exact exit.':''):action==='stop'?(view.S?.stop_modes?'Drain: durable hold; current work finishes. Automatic rollback to older releases is disabled. Use reviewed recovery to resume selected tasks.':'Waits for all owned work to end; retains OS autostart. Explicit service install can start it again.'):'Existing work is preserved; active turns can delay restart.';
  const scope=action==='uninstall'?esc(p.name):'all registered projects: '+(targets?targets.length?targets.map(esc).join(', '):'none registered':'unknown');
  return '<details class="host-'+action+'"><summary>'+title+' '+(action==='uninstall'?esc(p.name):'host service')+'</summary><div class="body meta">Affects '+scope+'. '+effect+'</div>'+(!review?.instance?'<div class="body alert">'+esc(review?.reason||title+' protocol unavailable')+'</div>':'')+'<button class="project" data-a="'+action+'" data-root="'+esc(p.root)+'"'+(pending||!review?.instance||!targets?' disabled':'')+'>Confirm '+(action==='uninstall'?'uninstall '+esc(p.name):'host '+action)+'</button>'+(pending&&r?api.lifecycleWaits():'')+(r?.id||r?.reason?'<div class="body meta" role="status">'+title+' '+esc(r.id||'')+': '+esc(r.state||'unknown')+(r.reason?' · '+esc(r.reason):'')+'</div>':'')+'</details>';
},
stopPanel:function(){
  const projects=view.S.projects||[],root=projects[0]?.root,held=view.S.stop_hold;
  const pending=!!held&&!held.selected_count||view.hostCommand&&!view.hostCommand.unknown&&view.hostCommand.state!=='observed'||['queued','applied'].includes(view.S.lifecycle?.state);
  const names=projects.map(p=>esc(p.name)).join(', ')||'no registered projects';
  const host=projects[0]?.host||'unknown host',command=view.hostCommand||view.S.lifecycle;
  const rows=projects.map(p=>'<div class="body"><strong>'+esc(p.name)+'</strong><div class="meta">'+(view.S.service?.state!=='running'||typeof p.age!=='number'||p.age<0||p.age>60||p.service_pid!==view.S.service?.pid||p.host_instance!==view.S.service?.instance?'Process state unknown: service observation stale or from another generation.':Array.isArray(p.stop_progress)&&p.stop_progress.length?p.stop_progress.map(k=>'#'+esc(k.task)+' '+esc(k.rt)+': '+esc(k.state)+(k.reason?' · '+esc(k.reason):'')).join('<br>'):'No stop acknowledgement; running or unknown work may remain.')+'</div></div>').join('');
  return '<div class="body">Stop on '+esc(host)+' · '+names+'</div><div class="body meta">Only TaskQ-owned local work is targeted. Remote hosts and unrelated agents are unaffected. Dirty files and session references are retained; unsaved buffers and interrupted external effects may be lost. Automatic rollback to older releases is disabled.</div>'+
    '<div class="body"><strong>Drain</strong><div class="meta">Hold new admission and let current work finish. Retain autostart and the hold after service exit.</div></div><button class="project" data-a="stop" data-root="'+esc(root||'')+'"'+(pending||!root||!view.S.stop_review?.instance?' disabled':'')+'>Confirm Drain</button>'+
    '<div class="body"><strong>Full stop</strong><div class="meta">Request cancellation now, including offline. Unknown termination stays fenced. Board reconciliation may keep the service alive.</div></div><button class="project" data-a="stop-full" data-root="'+esc(root||'')+'"'+(pending||!root||!view.S.full_stop_review?.instance?' disabled':'')+'>Confirm Full stop</button>'+
    (!view.S.full_stop_review?.instance?'<div class="body alert">'+esc(view.S.full_stop_review?.reason||'Full stop capability unknown')+'</div>':'')+
    (held?'<div class="body alert" role="status">Admission held: '+esc(held.mode||'unknown')+'; ordinary Play cannot clear this hold.</div>':'')+(held?'<button class="project" data-route="recovery">Review recovery</button>':'')+(command?.action==='stop'?'<div class="body meta" role="status">Stop '+esc(command.id||'')+': '+esc(command.state||'unknown')+(command.reason?' · '+esc(command.reason):'')+'</div>':'')+rows+api.lifecycleWaits();
},
recoveryPanel:function(){
  return '<div class="body">Only selected local tasks resume. Keep held is the default. Paused projects, other tasks and operators stay held.</div><div class="body meta">Inspect native release, work and external effects before confirming. Unknown effects must remain held. Restart discards the old result and starts from preserved files.</div>'+view.S.projects.map(p=>{
    const item=view.recovery[p.root],review=item?.review;
    return '<section class="group" data-recovery-project="'+esc(p.root)+'"><div class="body"><strong>'+esc(p.name)+'</strong></div><button data-a="recovery-read" data-root="'+esc(p.root)+'"'+(item?.pending?' disabled':'')+'>Read current recovery</button>'+(item?.notice?'<div class="body alert" role="status">'+esc(item.notice)+'</div>':'')+(review?'<div class="body meta">'+esc(review.mode)+' · focus '+esc((review.focus||[]).join(', ')||'off')+'</div>'+'<div class="body meta">Limits: '+esc(review.limits?.unknown||Object.entries(review.limits?.limits||{}).map(([k,v])=>k+' '+v).join(', ')||'unknown')+'<br>Access: '+esc((review.access||[]).map(a=>a.runtime+': '+(a.refused||a.effective||'unknown')).join('; ')||'unknown')+'</div>'+review.rows.map(r=>{
      const draft=item.drafts?.[r.task]||{},blocked=!r.eligible||r.selected||item.pending||(review.pending||[]).some(q=>q.rows.some(s=>s.task===r.task));
      return '<div class="body"><label><input type="checkbox" data-recovery-task="'+r.task+'" data-root="'+esc(p.root)+'"'+(draft.selected?' checked':'')+(blocked?' disabled':'')+'> #'+r.task+' '+esc(r.title||'Task')+'</label><div class="meta">'+esc(r.runtime||'unknown runtime')+' · '+esc(r.host||'unknown host')+' · '+esc(r.native_exit||'unknown')+'<br>Execution '+esc(r.execution??'unknown')+' · revision '+esc(r.revision??'unknown')+'<br>'+esc(r.workspace?.state||'work unknown')+(r.blocker?'<br>'+esc(r.blocker):'')+(r.selected?'<br>Selected admission confirmed; running state is separate.':'')+'</div><a href="'+esc(r.url||'#')+'" data-url="'+esc(r.url||'')+'">Inspect task and receipts ↗</a><label class="meta recovery-evidence">Reconciled effect outcomes<input type="text" maxlength="160" data-recovery-evidence="'+r.task+'" data-root="'+esc(p.root)+'" aria-label="Effect evidence for task '+r.task+'" value="'+esc(draft.evidence||'')+'"'+(blocked?' disabled':'')+'></label></div>';
    }).join('')+(review.pending||[]).map((q,i)=>'<button data-a="recovery-pending" data-root="'+esc(p.root)+'" data-index="'+i+'"'+(item.pending?' disabled':'')+'>Reconcile saved selection #'+esc(q.rows[0].task)+'</button>').join('')+'<div class="body meta">Confirm only when the evidence establishes the effects; native release remains mandatory. Current grants, limits and focus still apply. If the original host exit is verified, confirmation starts host supervision once.</div><button data-a="recovery-apply" data-root="'+esc(p.root)+'"'+(item.pending?' disabled':'')+'>Confirm selected recovery</button>':'')+(item?.request&&!item.pending?'<button data-a="recovery-reconcile" data-root="'+esc(p.root)+'">Reconcile same selection</button>':'')+'</section>';
  }).join('');
},
recoveryStart:function(root,action){
  const item=view.recovery[root]||(view.recovery[root]={drafts:{}});if(item.pending)return;
  let request;
  if(action==='recovery-reconcile'){request=item.request;if(!request)return;action='recovery-apply';}
  else if(action==='recovery-apply'){
    const review=item.review;if(!review)return;
    const rows=review.rows.filter(r=>r.eligible&&!r.selected&&item.drafts[r.task]?.selected).map(r=>({task:r.task,execution:r.execution,revision:r.revision,workspace:r.workspace.fingerprint,evidence:item.drafts[r.task].evidence||'',effects_known:true}));
    if(!rows.length||rows.length>8||rows.some(r=>!r.evidence.trim())){item.notice='Select 1–8 eligible tasks and record verified effect outcomes for each.';present();return;}
    request={stop:review.stop,board:review.board,rows};item.request=JSON.parse(JSON.stringify(request));
  }
  item.id=Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');item.pending=true;item.notice=action==='recovery-read'?'Reading current evidence…':'Applying selected recovery…';
  const id=item.id;send({a:action,root,id,...(request?{request}:{})});
  defer(()=>{if(item.id!==id||!item.pending)return;item.pending=false;item.notice='Outcome unknown. Reconcile the same selection; do not submit changed evidence.';present();},65000);present();
},
recoveryReceipt:function(r){
  const item=view.recovery[r.root];if(!item||r.id!==item.id)return false;
  item.pending=false;
  if(r.state==='review'&&Array.isArray(r.rows)){item.review=r;item.drafts={};item.notice='Review current evidence; nothing is selected.';}
  else if(r.state==='mixed'&&Array.isArray(r.receipts)){
    item.notice=r.receipts.map(x=>'#'+x.task+': '+x.outcome+(x.reason?' · '+x.reason:'')).join('; ')+' · Mixed; unselected work stays held.'+(r.service?' Service: '+r.service.state+(r.service.reason?' · '+r.service.reason:''):'');
    for(const receipt of r.receipts)if(receipt.outcome==='accepted'){const row=item.review?.rows.find(r=>r.task===receipt.task);if(row)row.selected=true;}
  }else item.notice=r.reason||'Recovery outcome unknown; reconcile the same selection.';
  present();return true;
},
focusStart:function(action,root,label){
  if(view.pending[root]&&!view.pending[root].unknown)return;
  const item={id:Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join(''),text:action==='pause'?'Pausing':'Resuming',receipt:null,unknown:false};
  view.pending[root]=item;actionErrors.delete(root);
  defer(()=>{if(view.pending[root]!==item)return;item.unknown=true;actionErrors.set(root,{id:item.id,text:'Command confirmation unknown. Check service status before another command.'});present();},30000);
  send({a:action,root,label,id:item.id});
},
focusReceipt:function(r){
  const item=view.pending[r.root];
  if(!view.S?.projects.some(p=>p.root===r.root)||!item||item.id!==r.id)return;
  if(typeof r.applied_receipt==='string'&&/^[a-f0-9]{32}$/.test(r.applied_receipt)){
    item.receipt=r.applied_receipt;item.text='Waiting for service';focusReadback();
  }else{delete view.pending[r.root];actionErrors.set(r.root,{id:item.id,text:r.text||'Command confirmation unavailable. Check service status.'});}
  present();
},
focusReadback:function(){
  for(const p of view.S.projects){
    const item=view.pending[p.root],ack=p.focus_command;
    if(item?.receipt&&typeof p.age==='number'&&p.age>=0&&p.age<=60&&ack?.state==='observed'&&ack.applied_receipt===item.receipt&&typeof ack.observed_receipt==='string'&&/^[a-f0-9]{32}$/.test(ack.observed_receipt)){
      delete view.pending[p.root];if(actionErrors.get(p.root)?.id===item.id)actionErrors.delete(p.root);
    }
  }
  for(const root of Object.keys(view.pending))if(!view.S.projects.some(p=>p.root===root))delete view.pending[root];
},
remember:function(){return {page:view.page,taskFilter:view.taskFilter,working:view.working,workCard:view.workCard,scroll:(menu.querySelector('.content')||document.scrollingElement).scrollTop,open:[...menu.querySelectorAll('details')].map(d=>d.open)};},
navigate:function(next,filter=null,work=null){
  trail.push(remember());view.page=next;view.taskFilter=filter===null?null:[filter];view.working=work;view.workCard=null;view.ask=null;view.noted=null;view.disclosures=[];
  present(true);(menu.querySelector('.content')||document.scrollingElement).scrollTop=0;
  menu.querySelector('h1')?.focus();
},
back:function(){
  const prior=trail.pop()||{page:'home',taskFilter:null,working:null,workCard:null,scroll:0,open:[]};
  ({page:view.page,taskFilter:view.taskFilter,working:view.working,workCard:view.workCard}=prior);view.noted=null;view.disclosures=prior.open;
  present(true);(menu.querySelector('.content')||document.scrollingElement).scrollTop=prior.scroll;
  menu.querySelector('h1')?.focus({preventScroll:true});
},
routeButton:function(route,label=routes[route]){return '<button class="project" data-route="'+route+'">'+esc(label)+'</button>';},
answerCapability:function(board){const p=view.S&&view.S.projects.find(p=>p.issues===board);return {manage:true,answer:!!p&&p.alive&&p.complete!==false,reason:'Project status is unavailable or stale; check the board'};},

answerReceipt:function(r){
  const key=r.root+'#'+r.id,request=answerRequests.get(key);
  const previous=answerFeedback.get(key);
  if(!request&&previous?.state!=='unknown')return;
  const item=request||previous;
  answerFeedback.set(key,{board:item.board,op:item.op,state:r.state,reason:r.reason,next:r.next});
  // M3: an accepted answer gets a 6 s overlay toast and joins Answered, which keeps the newest 20
  if(r.state==='accepted'){
    const answered=[...answerFeedback].filter(([,x])=>x.state==='accepted');
    if(answered.length>20)answerFeedback.delete(answered[0][0]);
    view.toast=key;defer(()=>{if(view.toast===key){view.toast=null;present();}},6000);
  }
  if(request){
    cancel(request.timer);answerRequests.delete(key);
    request.resolve({state:r.state,reason:r.reason,next:r.next});
  }else answerCards.get(item.board+'/'+item.op.task)?.ui.receiveReceipt(item.board,r.id,r);
  present();
  defer(fit,0);
},
answerReadback:function(){
  for(const [key,r] of answerRequests){
    const p=view.S.projects.find(p=>p.issues===r.board),t=p&&p.complete!==false&&p.tasks.find(t=>t.n===r.op.task);
    if(r.reconciled||r.op.action||!t||(t.state==='ask'&&t.revision===r.op.revision))continue;
    r.reconciled=true;send({a:'answer',root:p.root,board:r.board,op:r.op});
  }
},
answerSlot:function(t){return t.state==='ask'?'<div data-answer="'+esc(t.key)+'"></div>':'';},
answerNotices:function(board){
  // M3: receipts follow Attention or their project's task rows, never above them; accepted ones collapse into Answered, newest first
  const {line}=execution.module("tray.create.answerNotices.helpers",{line:([key,r],role)=>'<div class="body'+(r.state==='accepted'?'':' alert')+'"'+role+'>#'+r.op.task+' · '+esc(r.state)+(r.reason?': '+esc(r.reason):'')+(r.next?' · '+esc(r.next):'')+' <a href="'+esc(r.board+'/'+r.op.task)+'" data-url="'+esc(r.board+'/'+r.op.task)+'">Board ↗</a>'+(r.state==='unknown'?'<button data-a="answer-retry" data-key="'+esc(key)+'">Check delivery</button>':'')+(answerRequests.has(key)?'':'<button data-a="answer-dismiss" data-key="'+esc(key)+'" aria-label="Dismiss #'+r.op.task+' receipt" title="Dismiss">×</button>')+'</div>'});
  const shown=[...answerFeedback].filter(([,r])=>(!board||r.board===board)&&!view.S.projects.some(p=>p.issues===r.board&&p.tasks.some(t=>t.n===r.op.task&&t.state==='ask')));
  const answered=shown.filter(([,r])=>r.state==='accepted').reverse();
  return shown.filter(([,r])=>r.state!=='accepted').map(x=>line(x,' role="status"')).join('')+(answered.length?'<details class="group answered"><summary>Answered'+counter(answered.length,'muted','Answered')+'</summary>'+answered.map(x=>line(x,'')).join('')+'</details>':'');
},
answerToast:function(){
  const r=view.toast&&answerFeedback.get(view.toast);
  return r?'<div class="toast" role="status">#'+r.op.task+' · answer accepted <a href="'+esc(r.board+'/'+r.op.task)+'" data-url="'+esc(r.board+'/'+r.op.task)+'">Board ↗</a><button data-a="toast-dismiss" aria-label="Dismiss" title="Dismiss">×</button></div>':'';
},
mountAnswers:function(){
  for(const slot of menu.querySelectorAll('[data-answer]')){
    const t=unique(view.S.projects.flatMap(tasks)).find(t=>t.key===slot.dataset.answer);
    if(!t)continue;
    let card=answerCards.get(t.key);
    if(!card){const el=document.createElement('div');card={el,ui:TaskqAnswerCard.mount(el,{capability:answerCapability,send:answerSend})};answerCards.set(t.key,card);}
    if(card.el.parentNode!==slot)slot.append(card.el);
    const p=view.S.projects.find(p=>p.issues+'/'+t.n===t.key);
    card.ui.update([{board:p.issues,row:t}]);
  }
  const keys=new Set(view.S.projects.flatMap(tasks).map(t=>t.key));
  for(const key of answerCards.keys())if(!keys.has(key))answerCards.delete(key);
},
tasks:function(p){return p.tasks.map(t=>({...t,project:p.name,key:p.issues+'/'+t.n,url:t.url||p.issues+'/'+t.n}));},
blockProject:function(p){return {now:view.S.now+Math.round((performance.now()-view.receivedAt)/1000),id:p.root,name:p.name,board:{url:p.issues,kind:p.board_kind},complete:p.complete===true,closed:p.closed,tasks:tasks(p)};},
unique:function(ts){return [...new Map(ts.map(t=>[t.key,t])).values()];},
counts:function(ts){ts=unique(ts);const {n}=execution.module("tray.create.counts",{n:f=>ts.filter(t=>f.includes(t.state)).length});return {work:n(['doing','review']),ask:n(['ask']),ready:n(['ready']),failed:n(['failed'])};},
mode:function(p){return p.focus.length?'run':p.saved?'paused':'watch';},
counter:function(v,kind,label){return '<span class="counter '+(v>0?kind+' filled':'zero')+'" aria-label="'+esc(label+': '+v)+'" title="'+esc(label)+'">'+v+'</span>';},
known:function(ps){return ps.every(p=>p.complete!==false);},
taskRow:function(t,global){const p=view.S.projects.find(p=>p.issues+'/'+t.n===t.key);return '<div data-card="'+esc(t.key)+'" tabindex="-1">'+TaskqProjectBlocks.task(blockProject(p),t,global)+answerSlot(t)+'</div>';},
globalGroup:function(st,ts){ts=ts.filter(t=>t.state===st);return ts.length?'<details class="group"><summary><span class="'+color(st)+'">'+names[st]+'</span>'+counter(ts.length,color(st),names[st])+'</summary>'+ts.map(t=>taskRow(t,true)).join('')+'</details>':'';},
age:function(s){return s<120?s+' s':s<7200?Math.round(s/60)+' min':s<172800?Math.round(s/3600)+' h':Math.round(s/86400)+' d';},
problem:function(p){const e=!p.alive?(p.age==null?'no status.json: service not running':'status.json '+age(p.age)+' old: service not running?'):p.error?'service error: '+p.error:'';return e?'<button class="project alert" data-a="log" data-root="'+esc(p.root)+'" title="'+esc(e)+'"'+(p.log?'':' disabled')+'><span>⚠ '+esc(p.name)+': '+esc(e)+'</span><span class="meta">'+(p.log?'Log':'')+'</span><span></span></button>':'';},
modeText:function(p){return {run:'Admitting: '+p.focus.join(' '),paused:'Paused: running tasks finish, no new ones',watch:'Observation only: focus off'}[mode(p)];},
admission:function(p){
  if(view.pending[p.root]&&!view.pending[p.root].unknown)return '<button class="project" disabled>'+esc(view.pending[p.root].text)+'…</button>';
  if(mode(p)==='run')return '<button class="project" data-a="pause" data-root="'+esc(p.root)+'">Pause admission in '+esc(p.name)+'</button>';
  if(p.saved)return '<button class="project" data-a="resume" data-root="'+esc(p.root)+'">Resume admission in '+esc(p.name)+'</button>';
  if(view.ask!==p.root)return '<button class="project" data-a="pick" data-root="'+esc(p.root)+'">Resume: choose labels…</button>';
  // no saved focus: offer labels, never `all` (M-A12)
  return '<div class="body meta">No saved focus. Admit tasks with one label:</div><div class="options">'+(p.labels.length?p.labels.map(l=>'<button data-a="focus" data-root="'+esc(p.root)+'" data-label="'+esc(l)+'">'+esc(l)+'</button>').join(''):'<span class="meta">No labels on open tasks: use taskq service focus</span>')+'</div>';
},
incident:function(p,top=false){
  const i=p.incident,local=i&&['local','unsupported-auth'].includes(i.state);
  if(top&&(!p.alive||!local))return '';
  const line=!p.alive?'Incidents: unknown ('+(p.age==null?'no status.json':'stale status.json')+')':p.incident_line||'Incidents: unknown (no incident status)';
  const text=p.alive&&i&&(i.url||i.path)?line.replace(' · '+(i.url||i.path),''):line;
  let h='<div class="body '+(top?'incident-attention':'meta')+'" title="'+esc(line)+'">'+esc((top?p.name+': ':'')+text)+'</div>';
  if(p.alive&&local)h+='<div class="options"><button data-a="incident-report" data-root="'+esc(p.root)+'"'+(i.path?'':' disabled')+'>Open report</button><button data-a="incident-manual" data-root="'+esc(p.root)+'">Report manually ↗</button></div>';
  if(p.alive&&i&&i.state==='sent'&&i.url)h+='<a class="project" href="'+esc(i.url)+'" data-url="'+esc(i.url)+'">Open incident ↗</a>';
  return h;
},
incidentNotice:function(p){return p.incident_notice?'<details class="group incident-notice"><summary>Incident reports: notice <span class="meta">'+esc(p.name)+'</span></summary><div class="body">Reports stay local until you choose.</div><div class="body meta">Destination: '+esc(p.incident_repo)+' on GitHub (public). Posting account: unknown here; verify the github.com account used by this project before enabling.</div><div class="body meta">Only sanitized fault codes, fingerprint, report ID, release, revision, contract, board/runtime/OS classes, source site and evidence codes are sent. No project, task, user, session, token or raw error text.</div><div class="body meta">Offline or unsupported login: keep a local JSON report. After confirming the destination and posting account, set incident_reports to true in taskq.json to enable, or false to opt out. TASKQ_INCIDENT_REPORTS=off also disables reporting.</div></details>':'';},
service:function(p){const s=view.S?.service;return '<details class="group service"><summary>⚙ Service <span class="meta">'+esc(p.name)+'</span></summary><div class="body meta" role="status">Host service: '+esc(s?.state==='running'?'running · PID '+s.pid+(s.fresh?' · fresh observation':' · stale observation'):s?.state==='exited'?'recorded process exited; worker outcomes separate':'unknown')+(s?.reason?': '+esc(s.reason):'')+'</div>'+incident(p)+
  (p.log?'<button class="project" data-a="log" data-root="'+esc(p.root)+'">Open log</button>':'<div class="body meta">Log path unknown</div>')+
  '<button class="project" data-a="folder" data-root="'+esc(p.root)+'">Open folder</button>'+
  api.lifecycleControl(p)+
  api.lifecycleControl(p,'stop')+
  api.lifecycleControl(p,'uninstall')+'</details>';},
when:function(t,tz){return new Date(t*1000).toLocaleString([],{timeZone:tz==='UTC'?'UTC':undefined,dateStyle:'short',timeStyle:'short'})+' '+(tz==='UTC'?'UTC':zone);},
local:function(t,tz){const d=new Date(t*1000);return (tz==='UTC'?d.toISOString():new Date(d-d.getTimezoneOffset()*60000).toISOString()).slice(0,16);},
custom:function(){
  const d=view.draft;
  return '<div class="range"><label for="hs">Start</label><input id="hs" type="datetime-local" data-draft="start" value="'+esc(d.start)+'"><label for="he">End</label><input id="he" type="datetime-local" data-draft="end" value="'+esc(d.end)+'"><label for="hz">Time zone</label><select id="hz" data-draft="tz">'+[['local','Local ('+zone+')'],['UTC','UTC']].map(([v,l])=>'<option value="'+v+'"'+(d.tz===v?' selected':'')+'>'+esc(l)+'</option>').join('')+'</select></div>'+
  (d.error?'<div class="body alert" role="alert">'+esc(d.error)+'</div>':'')+'<button class="project" data-a="range">Show [start, end)</button>';},
history:function(){
  // series stack from the bottom in this order on one scale: the peak of the sum (M6)
  const H=view.S.history,n=H.bins.length,w=(H.end-H.start)/n,kinds=['closed','failed','ask','waiting'],tot=kinds.map((_,k)=>H.bins.reduce((a,b)=>a+b[k],0));
  const {sum}=execution.module("tray.create.history.helpers",{sum:b=>b.reduce((a,x)=>a+x,0)});
const {named}=execution.module("tray.create.history.helpers",{named:(b,sep)=>kinds.map((k,i)=>k+' '+b[i]).join(sep)});
const peak=Math.max(1,...H.bins.map(sum));
const none=view.S.projects.filter(p=>p.since==null).map(p=>p.name);
const part=none.length>0||H.since>H.start;
  const note=H.since==null?'No history yet: unknown, not zero.':[H.since>H.start?'Incomplete: history since '+when(H.since,H.tz)+'; earlier is unknown, not zero.':'',none.length?'No history in '+none.join(', ')+'.':''].join(' ').trim();
  // the summary wraps between totals, never inside one, so all four stay readable at every text size (#1083)
  const total=[periods[view.S.period],...(H.since==null?kinds.map(k=>k+' —'):kinds.map((k,i)=>k+' '+tot[i]).concat(part?['incomplete']:[]))].map(t=>t.replace(/ /g,'\u00a0')).join('\u00a0· ');
  return '<details class="group hist"><summary>▥ History <span class="meta">'+total+'</span></summary><div class="options periods">'+Object.entries(periods).map(([k,l])=>'<button data-set="period" data-v="'+k+'" aria-pressed="'+(view.S.period===k||k==='custom'&&view.draft!=null)+'">'+l+'</button>').join('')+'</div>'+
  (view.draft?custom():'')+'<div class="body meta">['+esc(when(H.start,H.tz))+', '+esc(when(H.end,H.tz))+')'+(view.S.now<H.end?' · last bin partial':'')+' · peak '+peak+'</div>'+
  '<div class="bars" role="img" aria-label="'+esc(named(tot,', ')+'; '+n+' equal bins, oldest left'+(note?'; '+note:''))+'">'+H.bins.map((b,i)=>{const t=H.start+i*w,u=H.since==null||t+w<=H.since;
    return '<span'+(u?' class="unknown" title="unknown"':' title="'+esc(when(t,H.tz)+(view.S.now<t+w?' (partial)':'')+': '+named(b,', '))+'"')+'>'+[3,2,1,0].map(k=>'<i class="k'+k+'" style="height:'+b[k]/peak*100+'%"></i>').join('')+'</span>';}).join('')+'</div>'+(note?'<div class="body meta">'+esc(note)+'</div>':'')+'</details>';},
update:function(){
  const up=view.S.projects.find(p=>p.update&&p.update!==p.version);
  if(view.updating&&view.S.projects.some(p=>p.version===view.updating.v))return '<div class="row meta">Updated to '+esc(view.updating.v)+': read from status.json</div>';
  if(view.updating&&view.S.now-view.updating.at<300)return '<button class="project" disabled>Updating to '+esc(view.updating.v)+'…</button>';
  return (view.updating?'<div class="row alert">Update to '+esc(view.updating.v)+' not confirmed: see the log</div>':'')+(up?'<button class="project" data-a="update" data-root="'+esc(up.root)+'" data-v="'+esc(up.update)+'">Update now to '+esc(up.update)+'</button>':'');},
opText:function(o){return [o.project+':',o.op,o.task?'#'+o.task:'',(o.labels||[]).join(' '),o.title?'"'+o.title+'"':'',o.goal?'goal: '+o.goal:'',(o.acceptance||[]).map(x=>'acceptance: '+x).join('; '),o.request?'request: '+o.request:'',o.option?'option '+o.option:'',o.text||''].filter(Boolean).join(' ');},
nextActions:function(t){
  const l=String(t||'').trim().split('\n').pop().trim();if(!l.startsWith('Next: '))return null;
  const xs=l.slice(6).split(' · ').map((s,i)=>{const m=/^(\d+) (\S.*?)( ★)?$/.exec(s.trim());return m&&+m[1]===i+1?{n:m[1],text:s.trim(),rec:!!m[3]}:null;});
  return xs.every(Boolean)&&xs.filter(x=>x.rec).length<2?xs:null;},
ai:function(){
  const A=view.S.ai,r=A.request,an=r&&r.answer||{},st=r?(an.state||'queued'):null,bad=['failed','refuse','apply failed'].includes(st);
  let h='<div class="group"><div class="row meta">✦ Ask AI · '+(A.target?'projects: '+esc(view.S.projects.map(p=>p.name).join(', ')):'no running service: cannot send')+'</div>';
  if(view.aiError)h+='<p role="alert">'+esc(view.aiError)+'</p>';
  const chain=r?(r.chain||[r]):[];
  if(r)h+='<div class="aichain" tabindex="0" aria-label="AI exchange" data-bound="'+esc('chain/'+r.id+'/'+chain.length+'/'+st+'/'+view.S.font)+'">';
  for(const q of chain){
    const a=q.answer||{},state=a.state||'queued',bad=['failed','refuse','apply failed'].includes(state);
    h+='<div class="body"><div class="meta">'+(q.url?'<a href="'+esc(q.url)+'" data-url="'+esc(q.url)+'">Comment on #'+esc(q.task)+' ↗</a>: ':'You: ')+esc(q.text||'')+'</div><div class="'+(bad?'alert':state==='applied'||state==='result'?'work':'ask')+'" role="status">'+esc(aiStates[state]||state)+(a.reason?': '+esc(a.reason):'')+'</div>'+(a.text?'<div>'+esc(a.text).replace(/\n/g,'<br>')+'</div>':'')+'</div>';
  }
  if(r)h+='</div>';
  if(r&&view.S.platform!=='windows')h+='<button data-a="ai-open" data-root="'+esc(r.root)+'" data-id="'+esc(r.id)+'">Open full exchange</button>';
  // a Next button replies with its number, the same token as typing it; the chain then shows `You: <number>`
  const nx=r&&['result','clarify','refuse'].includes(st)&&nextActions(an.text);
  if(nx)h+='<div class="options" role="group" aria-label="Next">'+nx.map(x=>'<button data-a="ai-next" data-v="'+x.n+'"'+(x.rec?' class="rec"':'')+'>'+esc(x.text)+'</button>').join('')+'</div>';
  h+='<div class="askai"><textarea id="ai" rows="2" aria-label="'+(st==='clarify'?'Answer to AI':'Request to AI')+'" placeholder="'+(st==='clarify'?'Answer…':'Ask, or request a change')+'">'+esc(view.aiText)+'</textarea><button data-a="ai-menu" aria-label="History and commands" title="History and commands" aria-expanded="'+view.aiMenu+'">↶</button></div>';
  if(view.aiMenu)h+='<div class="row meta">Recent</div>'+(A.history.length?A.history.map(t=>'<button class="project" data-a="ai-pick" data-v="'+esc(t)+'"><span>'+esc(t)+'</span></button>').join('')+'<button class="project" data-a="ai-clear">Clear history</button>':'<div class="body meta">No sent requests</div>')+
    '<div class="row meta">Commands</div>'+aiCommands.map(t=>'<button class="project" data-a="ai-pick" data-v="'+esc(t)+'"><span>'+esc(t)+'</span></button>').join('');
  h+='<div class="options"><button data-a="ai-send"'+(A.target?'':' disabled')+'>'+(st==='clarify'?'Reply':'Send')+'</button></div>';
  if(!r)return h+'</div>';
  // a setup lists its effects, computed by the service, instead of the op (§ 6)
  const lines=[...an.effects||(an.ops||[]).map(opText),...an.readback||[]];
  if(lines.length)h+='<div class="body meta aiops" tabindex="0" aria-label="Proposed changes" data-bound="'+esc('ops/'+r.id+'/'+an.rev+'/'+st+'/'+view.S.font)+'">'+lines.map(esc).join('<br>')+'</div>';
  if(st==='proposal')h+='<button class="project" data-a="ai-confirm" data-root="'+esc(r.root)+'" data-id="'+esc(r.id)+'" data-rev="'+Number(an.rev)+'"'+(view.aiConfirm===r.id+'/'+Number(an.rev)?' disabled':'')+'>'+(view.S.platform==='windows'?'Confirm: apply ':'Review changes: ')+(an.effects||an.ops).length+' change'+((an.effects||an.ops).length===1?'':'s')+'</button>';
  if(bad)h+='<button class="project" data-a="ai-pick" data-v="'+esc(r.text||'')+'">Edit and send again</button>';
  return h+'</div>';},
addProject:function(){
  const A=view.S.add;
  if(!A)return '<button class="project" data-a="add">Add project…</button>';
  if(A.state==='picking')return '<div class="row meta">Choose a folder…</div><button class="project" data-a="add-cancel">Hide picker result</button>';
  if(A.state==='picking dismissed')return '<div class="body meta">Folder selection is still being processed; its result will be ignored.</div>';
  if(A.state==='installing dismissed')return '<div class="body meta">Installation was submitted; its result is hidden.</div>';
  if(A.state==='pick failed'||A.state==='add failed')return '<div class="body alert" role="alert">'+esc(A.state==='pick failed'?'Folder picker failed':'Add project unavailable')+(A.reason?': '+esc(A.reason):'')+'</div><button class="project" data-a="add-cancel">Done</button>';
  const failed=A.state==='install failed',known=view.S.projects.some(p=>p.root===A.path);
  let h='<div class="body"><div class="meta">Folder: '+esc(A.path)+'</div><div role="status" class="'+(A.kind==='old'||failed?'alert':'')+'">'+(failed?'Install failed':esc(addKinds[A.kind]))+(A.reason?': '+esc(A.reason):'')+'</div></div>';
  if(A.kind==='old')h+='<a class="project" href="'+cutover+'" data-url="'+cutover+'"><span>Repair or cutover: taskq.md § 9 ↗</span><span></span><span></span></a>';
  if(A.kind==='taskq'){
    if(A.state==='installed')h+='<div class="row work" role="status">Installed: read back from this host\'s projects</div>';
    else if(A.state==='installing')h+='<button class="project" disabled>Installing…</button>';
    else if(known)h+='<div class="body meta">Already in this menu</div>';
    else if(view.addConfirm)h+='<div class="body" role="alert">Register '+esc(A.path)+' and install its service? It starts at logon.</div><div class="options"><button data-a="add-install">Confirm</button><button data-a="add-back">Cancel</button></div>';
    else if(!failed)h+='<button class="project" data-a="add-ask">Install service…</button>';
  }
  if(A.kind==='new'||A.kind==='plain')h+=view.S.ai.target?'<button class="project" data-a="add-setup">Set up with AI…</button>':'<div class="body meta">Setup needs one running project service</div>';
  return h+(A.state==='installing'?'<button class="project" data-a="add-cancel">Hide result; install continues</button>':'<button class="project" data-a="add-cancel">'+(A.state==='installed'?'Done':'Cancel')+'</button>');},
bulk:function(run,held){
  const pausing=run.length>0;
const {state}=execution.module("tray.create.bulk.helpers",{state:p=>({run:pausing?'pause':'admitting, unchanged',paused:pausing?'paused, unchanged':'resume',watch:'observation, unchanged'}[mode(p)])});
  if(view.confirming)return '<div class="body" role="alert">'+(pausing?'Pause admission? Running tasks finish.':'Resume admission?')+'<br>'+view.S.projects.map(p=>esc(p.name)+': '+state(p)).join('<br>')+'</div><div class="options"><button data-a="'+(pausing?'pause-all':'resume-all')+'">Confirm</button><button data-a="cancel">Cancel</button></div>';
  return run.length?'<button class="project" data-a="confirm">Pause all…</button>':held.length?'<button class="project" data-a="confirm">Resume all…</button>':'';},
views:function(){return view.S.platform==='macos'?'<div class="options views">'+[['icon','Icon'],['full','Summary']].map(([v,l])=>'<button data-set="view" data-v="'+v+'" aria-pressed="'+(view.S.view===v)+'"><img alt="" src="'+view.S.icons[v]+'">'+l+'</button>').join('')+'</div>':'';},
appearance:function(){
  return '<div class="body meta">Theme follows system appearance.</div>'+views()+'<div class="body">Text size · '+view.S.font+' px</div><div class="options">'+[14,16,18,20].map(n=>'<button data-set="font" data-v="'+n+'" aria-pressed="'+(n===view.S.font)+'">'+n+'</button>').join('')+'</div>';
},
agents:function(){
  const A=view.S.agents;
  return '<div class="body meta">'+esc(view.S.host+' · '+(A?'checked '+age(Math.max(0,view.S.now-A.at))+' ago':'unknown: no service probe yet'))+'</div>'+(A?A.rows.map(r=>'<div class="body">'+esc(r)+'</div>').join(''):'<div class="body meta">Unknown: no status.json has agents</div>');
},
about:function(){
  return '<div class="body">Tray '+esc(view.S.version)+' · '+esc(view.S.host)+'</div>'+view.S.projects.map(p=>'<div class="body">'+esc(p.name)+': '+esc(p.version||'version unknown')+'<span class="meta"> · '+esc(p.age==null?'status unknown':age(p.age)+' old')+'</span></div>').join('')+update();
},
setup:function(){
  const s=view.S.setup;if(!s)return '';
  const link=s.url&&/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(s.url)?' <a href="#" data-url="'+esc(s.url)+'">Open session</a>':'';
  return '<div class="group"><div class="row">Setup · '+esc(s.kind)+link+'</div>'+(s.reason?'<div class="body meta">'+esc(s.reason)+'</div>':'')+'<div class="body">'+esc(s.next||'Setup state unknown; run `taskq service setup`')+'</div></div>';
},
attention:function(){
  const all=unique(view.S.projects.flatMap(tasks));
  return setup()+view.S.projects.map(p=>problem(p)+incident(p,true)+incidentNotice(p)).join('')+globalGroup('ask',all)+globalGroup('failed',all)+answerNotices();
},
projects:function(){
  return TaskqProjectBlocks.aggregate(view.S.projects.map(blockProject))+(view.S.projects.length?view.S.projects.map(p=>TaskqProjectBlocks.summary(blockProject(p))+'<div class="body meta">'+esc(modeText(p))+'</div>').join(''):'<div class="body meta">No projects yet</div>')+addProject();
},
controls:function(){
  const run=view.S.projects.filter(p=>mode(p)==='run'),held=view.S.projects.filter(p=>mode(p)==='paused');
  return workEntry()+(held.length&&run.length?'<div class="row meta">Paused '+held.length+' of '+(held.length+run.length)+': '+esc(held.map(p=>p.name).join(', '))+'</div>':'')+bulk(run,held);
},
runningAgents:function(){
  const boards=new Map(),hosts=new Map();
  for(const p of view.S.projects){
    const key=p.issues||p.root,old=boards.get(key);
    if(!old||p.observed>old.observed)boards.set(key,p);
  }
  let partial=false,doing=0,review=0;
  for(const p of boards.values()){
    const elapsed=Number.isFinite(p.age)&&p.age>=0&&Number.isFinite(p.tasks_age_seconds)&&p.tasks_age_seconds>=0?p.age+p.tasks_age_seconds:null;
    const fresh=p.complete===true&&elapsed!==null&&elapsed<=60&&Number.isFinite(p.observed);
    partial ||= !fresh||!Array.isArray(p.hosts);
    for(const h of Array.isArray(p.hosts)?p.hosts:[]){
      if(!h||typeof h.name!=='string'||!h.name){partial=true;continue;}
      const age=Number.isFinite(h.updated)&&Number.isFinite(p.observed)&&elapsed!==null?p.observed-h.updated+elapsed:null;
      const old=hosts.get(h.name);
      if(!old)hosts.set(h.name,{name:h.name,age,unknown:!fresh||age===null||age<0||age>60,counts:new Map()});
      else {old.unknown ||= !fresh||age===null||age<0||age>60;old.age=old.age===null||age===null?null:Math.max(old.age,age);}
    }
    for(const t of p.tasks||[]){
      doing+=t.state==='doing'?1:0;review+=t.state==='review'?1:0;
      if(t.executions!=null&&!Array.isArray(t.executions))partial=true;
      for(const x of Array.isArray(t.executions)?t.executions:[]){
        if(!x||typeof x.host!=='string'){partial=true;continue;}
        const name=x.host||'Unknown host';
        if(!hosts.has(name))hosts.set(name,{name,age:null,unknown:true,counts:new Map()});
        const h=hosts.get(name),age=Number.isFinite(x.observed)&&Number.isFinite(p.observed)&&elapsed!==null?p.observed-x.observed+elapsed:null;
        if(!fresh||age===null||age<0||age>60||x.state!=='running'||!['worker','reviewer'].includes(x.role)||typeof x.runtime!=='string'||!x.runtime){h.unknown=true;continue;}
        const counts=h.counts.get(x.runtime)||{worker:0,reviewer:0};counts[x.role]++;h.counts.set(x.runtime,counts);
      }
    }
  }
  let h='<section class="group running-agents" aria-label="Running agents"><div class="row">Running agents</div>';
  for(const host of [...hosts.values()].sort((a,b)=>a.name.localeCompare(b.name))){
    const unknown=partial||host.unknown;
    const runtimes=[...host.counts].sort(([a],[b])=>a.localeCompare(b));
    if(!unknown&&runtimes.length){
      h+='<details><summary><strong>'+esc(host.name)+'</strong> · '+runtimes.map(([rt,c])=>esc(rt)+' '+(c.worker+c.reviewer)).join(' · ')+'</summary><div class="body">';
      for(const [rt,c] of runtimes)h+='<div>'+esc(rt)+': '+c.worker+' worker'+(c.worker===1?'':'s')+' · '+c.reviewer+' reviewer'+(c.reviewer===1?'':'s')+'</div>';
      h+='</div></details>';
    }else h+='<div class="body"><strong>'+esc(host.name)+'</strong>'+(unknown?' · unknown'+(host.age!==null&&host.age>=0?' · '+age(host.age)+' old':''):' · idle')+'</div>';
  }
  if(!hosts.size)h+='<div class="body meta">Hosts unknown: no observation</div>';
  const local=[...boards.values()].filter(p=>p.host===view.S.host),latest=local.filter(p=>p.capacity).sort((a,b)=>a.age-b.age)[0];
  const cap=latest?.capacity?.host_runtime_used,capFresh=latest&&Number.isFinite(latest.age)&&latest.age>=0&&latest.age<=60;
  const occupied=cap&&typeof cap==='object'&&!Array.isArray(cap)&&Object.values(cap).every(n=>Number.isInteger(n)&&n>=0);
  h+='<details><summary>Reconcile counts</summary><div class="body meta">Registered project boards · '+(partial?'Tasks unknown':'Tasks: Doing '+doing+' · Review '+review)+'</div>';
  if(capFresh&&occupied){
    h+='<div class="body meta">'+esc(view.S.host)+' · Occupied: '+(Object.entries(cap).map(([rt,n])=>esc(rt)+' '+n).join(' · ')||'0')+' · '+age(latest.age)+' old</div>';
    const reserved=new Map();let known=local.length>0;
    for(const p of local){
      if(!Number.isFinite(p.age)||p.age<0||p.age>60||p.complete!==true||!Number.isFinite(p.tasks_age_seconds)||p.tasks_age_seconds<0||p.tasks_age_seconds+p.age>60||!Array.isArray(p.capacity?.reservations)){known=false;continue;}
      for(const r of p.capacity.reservations){
        if(!r||typeof r.rt!=='string'||!r.rt||!['held','requested'].includes(r.state)){known=false;continue;}
        const running=(p.tasks||[]).some(t=>t.n===r.task&&(Array.isArray(t.executions)?t.executions:[]).some(x=>x&&x.attempt===r.attempt&&x.host===view.S.host&&x.runtime===r.rt&&x.state==='running'&&Number.isFinite(x.observed)&&p.observed-x.observed+p.age+p.tasks_age_seconds>=0&&p.observed-x.observed+p.age+p.tasks_age_seconds<=60));
        if(!running)reserved.set(p.issues+'/'+r.task+'/'+r.attempt,r.rt);
      }
    }
    const counts=new Map();for(const rt of reserved.values())counts.set(rt,(counts.get(rt)||0)+1);
    h+='<div class="body meta">Reservations: '+(known?([...counts].map(([rt,n])=>esc(rt)+' '+n).join(' · ')||'none'):'unknown')+'</div>';
  }else h+='<div class="body meta">'+esc(view.S.host)+' · Capacity unknown</div>';
  return h+'<div class="body meta">Reservations include pending or unconfirmed work. Task states differ from running processes; occupied slots can include other projects. Remote reservations: unknown.</div></details></section>';
},
home:function(){
  let h='<div class="body meta">'+esc(view.S.host)+'</div>'+views()+controls();
  h+='<details><summary>↳ Scope <span class="meta">'+view.S.projects.length+' projects</span></summary><div class="body">'+view.S.projects.map(p=>'<div><strong>'+esc(p.name)+'</strong> <span class="meta">'+esc(modeText(p))+'</span></div><div class="meta">You asked: '+esc(p.request||'not recorded')+'</div>').join('')+'</div></details>';
  h+='<div class="row">Attention'+routeButton('attention','All')+'</div>'+attention();
  h+=api.runningAgents();
  h+=ai();
  h+='<div class="group"><div class="row">Projects'+routeButton('projects','All '+view.S.projects.length)+'</div>'+projects()+'</div>';
  return h+routeButton('cadence');
},
routeBody:function(){
  switch(view.page){
    case 'projects':return projects();
    case 'attention':return attention()||'<div class="body meta">'+(known(view.S.projects)?'No reported questions or failures':'Status incomplete; check Services')+'</div>';
    case 'settings':return ['appearance','cadence','notifications','agents','services','about'].map(r=>routeButton(r)).join('')+workEntry();
    case 'appearance':return appearance();
    case 'cadence':return history();
    case 'notifications':return notifications();
    case 'agents':return agents();
    case 'stop':return api.stopPanel();
    case 'recovery':return api.recoveryPanel();
    case 'services':return '<button class="project" data-a="resources" title="btop in its own terminal window">◔ Resources</button>'+(view.resourceNotice?'<div class="body '+(view.resourceNotice.state==='submitted'?'meta':'alert')+'" role="status">Resources: '+esc(view.resourceNotice.reason)+'</div>':'')+view.S.projects.map(p=>'<div class="body">'+esc(p.name)+' · '+esc(p.host||'host unknown')+'<span class="meta"> · '+esc(p.age==null?'status unknown':age(p.age)+' old')+'</span></div>'+problem(p)+incidentNotice(p)+service(p)).join('');
    case 'about':return about();
    default:return home();
  }
},
resourceGroup:function(p){return {...p.resources,key:p.root,labels:{host:p.cap_host||'this host',project:p.name}};},
resourceCapability:function(g){const p=view.S&&view.S.projects.find(p=>p.root===g.key),e=p&&p.cap_saved&&p.cap_saved.error;return e||!p?{edit:false,reason:'Read-only here',note:e?'Config: '+e:null}:p.resources?.targets?.some(t=>t.supported?.some(s=>s.policy_schema==='taskq.host-capacity/1'))?{edit:TaskqResources.fields(g).length>0,reason:'Editing unavailable here',note:'Host limits affect all registered projects. Project ceilings are read-only here.'}:{edit:false,reason:'Legacy editor unavailable; refresh host status'};},

resourceReceipt:function(m){
  const key=m.root+'#'+m.id,r=resourceRequests.get(key);
  if(!r)return;
  cancel(r.timer);resourceRequests.delete(key);
  const receipt=['saved','rejected','unknown'].includes(m.state)&&(m.state!=='saved'||typeof m.saved_revision==='string'&&/^[a-f0-9]{32}$/.test(m.saved_revision))?{state:m.state,reason:m.reason,saved_revision:m.saved_revision}:{state:'unknown',reason:'Invalid capacity receipt'};
  r.resolve(receipt);
  defer(fit,0);},
capacity:function(p){
  if(!p.resources)return '';
  return '<details class="group"><summary>▤ Capacity <span class="meta">'+esc((p.cap_host||'this host')+' · '+TaskqResources.headline(resourceGroup(p),view.S.now))+'</span></summary><div data-resources="'+esc(p.root)+'"></div></details>';},
workEntry:function(root){return view.S.projects.some(p=>p.resources)?'<button class="project" data-a="work"'+(root?' data-root="'+esc(root)+'"':'')+'>◎ Work mode…</button>':'';},
workPage:function(){const ps=view.S.projects.filter(p=>p.resources),p=ps.find(p=>view.working?.[p.root])||ps[0];return p?.resources.targets?.some(t=>t.supported?.some(s=>s.policy_schema==='taskq.host-capacity/1'))?'<div data-resources="'+esc(p.root)+'"></div>':'<div data-work></div>';},
mountResources:function(){
  const ws=menu.querySelector('[data-work]');
  if(ws){
    if(!view.workCard){const el=document.createElement('div');view.workCard={el,ui:TaskqResources.mountWork(el,{capability:resourceCapability,send:resourceSend,now:()=>view.S.now,host:view.S.host},view.working)};}
    ws.append(view.workCard.el);
    view.workCard.ui.update(view.S.projects.filter(p=>p.resources).map(resourceGroup));
  }
  for(const slot of menu.querySelectorAll('[data-resources]')){
    const p=view.S.projects.find(p=>p.root===slot.dataset.resources);
    if(!p)continue;
    const scope=p.resources.targets?.[0]?.supported?.[0]?.policy_schema;
    let card=resourceCards.get(p.root);
    if(!card||card.scope!==scope){const el=document.createElement('div');card={el,scope,ui:TaskqResources.mount(el,{capability:resourceCapability,send:resourceSend,now:()=>view.S.now})};resourceCards.set(p.root,card);}
    if(card.el.parentNode!==slot)slot.append(card.el);
    card.ui.update(resourceGroup(p));
  }
},
notifications:function(){
  const N=view.S.notify,st=N.status,line={allowed:'Allowed by the OS',blocked:'Blocked by the OS: nothing is posted','not asked':'Not asked yet: the first banner setting asks the OS',asking:'Asked: answer the OS prompt','request failed':'OS permission request failed: '+N.failure,unknown:'OS permission unknown',unsupported:'Not available on this host'}[st]||st;
  return '<div class="body'+(st==='blocked'||st==='request failed'?' alert':' meta')+'" role="status">'+esc(line)+'</div>'+(st==='blocked'?'<button class="project" data-a="notify-os">Open OS setting ↗</button>':'')+
    kinds.map(([k,l])=>'<div class="body">'+l+'</div><div class="options">'+modes.map(([v,m])=>'<button data-a="notify" data-kind="'+k+'" data-v="'+v+'" aria-pressed="'+(N.host[k]===v)+'">'+m+'</button>').join('')+'<button data-a="notify-test" data-kind="'+k+'"'+(st==='allowed'?'':' disabled')+'>Send test</button></div>').join('');
},
notifyGroup:function(p){
  const N=view.S.notify.projects[p.root];if(!N)return '';
  const {name}=execution.module("tray.create.notifyGroup",{name:v=>modes.find(m=>m[0]===v)[1]});
  return '<details class="group"><summary>◉ Notifications <span class="meta">'+kinds.map(([k])=>name(N[k][1])).join(' · ')+'</span></summary>'+kinds.map(([k,l])=>'<div class="body">'+l+' <span class="meta">· '+name(N[k][1])+'</span></div><div class="options">'+[['inherit','Inherit'],...modes].map(([v,m])=>'<button data-a="notify" data-kind="'+k+'" data-root="'+esc(p.root)+'" data-v="'+v+'" aria-pressed="'+((N[k][0]||'inherit')===v)+'">'+m+'</button>').join('')+'</div>').join('')+'</details>';
},
openCard:function(o){
  const p=view.S&&view.S.projects.find(p=>p.root===o.root);if(!p)return;
  if(view.page!==p.root||view.working||view.taskFilter)navigate(p.root);
  const key=p.issues+'/'+o.n;
const {find}=execution.module("tray.create.openCard.helpers",{find:()=>o.n!=null&&(menu.querySelector('[data-answer="'+CSS.escape(key)+'"]')||menu.querySelector('[data-card="'+CSS.escape(key)+'"]'))});
  if(o.n!=null&&!find()){view.noted={root:p.root,n:o.n,key,url:key,title:o.title||'',state:o.kind==='close'?'closed':o.kind};present(true);}
  const el=find();
  if(el){el.scrollIntoView({block:'center'});(el.querySelector('textarea,button,a')||el).focus({preventScroll:true});}
},
notedCard:function(p){return view.noted&&view.noted.root===p.root?'<div class="group"><div class="row meta"><span>Notification</span></div><div data-card="'+esc(view.noted.key)+'" tabindex="-1">'+TaskqProjectBlocks.task(blockProject(p),view.noted)+'</div></div>':'';},
project:function(p){
  const ts=tasks(p);
  return TaskqProjectBlocks.summary(blockProject(p))+'<div class="body meta">'+esc(modeText(p))+'</div>'+problem(p)+incident(p,true)+incidentNotice(p)+notedCard(p)+
  (['ask','failed','doing','review','ready','waiting','later'].map(st=>{const r=ts.filter(t=>t.state===st);return r.length?'<div class="group"><div class="row meta"><span>'+names[st]+'</span>'+counter(r.length,color(st),names[st])+'</div>'+r.map(t=>taskRow(t)).join('')+'</div>':'';}).join('')||'<div class="body muted">No open tasks</div>')+answerNotices(p.issues)+
  '<div class="group">'+(p.issues?'<a class="project" href="'+esc(p.issues)+'" data-url="'+esc(p.issues)+'"><span>Board ↗</span><span></span><span></span></a>':'')+admission(p)+'</div>'+capacity(p)+notifyGroup(p)+workEntry(p.root)+service(p);},
filteredTasks:function(p){
  const ps=p?[p]:view.S.projects,selected=view.taskFilter,states=TaskqProjectBlocks.states;
  const rows=unique(ps.flatMap(tasks)).filter(t=>!selected.length||selected.includes(t.state));
  return '<header class="filter-header">'+(p?TaskqProjectBlocks.summary(blockProject(p)):TaskqProjectBlocks.aggregate(ps.map(blockProject)))+
    '<details class="state-filter"><summary><span>Filter: '+esc(selected.length&&selected.length<states.length?selected.map(s=>states.find(v=>v[0]===s)[1]).join(', '):'All')+'</span><span class="meta" role="status">'+rows.length+' loaded'+(known(ps)?'':' · Partial')+'</span></summary><fieldset><legend>States</legend><div class="filter-options"><label><input type="checkbox" data-filter-all'+(selected.length===states.length?' checked':'')+'>Select all</label><button data-a="filter-clear">Clear all</button></div><div class="filter-options">'+states.map(([state,name])=>'<label><input type="checkbox" data-filter-state="'+state+'"'+(selected.includes(state)?' checked':'')+'>'+esc(name)+'</label>').join('')+'</div></fieldset></details></header>'+
    (known(ps)?'':'<div class="body meta">Partial: refresh the affected project or open its Board.</div>')+
    (!selected.length||selected.includes('closed')?'<div class="body meta">Closed total and task list unknown: open the Board for authoritative closed tasks.</div>':'')+
    (rows.map(t=>taskRow(t,!p)).join('')||'<div class="body meta">'+(known(ps)?'No matching loaded tasks':'Tasks unknown')+' <button data-a="filter-reset">Reset</button></div>');
},
head:function(){const mode=view.S.size_anchor||(view.S.size?'fixed':'top'),label={fixed:'Fixed size',bottom:'Keep bottom edge',top:'Keep top edge'}[mode];return '<div class="row head"><nav aria-label="Menu"><button data-route="home" title="Home" aria-label="Home">⌂</button><button data-route="settings" title="Settings" aria-label="Settings">⚙</button></nav><span class="window-controls">'+(view.S.stop_modes?'<button data-a="stop-review" aria-label="Emergency stop" title="Emergency stop">■</button>':'')+'<button data-a="size-anchor" title="'+label+'" aria-label="Size anchor: '+label+'">'+({fixed:'▣',bottom:'⊥',top:'⊤'}[mode])+'</button><button data-a="top" aria-pressed="'+!!view.S.top+'" title="Keep above other windows" aria-label="Keep above other windows">⧉</button><button data-a="pin" aria-pressed="'+!!view.S.pinned+'" title="'+(view.S.pinned?'Unpin: back to the popover':'Pin: keep open as a movable window')+'" aria-label="'+(view.S.pinned?'Unpin window':'Pin window')+'"><span class="pin-glyph" style="transform:rotate('+(view.S.pinned?'0':'45')+'deg)">📌&#xfe0e;</span></button><button data-a="hide" title="Close window" aria-label="Close window">×</button></span></div>';},
draw:function(navigating=false){
  if(!view.S||view.resizing)return;
  const active=document.activeElement,answerFocus=active&&active.closest('[data-answer]'),scroll=(menu.querySelector('.content')||document.scrollingElement).scrollTop;
  const bounds=new Map([...menu.querySelectorAll('[data-bound]')].map(e=>[e.dataset.bound,e.scrollTop]));
  const focusKey=!navigating&&active&&menu.contains(active)?JSON.stringify(active.dataset):null;
  // a refresh rebuilds the menu: refocus the same control, link or heading, by tag, data and position (U-A4)
  const {same}=execution.module("tray.create.draw.helpers",{same:e=>e.tagName===active.tagName&&JSON.stringify(e.dataset)===focusKey});
const focusAt=focusKey?[...menu.querySelectorAll(active.tagName)].filter(same).indexOf(active):-1;
  const focusedSummary=!navigating&&active?.tagName==='SUMMARY'?[...menu.querySelectorAll('summary')].indexOf(active):-1;
  const caret=answerFocus&&active.tagName==='TEXTAREA'?[active.selectionStart,active.selectionEnd]:null;
  const answerSelector=answerFocus&&(caret?'textarea':active.dataset.choice?'[data-choice="'+active.dataset.choice+'"]':active.dataset.qac?'[data-qac="'+active.dataset.qac+'"]':'button[type=submit]');
  const open=navigating?view.disclosures:[...menu.querySelectorAll('details')].map(d=>d.open),p=view.S.projects.find(p=>p.root===view.page);
  if(!p&&!routes[view.page])view.page='home';
  document.documentElement.style.colorScheme=view.S.theme||'light dark';
  menu.style.setProperty('--size',view.S.font+'px');
  menu.classList.toggle('pinned',!!view.S.pinned);const edge=view.S.attached_edge||'top';if(menu.dataset.edge!==edge)menu.dataset.edge=edge;
  const title=view.working?'Work mode':view.taskFilter?'Tasks':p?p.name:routes[view.page];
  const html=head()+'<div class="content"><div class="content-body">'+actionNotices()+(view.page!=='home'||view.working||view.taskFilter?'<button data-a="back">‹ Back</button>':'')+'<h1 tabindex="-1">'+esc(title)+'</h1>'+(view.S.settings_error?'<div class="body alert" role="alert">Settings unknown: '+esc(view.S.settings_error)+'; defaults shown</div>':'')+(view.working?workPage():view.taskFilter?filteredTasks(p):p?project(p):routeBody())+'</div></div><div class="grip" data-resize="" role="button" aria-label="Resize window with arrow keys" tabindex="0"></div>'+answerToast();
  // #1430: an unchanged refresh leaves the menu alone; a rebuild replaced the textarea being typed in and its edits
  // landed on the detached node. Mounted cards still take the new rows and repaint only what changed.
  if(!navigating&&html===view.html){mountAnswers();mountResources();return;}
  menu.innerHTML=view.html=html;
  mountAnswers();mountResources();
  if(view.taskFilter){
    for(const button of menu.querySelectorAll('.tqp-counts [data-state]'))button.setAttribute('aria-pressed',String(view.taskFilter.includes(button.dataset.state)));
    const all=menu.querySelector('[data-filter-all]');if(all)all.indeterminate=view.taskFilter.length>0&&view.taskFilter.length<TaskqProjectBlocks.states.length;
  }
  menu.querySelectorAll('details').forEach((d,i)=>d.open=open[i]??(!d.classList.contains('answered')&&['cadence','agents','services','attention'].includes(view.page)));
  if(view.S.page!==undefined)menu.querySelectorAll('.hist,.agents,.service,.incident-notice').forEach(d=>d.open=!(p&&d.classList.contains('service')));
  const restored=answerFocus&&menu.querySelector('[data-answer="'+CSS.escape(answerFocus.dataset.answer)+'"] '+answerSelector);
  if(restored&&!navigating){restored.focus({preventScroll:true});if(caret)restored.setSelectionRange(...caret);}
  else if(!navigating){const target=focusedSummary>=0?menu.querySelectorAll('summary')[focusedSummary]:focusKey&&active.tagName==='H1'?menu.querySelector('h1'):focusKey&&focusKey!=='{}'?[...menu.querySelectorAll(active.tagName)].filter(same)[focusAt]:null;target?.focus({preventScroll:true});}
  (menu.querySelector('.content')||document.scrollingElement).scrollTop=scroll;
  fit();
  // after fit() sets the width: a new exchange or text size opens at the newest entry; a tick keeps the reader's place;
  // a block that drops a cut line asks for the smaller height
  const bound=[...menu.querySelectorAll('[data-bound]')];
  bound.forEach(c=>{c.scrollTop=bounds.has(c.dataset.bound)?bounds.get(c.dataset.bound):c.classList.contains('aichain')?c.lastElementChild.offsetTop:0;wholeLines(c);});
  if(bound.some(c=>c.style.maxHeight))fit();},
wholeLines:function(c){
  if(c.scrollHeight<=c.clientHeight)return;
  const top=c.getBoundingClientRect().top+c.clientTop;
const {lines}=execution.module("tray.create.wholeLines.helpers",{lines:()=>{const r=document.createRange(),w=document.createTreeWalker(c,NodeFilter.SHOW_TEXT),a=[];
    for(let n;(n=w.nextNode());){r.selectNodeContents(n);a.push(...r.getClientRects());}return a;}});
  const {shrink}=execution.module("tray.create.wholeLines.helpers",{shrink:d=>c.style.maxHeight=c.offsetHeight-d+'px'});
const {cut}=execution.module("tray.create.wholeLines.helpers",{cut:edge=>Math.min(...lines().filter(l=>l.top<edge&&l.bottom>edge).map(l=>l.top))});
  const head=cut(top);
  if(head<top&&c.scrollTop+c.clientHeight<c.scrollHeight-1)c.scrollTop-=top-head;
  else if(head<top){const d=Math.min(...lines().filter(l=>l.top>=top).map(l=>l.top))-top;if(d<c.clientHeight){shrink(d);c.scrollTop+=d;}}
  const bottom=top+c.clientHeight,tail=cut(bottom);if(tail>top&&tail<bottom)shrink(bottom-tail);},
settle:function(c){if(!c.isConnected)return;const s=c.scrollTop;c.style.maxHeight='';c.scrollTop=s;wholeLines(c);fit();},
refresh:function(now){if(view.pressedAt!==null&&now-view.pressedAt<5000){view.stale=true;return;}present();},
release:function(){view.pressedAt=null;if(view.stale){view.stale=false;defer(present);}},
receive:function(s){
  if(s.diagnose){const d=s.diagnose,remaining=d.until_ms-Date.now();
    if(d.id!==view.diagnosticId&&remaining>0){execution.diagnose(d.selector,Math.min(60000,remaining),d.limit);view.diagnosticId=d.id;}
    return;
  }
  if(s.aiSubmitted){view.aiText='';view.aiSending=false;view.aiError='';present();return;}
  if(s.aiError){view.aiSending=false;view.aiError=s.aiError;present();return;}
  if(s.action_error){actionReceipt(s.action_error);return;}
  if(s.focus_receipt){focusReceipt(s.focus_receipt);return;}
  if(s.answerReceipt){answerReceipt(s.answerReceipt);return;}
  if(s.resources_receipt){view.resourceNotice=s.resources_receipt;present();return;}
  if(s.capmsg){if(api.recoveryReceipt(s.capmsg))return;if(view.hostCommand?.id===s.capmsg.id)api.lifecycleReceipt(s.capmsg);else resourceReceipt(s.capmsg);return;}
  if(s.open){openCard(s.open);return;}
  if(view.S?.size_anchor!==s.size_anchor)view.manualSize=null;
  view.S=s;view.receivedAt=performance.now();focusReadback();answerReadback();api.lifecycleReadback();
  if(view.aiText===null)view.aiText=s.ai.draft||'';
  // snapshot mode names the page
  if(s.page!==undefined&&s.page!==view.page){view.page=s.page;view.taskFilter=null;view.working=null;view.workCard=null;trail.length=0;view.disclosures=[];present(true);return;}
  // a tick does not take the caret out of the range fields
  if(document.activeElement&&(document.activeElement.dataset.draft||document.activeElement.dataset.rscField||document.activeElement.dataset.recoveryEvidence||document.activeElement.id==='ai'))return;
  refresh(performance.now());
},
measure:function(name){
  const e=document.documentElement,m=menu,x=Math.max(e.scrollWidth-e.clientWidth,m.scrollWidth-m.clientWidth);
  if(x>0)send({a:'overflow',shot:name,x});
  const seen=new Set();for(const b of m.querySelectorAll('button')){
    const text=b.textContent.trim().slice(0,24);
    if(b.offsetParent&&b.scrollWidth>b.clientWidth+1&&!seen.has(text)){seen.add(text);send({a:'overflow',shot:name+' button '+text,x:b.scrollWidth-b.clientWidth});}
  }
},
mount:function(){
window.taskq={settle,receive,measure,model:{unique,counts,mode}};
ui.listen(menu,'scroll',e=>{const c=e.target;if(!c.dataset?.bound)return;cancel(c.settling);c.settling=defer(()=>settle(c),150);},true);
// a refresh between press and release rebuilds the pressed control and its click is lost (U-A4):
// the refresh waits for the release, at most 5 s, then runs after the click


ui.listen(menu,'pointerdown',()=>{view.pressedAt=performance.now();});
for(const event of ['pointerup','pointercancel'])ui.listen(window,event,release,true);
ui.listen(window,'blur',release);
// Press end.

ui.listen(menu,'change',e=>{
  if(!view.taskFilter)return;
  if(e.target.matches('[data-filter-all]'))view.taskFilter=e.target.checked?TaskqProjectBlocks.states.map(s=>s[0]):[];
  else if(e.target.matches('[data-filter-state]')){const state=e.target.dataset.filterState;view.taskFilter=e.target.checked?[...view.taskFilter,state]:view.taskFilter.filter(s=>s!==state);}
  else return;
  present();
});
ui.listen(menu,'click',e=>{
  const cardLink=e.target.closest('.qac a');if(cardLink){e.preventDefault();send({a:'open',url:cardLink.getAttribute('href')});return;}
  if(e.target.closest('.qac,.rsc'))return;
  if(TaskqProjectBlocks.handle(e,{open:url=>send({a:'open',url}),project:id=>navigate(id),tasks:(id,state)=>{if(view.taskFilter&&view.page===(id||'home')){view.taskFilter=[state];present();}else navigate(id||'home',state);}}))return;
  const a=e.target.closest('a[data-url]');if(a){e.preventDefault();send({a:'open',url:a.dataset.url});return;}
  const b=e.target.closest('button');if(!b||b.disabled)return;const d=b.dataset;
  if(d.a==='filter-clear'||d.a==='filter-reset'){view.taskFilter=d.a==='filter-clear'?[]:TaskqProjectBlocks.states.map(s=>s[0]);present();return;}
  if(d.route){if(d.route!==view.page||view.working||view.taskFilter)navigate(d.route);return;}
  if(d.a==='answer-dismiss'){answerFeedback.delete(d.key);if(view.toast===d.key)view.toast=null;present();return;}
  if(d.a==='toast-dismiss'){view.toast=null;present();return;}
  if(d.a==='answer-retry'){const r=answerFeedback.get(d.key);if(r)ui.effect('answer.reconcile',()=>answerSend(r.board,r.op),()=>{});present();return;}
  if(d.a==='notify'||d.a==='notify-test'){send({a:d.a,kind:d.kind,value:d.v,root:d.root});return;}
  if(d.a==='work'){navigate(view.page,null,d.root?{[d.root]:true}:{});return;}
  if(d.a==='back'){back();return;}
  if(d.set==='period'&&d.v==='custom'){const H=view.S.history;view.draft={start:local(H.start,H.tz),end:local(H.end,H.tz),tz:H.tz,error:''};present();return;}
  if(d.set){view.S[d.set]=d.set==='font'?Number(d.v):d.v;if(d.set==='period')view.draft=null;send({a:'set',[d.set]:view.S[d.set]});present();return;}
  if(d.a==='range'){
    const {t}=execution.module("tray.create.mount.helpers",{t:v=>v?Date.parse(view.draft.tz==='UTC'?v+'Z':v)/1000:NaN});
const a=t(view.draft.start);
const z=t(view.draft.end);
    view.draft.error=!(a<z)?'Refused: start must be before end.':'';
    if(!view.draft.error){view.S.period='custom';send({a:'set',period:'custom',start:a,end:z,tz:view.draft.tz});view.draft=null;}
    present();return;}
  if(d.a==='ai-menu'){view.aiMenu=!view.aiMenu;present();return;}
  // picking fills the input, never sends (M-A13)
  if(d.a==='ai-pick'){view.aiText=d.v;view.aiMenu=false;send({a:'ai-draft',text:view.aiText});present();const t=document.getElementById('ai');t.focus();t.setSelectionRange(t.value.length,t.value.length);return;}
  if(d.a==='ai-next'){send({a:'ai-send',text:d.v,root:view.S.ai.request.root,after:view.S.ai.request.id});present();return;}
  if(d.a==='ai-send'){const r=view.S.ai.request,an=r&&r.answer||{};if(!view.aiText.trim()||view.aiSending)return;view.aiSending=true;send({a:'ai-send',text:view.aiText,root:an.state==='clarify'?r.root:view.S.ai.target,after:an.state==='clarify'?r.id:''});if(view.S.platform==='windows'){view.aiText='';view.aiSending=false;}present();return;}
  if(d.a==='ai-open'){send({a:'ai-open',root:d.root,id:d.id});return;}
  if(d.a==='ai-confirm'){if(view.S.platform==='windows')view.aiConfirm=d.id+'/'+d.rev;send({a:view.S.platform==='windows'?'ai-confirm':'ai-open',root:d.root,id:d.id,rev:Number(d.rev)});present();return;}
  if(d.a==='ai-clear'){send({a:'ai-clear'});return;}
  if(d.a==='add-ask'||d.a==='add-back'){view.addConfirm=d.a==='add-ask';present();return;}
  if(d.a==='add-install'||d.a==='add-cancel')view.addConfirm=false;
  // the pressed button is replaced: focus Cancel in the question, or the command again after it (U-A8)
  if(d.a==='confirm'||d.a==='cancel'){view.confirming=d.a==='confirm';present();menu.querySelector(view.confirming?'[data-a="cancel"]':'[data-a="confirm"]')?.focus();return;}
  if(d.a==='update'){view.updating={v:d.v,at:view.S.now};}
  if(d.a==='pick'){view.ask=d.root;present();return;}
  if(d.a==='pause-all'||d.a==='resume-all'){view.confirming=null;for(const p of view.S.projects.filter(p=>mode(p)===(d.a==='pause-all'?'run':'paused')))focusStart(d.a.slice(0,-4),p.root);present();menu.querySelector('[data-a="confirm"]')?.focus();return;}
  if(d.a==='recovery-pending'){const item=view.recovery[d.root],request=item?.review?.pending?.[Number(d.index)];if(request){item.request=request;api.recoveryStart(d.root,'recovery-reconcile');}return;}
  if(['recovery-read','recovery-apply','recovery-reconcile'].includes(d.a)){api.recoveryStart(d.root,d.a);return;}
  if(d.a==='stop-review'){view.page='stop';present();return;}
  if(d.a==='stop-full'){api.lifecycleStart(d.root,'stop','full');return;}
  if(d.a==='restart'||d.a==='stop'||d.a==='uninstall'){api.lifecycleStart(d.root,d.a);return;}
  if(d.a==='pause'||d.a==='resume'||d.a==='focus'){focusStart(d.a,d.root,d.label);view.ask=null;present();return;}
  if(d.a){actionErrors.delete(d.root);send({a:d.a,root:d.root,label:d.label});}present();});
ui.listen(menu,'input',e=>{const d=e.target.dataset;if(d.recoveryTask||d.recoveryEvidence){const item=view.recovery[d.root];if(!item)return;const key=d.recoveryTask||d.recoveryEvidence,draft=item.drafts[key]||(item.drafts[key]={});if(d.recoveryTask)draft.selected=e.target.checked;else draft.evidence=e.target.value;item.notice='';return;}if(e.target.id==='ai'){view.aiText=e.target.value;send({a:'ai-draft',text:view.aiText});return;}const k=e.target.dataset.draft;if(k&&view.draft)view.draft[k]=e.target.value;});
ui.listen(menu,'focusout',e=>{const {f}=execution.module("tray.create.mount.focusout",{f:x=>x&&(x.id==='ai'||x.dataset&&(x.dataset.draft||x.dataset.rscField||x.dataset.recoveryEvidence))});if(f(e.target))defer(()=>{if(!f(document.activeElement))present();});});
ui.listen(menu,'mousedown',e=>{if(view.S&&e.button===0&&e.target.closest('.head')&&!e.target.closest('button'))send({a:'drag'});});
ui.listen(menu,'pointerdown',e=>{if(e.button!==0||!e.target.closest('.grip'))return;view.resizing={x:e.screenX,y:e.screenY,w:window.innerWidth,h:window.innerHeight};e.target.setPointerCapture(e.pointerId);e.preventDefault();});
ui.listen(menu,'pointermove',e=>{if(!view.resizing)return;view.manualSize=[Math.max(240,view.resizing.w+e.screenX-view.resizing.x),Math.max(120,view.resizing.h+e.screenY-view.resizing.y)];menu.style.width='100%';send({a:'resize',d:window.devicePixelRatio,w:view.manualSize[0],h:view.manualSize[1]});});
for(const event of ['pointerup','pointercancel','lostpointercapture'])ui.listen(menu,event,()=>{if(view.resizing)send({a:'resize-end'});view.resizing=null;});
ui.listen(menu,'keydown',e=>{if(!e.target.closest('.grip')||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();view.manualSize=[Math.max(240,window.innerWidth+(e.key==='ArrowRight'?10:e.key==='ArrowLeft'?-10:0)),Math.max(120,window.innerHeight+(e.key==='ArrowDown'?10:e.key==='ArrowUp'?-10:0))];send({a:'resize',d:window.devicePixelRatio,w:view.manualSize[0],h:view.manualSize[1]});send({a:'resize-end'});});
ui.listen(document,'keydown',e=>{if(e.key==='Escape')send({a:'hide'});});
ui.listen(menu,'toggle',()=>fit(),true);
// § 4 Tray: row ages tick in place while the menu is open; a redraw would move the reading position
const {ages}=execution.module("tray.create.mount.ages",{ages:()=>{if(view.S){const now=view.S.now+Math.round((performance.now()-view.receivedAt)/1000);for(const e of menu.querySelectorAll('[data-since]'))e.textContent=TaskqProjectBlocks.age(now-Number(e.dataset.since));}defer(ages,30000);}});
defer(ages,30000);
ui.effect('ui.fonts',()=>document.fonts.ready,()=>{if(view.S)fit();});
send({a:'ready'});
}
});
const {gutter,fit,actionReceipt,actionNotices,focusStart,focusReceipt,focusReadback,remember,navigate,back,routeButton,answerCapability,answerReceipt,answerReadback,answerSlot,answerNotices,answerToast,mountAnswers,tasks,blockProject,unique,counts,mode,counter,known,taskRow,globalGroup,age,problem,modeText,admission,incident,incidentNotice,service,when,local,custom,history,update,opText,nextActions,ai,addProject,bulk,views,appearance,agents,about,setup,attention,projects,controls,home,routeBody,resourceGroup,resourceCapability,resourceReceipt,capacity,workEntry,workPage,mountResources,notifications,notifyGroup,openCard,notedCard,project,filteredTasks,head,draw,wholeLines,settle,refresh,release,receive,measure,mount}=api;

function answerSend(board,op){
  const p=view.S&&view.S.projects.find(p=>p.issues===board);
  if(!p)return Promise.resolve({state:'rejected',reason:'Project is no longer registered'});
  const key=p.root+'#'+op.id;
  if(answerRequests.has(key))return answerRequests.get(key).promise;
  let resolve;
  const promise=new Promise(r=>resolve=r);
  const timer=defer(()=>{answerReceipt({root:p.root,id:op.id,state:'unknown',reason:'No bridge receipt received',next:'Check delivery with the same operation'});},30000);
  answerRequests.set(key,{promise,resolve,timer,board,op});
  answerFeedback.set(key,{board,op,state:'sending',reason:''});
  send({a:'answer',root:p.root,board,op});
  return promise;
}
function resourceSend(g,op){
  const capability=resourceCapability(g);
  if(!capability.edit)return Promise.resolve({state:'rejected',reason:capability.reason});
  if(!Array.isArray(op.ops)||!op.ops.length||op.ops.some(o=>o.policy_schema!=='taskq.host-capacity/1'||o.target?.project_id!==null))return Promise.resolve({state:'rejected',reason:'Unsupported host capacity scope'});
  const key=g.key+'#'+op.id;
  // A lost receipt retries the reviewed expectation, never a newer config; keep one operation per project.
  if(resourcePayloads.get(g.key)?.id!==op.id)resourcePayloads.set(g.key,JSON.parse(JSON.stringify({a:'capacity',root:g.key,id:op.id,ops:op.ops,issued_at:view.S.now,expires_at:view.S.now+300})));
  return new Promise(resolve=>{
    const prior=resourceRequests.get(key);if(prior)cancel(prior.timer);
    resourceRequests.set(key,{resolve,timer:defer(()=>{resourceRequests.delete(key);resolve({state:'unknown',reason:'no answer from the tray within 20 s; check the limits shown, then retry'});},20000)});
    send(resourcePayloads.get(g.key));});}
const window=ports.window||globalThis,document=ports.document||window.document,menu=ports.menu||document?.querySelector('.menu');
const {defer,cancel}=ports.timers||ui;
const TaskqAnswerCard=require('./answer-card.js'),TaskqResources=require('./resource-controls.js'),TaskqProjectBlocks=require('./project-blocks.js');
const send=ports.send||ui.send;
const performance=window.performance||globalThis.performance,crypto=window.crypto||globalThis.crypto,CSS=window.CSS||globalThis.CSS;
const present=execution.action('ui.render',(...args)=>(ports.render||draw)(...args));
const names={doing:'In work',review:'Review',ask:'Waiting for answer',failed:'Failed',ready:'Ready',waiting:'Dependencies',later:'Later'};
const {color}=execution.module("tray.create",{color:st=>({doing:'work',review:'work',ask:'ask',failed:'failed',ready:'ready'}[st]||'muted')});
const periods={'5m':'5 min','1h':'Hour','1d':'Day','1w':'7 days','1mo':'30 days',custom:'Custom'};
const zone=Intl.DateTimeFormat().resolvedOptions().timeZone;
const {esc}=execution.module("tray.create",{esc:s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]))});

// measure at the target width: after a font change the window still has the old width, so text wraps differently
// g: this renderer's scrollbar width; a menu taller than its window scrolls and the window adds it (#1073)



const actionErrors=new Map();





// Navigation is view state only: every route consumes the same pushed snapshot.
const routes={recovery:'Reviewed recovery',stop:'Stop and hold',home:'Home',projects:'Projects',attention:'Attention',settings:'Settings',appearance:'Appearance',cadence:'Activity history',notifications:'Notifications',agents:'Agents',services:'Services',about:'About'};
const trail=[];




// Task answers: the native bridge owns credentials and canonical dispatch/readback.
const answerRequests=new Map(),answerCards=new Map(),answerFeedback=new Map();



// A status refresh that reads the question past an answer op's revision ends Sending: reconcile once with the same op,
// so the CLI returns its first receipt (accepted) or names another writer (stale); the refresh itself never accepts.
// ponytail: fallback actions (#1021) keep Check delivery; their revision fence refuses another revision.




// one task per board and issue: `issues` names provider, host and repo (M2)






// an incomplete or stale snapshot has unknown counts, never zero (§ 4)




// service error and stale status.json are not failed tasks (M-A10)






// bins, range and since come from Rust (the icon draws the same); unknown is never drawn or counted as zero (M6)




// `Update now` only for a newer version in status.json; success only once a status.json reads it back (M9)

// AI requests (menu.md M7): one input, `↶` history and commands, the newest request with its answer
const aiStates={queued:'Queued',running:'Working…',clarify:'Needs your answer',proposal:'Needs your confirmation',applying:'Applying…',applied:'Applied: read back',result:'Done',refuse:'Refused',failed:'AI request not done','apply failed':'Not applied'};
const aiCommands=['What needs my answer?','What is running now?','Why is nothing starting?','Pause all projects'];
// every value the command gets is shown before confirm

// R6 item 7 `Next: 1 <action> ★ · 2 <action>` as the last line: numbered 1..n in order, at most one ★; else null (raw text only)


// Add project (menu.md M10): the folder comes from the OS picker; four cases
const addKinds={new:'New project: no taskq.json',plain:'Not a repository: files stay',taskq:'taskq project: selftest passes',old:'Old taskq project: selftest fails'};
const cutover='https://github.com/taskqorg/taskq/blob/main/taskq.md#9-config-and-cutover';

// the command over all projects names each one and its effect before acting (M5, M-A4)










// Capacity (taskq.md § 4): the shared resource controls (docs/design/shared-resource-controls.md) over the tray's
// taskq.resources/1 projection; this adapter only maps an operation onto `service capacity` and its receipt
const resourceCards=new Map(),resourceRequests=new Map(),resourcePayloads=new Map();






// Work mode (menu M12): the shared editor over this host's projects; only the explicitly selected ones change



// Notifications (menu.md M14): host mode per kind, the OS permission, and per project overrides
const modes=[['off','Off'],['silent','Silent'],['banner','Banner'],['sound','Banner with sound']],kinds=[['ask','Questions'],['close','Closed'],['failed','Failed'],['gone','Attempt gone']];


// a notification click: the project page scrolled to the card (for `ask`, its answer card); a task no longer in the
// open lists (closed): its card from the notification, kept on top of the page until navigation




// pin (menu.md M13): the same menu as a movable window; the header is its drag handle


// a bounded block shows whole lines: a line cut at the top scrolls into view, or at the scroll end (the newest text)
// moves out above; the bottom edge moves up to the first line it cuts

// a reader's scroll leaves cut lines: once it settles, the block takes its CSS bound again and realigns on whole lines






return {...api,answerSend,resourceSend};
}
});


module.exports={create,createState};
