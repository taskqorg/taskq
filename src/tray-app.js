'use strict';
const ui=require('./ui-runtime.js');
const execution=ui.execution;
const shell=require('./app-shell.js');
const {h,Fragment}=require('preact');

const {createState,create}=execution.module("tray",{
createState:function(snapshot=null){return {manualSize:null,aiRoot:null,addConfirm:false,resourceNotice:null,persistenceError:null,toast:null,receivedAt:0,S:snapshot,page:'home',taskFilter:null,working:null,workCard:null,ask:null,noted:null,pending:{},hostCommand:null,recovery:{},confirming:null,updating:null,draft:null,disclosures:{},pressedAt:null,stale:false,resizing:null};},
create:function(view,ports){
const api=execution.module("tray.create",{

operationLauncher:function(operation,root,label){return h('button',{class:'project','data-a':'workspace','data-page':'operations','data-operation':operation,'data-root':root||null},label);},
operationTarget:function(d){return ({work:'resources',confirm:'admission','pause-all':'admission','resume-all':'admission',pause:'admission',resume:'admission',focus:'admission',pick:'admission',add:'setup','add-ask':'setup','add-install':'setup','add-setup':'setup','add-cancel':'setup','stop-review':'stop',stop:'stop','stop-full':'stop',restart:'service',uninstall:'service',update:'update','recovery-read':'recovery','recovery-apply':'recovery','recovery-reconcile':'recovery','recovery-pending':'recovery'})[d.a];},
operationBody:function(context){
  const c=context||{},ps=view.S.projects,p=ps.find(p=>p.root===c.root),host=view.S.host||'unknown';
  if(c.root&&!p)return f(actionNotices(),h('p',{role:'status'},'Project unavailable; no operation can be applied. Return to Messages or Refresh.'));
  const heading=h('p',{class:'meta'},'Host '+host+' · '+(p?p.name+' · '+p.root:'all registered projects: '+ps.map(p=>p.name+' ('+p.root+')').join(', ')));
  let body;
  switch(c.operation){
   case 'service':body=[h('button',{class:'project','data-a':'resources'},'Open resource monitor'),view.resourceNotice&&h('p',{role:'status'},view.resourceNotice.reason),p?[service(p),capacity(p),workEntry(p.root)]:ps.map(service)];break;
   case 'resources':{const target=p||ps.find(p=>p.resources);body=target?[h('p',null,'Host limits affect every registered project; project ceilings remain read-only.'),workPageFor(target)]:h('p',{role:'status'},'Capacity unavailable: no supported host projection.');break;}
   case 'stop':body=view.S.stop_modes?api.stopPanel():h('p',{role:'status'},'Emergency stop unavailable on this host. Normal stop is under Services.');break;
   case 'recovery':body=view.S.stop_modes?api.recoveryPanel():h('p',{role:'status'},'Reviewed recovery unavailable on this host.');break;
   case 'admission':{const selected=p?[p]:ps;body=[h('p',null,'Pause holds new admission; current tasks finish. Resume restores saved focus and cannot clear an emergency hold.'),selected.map(p=>h('section',{key:p.root},h('p',null,p.name+' · '+modeText(p)),admission(p))),!p&&bulk(selected.filter(p=>mode(p)==='run'),selected.filter(p=>mode(p)==='paused'))];break;}
   case 'update':body=[h('p',null,'Request the advertised update on this host. Running work is preserved; success requires the version read back from service status.'),update()];break;
   case 'setup':body=[h('p',null,'Choose a folder using the native picker. Opening or leaving this review registers nothing. Setup must confirm account, repository, visibility and exact external effects under § 6 before applying them.'),addProject(),setup()];break;
   default:body=h('p',{role:'status'},'Operation unavailable; no effect was submitted.');
  }
  return f(heading,actionNotices(),body);
},
operationAction:function(d){
  if(d.a==='add-ask'||d.a==='add-back'){view.addConfirm=d.a==='add-ask';present();return true;}
  if(d.a==='add-install'||d.a==='add-cancel')view.addConfirm=false;
  // the pressed button is replaced: focus Cancel in the question, or the command again after it (U-A8)
  if(d.a==='confirm'||d.a==='cancel'){view.confirming=d.a==='confirm';present();menu.querySelector(view.confirming?'[data-a="cancel"]':'[data-a="confirm"]')?.focus();return true;}
  if(d.a==='update'){if(view.updating)return true;view.updating={v:d.v,root:d.root,at:view.S.now};send({a:'update',root:d.root});present();return true;}
  if(d.a==='pause-all'||d.a==='resume-all'){view.confirming=null;for(const p of view.S.projects.filter(p=>mode(p)===(d.a==='pause-all'?'run':'paused')))focusStart(d.a.slice(0,-4),p.root);present();menu.querySelector('[data-a="confirm"]')?.focus();return true;}
  if(d.a==='recovery-pending'){const item=view.recovery[d.root],request=item?.review?.pending?.[Number(d.index)];if(request){item.request=request;api.recoveryStart(d.root,'recovery-reconcile');}return true;}
  if(['recovery-read','recovery-apply','recovery-reconcile'].includes(d.a)){api.recoveryStart(d.root,d.a);return true;}
  if(d.a==='stop-full'){api.lifecycleStart(d.root,'stop','full');return true;}
  if(d.a==='restart'||d.a==='stop'||d.a==='uninstall'){api.lifecycleStart(d.root,d.a);return true;}
  if(d.a==='pause'||d.a==='resume'||d.a==='focus'){focusStart(d.a,d.root,d.label);view.ask=null;present();return true;}

  if(d.a==='pick'){view.ask=d.root;present();return true;}
  if(['add','add-install','add-cancel','add-setup','resources','log','folder'].includes(d.a)){if(['add-install','add-setup'].includes(d.a)){const A=view.S.add;if(!A||view.setupPending===A.generation)return true;view.setupPending=A.generation;send({a:d.a,generation:A.generation});}else send({a:d.a,root:d.root});present();return true;}
  return false;
},
operationInput:function(e){const d=e.target.dataset;if(d.recoveryTask||d.recoveryEvidence){const item=view.recovery[d.root];if(!item)return;const key=d.recoveryTask||d.recoveryEvidence,draft=item.drafts[key]||(item.drafts[key]={});if(d.recoveryTask)draft.selected=e.target.checked;else draft.evidence=e.target.value;item.notice='';}},
gutter:function(){if(ports.measureScrollbar)return ports.measureScrollbar();const d=document.createElement('div');d.style.cssText='position:absolute;top:-99px;width:60px;height:60px;overflow:scroll;scrollbar-width:thin';document.body.append(d);const g=d.offsetWidth-d.clientWidth;d.remove();return Math.ceil(g);},
// #1523: the window's own layout (header, chips, summary, list, footer, any new panel) with the scroll area at full content height.
natural:function(){const c=menu.querySelector('.content'),b=c&&c.querySelector('.content-body'),all=menu.getBoundingClientRect().height;if(!b)return Math.ceil(all);const cs=getComputedStyle(c),edges=['paddingTop','paddingBottom','borderTopWidth','borderBottomWidth'].reduce((n,k)=>n+(parseFloat(cs[k])||0),0);return Math.ceil(all-c.getBoundingClientRect().height+b.getBoundingClientRect().height+edges);},
fit:function(){if(ports.workspace)return;const size=view.manualSize||view.S.size,fixed=(view.S.size_anchor||(size?'fixed':'top'))==='fixed',w=view.page==='recovery'?Math.min(720,window.screen?.availWidth||720):size?size[0]:view.S.font>=18?360:320;document.body.style.width='100%';menu.style.width=w+'px';send({a:'size',d:window.devicePixelRatio,g:fixed&&size?0:gutter(),w,h:fixed&&size?size[1]:natural()});menu.style.width='100%';},
actionReceipt:function(r){if(!view.S?.projects.some(p=>p.root===r.root))return;actionErrors.set(r.root,{text:r.text});present();},
actionNotices:function(){if(!ports.workspace)return view.workspaceError?h('p',{role:'alert'},view.workspaceError):'';const r=view.hostCommand||view.S?.lifecycle;const {alert}=execution.module("tray.create.actionNotices",{alert:text=>h('div',{class:'body alert',role:'alert'},text)});
  const items=[view.aiError&&alert(view.aiError),view.workspaceError&&alert(view.workspaceError),...view.S.projects.filter(p=>actionErrors.has(p.root)).map(p=>alert(p.name+': '+actionErrors.get(p.root).text)),r?.action==='uninstall'&&h('div',{class:'body meta',role:'status'},'Uninstall '+(r.id||'')+': '+(r.state||'unknown')+(r.reason?' · '+r.reason:''))].filter(Boolean);
  return items.length?f(items):'';},
lifecycleStart:function(root,action='restart',mode='drain'){
  if(view.hostCommand&&view.hostCommand.state!=='observed')return;
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
  if(!Array.isArray(view.S?.projects))return h('div',{class:'body meta'},'Project registry unknown');
  return f(view.S.projects.map(p=>{
    const current=view.S.service?.state==='running'&&typeof p.service_pid==='number'&&typeof p.host_instance==='string'&&typeof p.age==='number'&&p.age>=0&&p.age<=60&&p.service_pid===view.S.service?.pid&&p.host_instance===view.S.service?.instance;
    const text=!current?'waiting for current project observation':p.error?p.error:Array.isArray(p.restart_waits)?p.restart_waits.length?'waits for '+p.restart_waits.join(', '):'prepared':'waiting for recovery observation';
    return h('div',{class:'body meta'},p.name+': '+text);
  }));
},
lifecycleControl:function(p,action='restart'){if(!ports.workspace)return api.operationLauncher('service',p.root,'Review '+p.name+' service…');
  const title={stop:'Stop',uninstall:'Uninstall'}[action]||'Restart',review=action==='uninstall'?p.uninstall_review:action==='stop'?view.S?.stop_review:view.S?.restart_review;
  const all=view.hostCommand||view.S?.lifecycle,r=(all?.action||'restart')===action&&(action!=='uninstall'||(all.root||all.project)===p.root)?all:null;
  const pending=view.hostCommand&&view.hostCommand.state!=='observed'||['queued','applied'].includes(view.S?.lifecycle?.state);
  const projects=Array.isArray(view.S?.projects)?view.S.projects:[];
  const targets=Array.isArray(review?.roots)&&review.roots.every(root=>typeof root==='string')?review.roots.map(root=>(projects.find(p=>p.root===root)?.name||'Project')+' ('+root+')'):null;
  const effect=action==='uninstall'?'Waits for this project’s owned work to end; files and other projects stay.'+(targets?.length===1?' Removes the last host unit after exact exit.':''):action==='stop'?(view.S?.stop_modes?'Drain: durable hold; current work finishes. Automatic rollback to older releases is disabled. Use reviewed recovery to resume selected tasks.':'Waits for all owned work to end; retains OS autostart. Explicit service install can start it again.'):'Existing work is preserved; active turns can delay restart.';
  const scope=action==='uninstall'?p.name:'all registered projects: '+(targets?targets.length?targets.join(', '):'none registered':'unknown');
  return h('details',{class:'host-'+action,'data-view-key':p.root+':'+action},h('summary',null,title+' '+(action==='uninstall'?p.name:'host service')),h('div',{class:'body meta'},'Affects '+scope+'. '+effect),!review?.instance&&h('div',{class:'body alert'},review?.reason||title+' protocol unavailable'),h('button',{class:'project','data-a':action,'data-root':p.root,disabled:!!(pending||!review?.instance||!targets)},'Confirm '+(action==='uninstall'?'uninstall '+p.name:'host '+action)),pending&&r&&api.lifecycleWaits(),(r?.id||r?.reason)&&h('div',{class:'body meta',role:'status'},title+' '+(r.id||'')+': '+(r.state||'unknown')+(r.reason?' · '+r.reason:'')));
},
stopPanel:function(){if(!ports.workspace)return api.operationLauncher('stop',null,'Review Stop and hold…');
  const projects=view.S.projects||[],root=projects[0]?.root,held=view.S.stop_hold;
  const pending=!!held&&!held.selected_count||view.hostCommand&&view.hostCommand.state!=='observed'||['queued','applied'].includes(view.S.lifecycle?.state);
  const names=projects.map(p=>p.name).join(', ')||'no registered projects';
  const host=projects[0]?.host||'unknown host',command=view.hostCommand||view.S.lifecycle;
  const rows=projects.map(p=>h('div',{class:'body'},h('strong',null,p.name),h('div',{class:'meta'},view.S.service?.state!=='running'||typeof p.age!=='number'||p.age<0||p.age>60||p.service_pid!==view.S.service?.pid||p.host_instance!==view.S.service?.instance?'Process state unknown: service observation stale or from another generation.':Array.isArray(p.stop_progress)&&p.stop_progress.length?lines(p.stop_progress.map(k=>'#'+k.task+' '+k.rt+': '+k.state+(k.reason?' · '+k.reason:''))):'No stop acknowledgement; running or unknown work may remain.')));
  return f(h('div',{class:'body'},'Stop on '+host+' · '+names),h('div',{class:'body meta'},'Only TaskQ-owned local work is targeted. Remote hosts and unrelated agents are unaffected. Dirty files and session references are retained; unsaved buffers and interrupted external effects may be lost. Automatic rollback to older releases is disabled.'),
    h('div',{class:'body'},h('strong',null,'Drain'),h('div',{class:'meta'},'Hold new admission and let current work finish. Retain autostart and the hold after service exit.')),h('button',{class:'project','data-a':'stop','data-root':root||'',disabled:!!(pending||!root||!view.S.stop_review?.instance)},'Confirm Drain'),
    h('div',{class:'body'},h('strong',null,'Full stop'),h('div',{class:'meta'},'Request cancellation now, including offline. Unknown termination stays fenced. Board reconciliation may keep the service alive.')),h('button',{class:'project','data-a':'stop-full','data-root':root||'',disabled:!!(pending||!root||!view.S.full_stop_review?.instance)},'Confirm Full stop'),
    !view.S.full_stop_review?.instance&&h('div',{class:'body alert'},view.S.full_stop_review?.reason||'Full stop capability unknown'),
    held&&h('div',{class:'body alert',role:'status'},'Admission held: '+(held.mode||'unknown')+'; ordinary Play cannot clear this hold.'),held&&h('button',{class:'project','data-route':'recovery'},'Review recovery'),command?.action==='stop'&&h('div',{class:'body meta',role:'status'},'Stop '+(command.id||'')+': '+(command.state||'unknown')+(command.reason?' · '+command.reason:'')),rows,api.lifecycleWaits());
},
recoveryPanel:function(){if(!ports.workspace)return api.operationLauncher('recovery',null,'Review recovery…');
  return f(h('div',{class:'body'},'Only selected local tasks resume. Keep held is the default. Paused projects, other tasks and operators stay held.'),h('div',{class:'body meta'},'Inspect native release, work and external effects before confirming. Unknown effects must remain held. Restart discards the old result and starts from preserved files.'),view.S.projects.map(p=>{
    const item=view.recovery[p.root],review=item?.review;
    return h('section',{class:'group','data-recovery-project':p.root,key:p.root},h('div',{class:'body'},h('strong',null,p.name)),h('button',{'data-a':'recovery-read','data-root':p.root,disabled:!!item?.pending},'Read current recovery'),item?.notice&&h('div',{class:'body alert',role:'status'},item.notice),review&&[
      h('div',{class:'body meta'},review.mode+' · focus '+((review.focus||[]).join(', ')||'off')),
      h('div',{class:'body meta'},'Limits: '+(review.limits?.unknown||Object.entries(review.limits?.limits||{}).map(([k,v])=>k+' '+v).join(', ')||'unknown'),h('br'),'Access: '+((review.access||[]).map(a=>a.runtime+': '+(a.refused||a.effective||'unknown')).join('; ')||'unknown')),
      review.rows.map(r=>{
        const draft=item.drafts?.[r.task]||{},blocked=!r.eligible||r.selected||item.pending||(review.pending||[]).some(q=>q.rows.some(s=>s.task===r.task));
        return h('div',{class:'body',key:r.task},h('label',null,h('input',{type:'checkbox','data-recovery-task':r.task,'data-root':p.root,checked:!!draft.selected,disabled:!!blocked}),' #'+r.task+' '+(r.title||'Task')),h('div',{class:'meta'},(r.runtime||'unknown runtime')+' · '+(r.host||'unknown host')+' · '+(r.native_exit||'unknown'),h('br'),'Execution '+(r.execution??'unknown')+' · revision '+(r.revision??'unknown'),h('br'),r.workspace?.state||'work unknown',r.blocker&&[h('br'),r.blocker],r.selected&&[h('br'),'Selected admission confirmed; running state is separate.']),h('a',{href:r.url||'#','data-url':r.url||''},'Inspect task and receipts ↗'),h('label',{class:'meta recovery-evidence'},'Reconciled effect outcomes',h('input',{type:'text',maxlength:'160','data-recovery-evidence':r.task,'data-root':p.root,'aria-label':'Effect evidence for task '+r.task,value:draft.evidence||'',disabled:!!blocked})));
      }),
      (review.pending||[]).map((q,i)=>h('button',{'data-a':'recovery-pending','data-root':p.root,'data-index':i,disabled:!!item.pending},'Reconcile saved selection #'+q.rows[0].task)),
      h('div',{class:'body meta'},'Confirm only when the evidence establishes the effects; native release remains mandatory. Current grants, limits and focus still apply. If the original host exit is verified, confirmation starts host supervision once.'),h('button',{'data-a':'recovery-apply','data-root':p.root,disabled:!!item.pending},'Confirm selected recovery')],
      item?.request&&!item.pending&&h('button',{'data-a':'recovery-reconcile','data-root':p.root},'Reconcile same selection'));
  }));
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
remember:function(){return {...viewState.capture(),workCard:view.workCard};},
navigate:function(next,filter=null,work=null){
  if(!ports.workspace&&(work||['stop','recovery'].includes(next))){send({a:'workspace',open:{page:'operations',operation:work?'resources':next,root:work?Object.keys(work)[0]||null:null}});return;}
  renderer.push(remember());view.page=next;view.taskFilter=filter===null?null:Array.isArray(filter)?filter:[filter];view.working=work;view.workCard=null;view.ask=null;view.noted=null;view.disclosures={};view.restoreScroll=0;
  view.unavailable=null;
  present(true);(menu.querySelector('.content')||document.scrollingElement).scrollTop=0;
  menu.querySelector('h1')?.focus();
},
back:function(){
  const prior=renderer.pop()||{page:['appearance','cadence','notifications','agents','services','about'].includes(view.page)?'settings':view.S?.projects.some(p=>p.root===view.page)?view.taskFilter||view.working?view.page:'projects':'home',taskFilter:null,working:null,workCard:null,scroll:0,open:{}};
  ({page:view.page,taskFilter:view.taskFilter,working:view.working,workCard:view.workCard}=prior);view.noted=null;view.disclosures=prior.open;view.aiRoot=prior.aiRoot;view.restoreScroll=prior.scroll;
  present(true);(menu.querySelector('.content')||document.scrollingElement).scrollTop=prior.scroll;
  viewState.focus(prior.focus);
},
routeButton:function(route,label=routes[route]){return h('button',{class:'project','data-route':route},label);},
answerSlot:function(t){return t.state==='ask'?h('button',{'data-a':'workspace','data-page':'answer','data-root':t.root||'','data-board':t.key.slice(0,t.key.lastIndexOf('/')),'data-task':t.n,'data-revision':t.revision},'Answer in workspace…'):'';},
answerNotices:function(board){return startOperations.answerNotices(board);},
answerToast:function(){return startOperations.answerToast();},
mountAnswers:function(){},
tasks:function(p){return p.tasks.map(t=>({...t,root:p.root,project:p.name,key:p.issues+'/'+t.n,url:t.url||p.issues+'/'+t.n}));},
blockProject:function(p){return {now:view.S.now+Math.round((performance.now()-view.receivedAt)/1000),id:p.root,name:p.name,board:{url:p.issues,kind:p.board_kind},complete:p.complete===true,tasks:tasks(p)};},
unique:function(ts){return [...new Map(ts.map(t=>[t.key,t])).values()];},
counts:function(ts){ts=unique(ts);const {n}=execution.module("tray.create.counts",{n:f=>ts.filter(t=>f.includes(t.state)).length});return {work:n(['doing','review']),ask:n(['ask']),ready:n(['ready']),failed:n(['failed'])};},
mode:function(p){return p.focus.length?'run':p.saved?'paused':'watch';},
counter:function(v,kind,label){return h('span',{class:'counter '+(v>0?kind+' filled':'zero'),'aria-label':label+': '+v,title:label},v);},
known:function(ps){return ps.every(p=>p.complete!==false);},
// #1523: no repeated row buttons; one ⋯ disclosure at the end of the top line holds the row's actions.
taskRow:function(t,global){const p=view.S.projects.find(p=>p.issues+'/'+t.n===t.key),controls=TaskqTaskControls.render(t,{key:t.key,except:['start']}),items=[answerSlot(t),t.state==='held'&&startButton([t]),controls&&raw(controls)].filter(Boolean),open=items.length&&view.rowMenu===t.key,id='row-actions-'+t.key.replace(/[^A-Za-z0-9_-]/g,'-'),label='Actions for #'+t.n;
  return h('div',{class:'task-row','data-card':t.key,tabindex:'-1',key:t.key},raw(TaskqProjectBlocks.task(blockProject(p),t,global)),items.length?h('button',{class:'row-menu','data-a':'row-menu','data-key':t.key,'aria-expanded':String(!!open),'aria-controls':id,'aria-label':label,title:label},'⋯'):'',open?h('div',{class:'row-actions',id,role:'group','aria-label':label},items):'');},
// #1523: Tasks list rows follow the counter-strip order (the one `states` list), then priority, then number.
taskKey:function(t){const order=TaskqProjectBlocks.states.map(([s])=>s),i=order.indexOf(t.state),l=t.labels||[];return [i<0?order.length:i,l.includes('p0')?0:l.some(x=>x==='p1'||x==='priority-1')?1:2,t.n];},
taskOrder:function(a,b){const x=taskKey(a),y=taskKey(b);return x[0]-y[0]||x[1]-y[1]||x[2]-y[2];},
startButton:function(rows){
  const held=rows.filter(t=>t.state==='held');
  if(!held.length)return '';
  const label=held.length===1&&rows.length===1?'Start #'+held[0].n:'Start '+held.length+' held';
  return h('button',{'data-a':'start','data-keys':held.map(t=>t.key).join(' '),title:label,'aria-label':label},label);
},
globalGroup:function(st,ts){ts=ts.filter(t=>t.state===st);return ts.length?h('details',{class:'group','data-view-key':'state:'+st,key:'state:'+st},h('summary',null,h('span',{class:color(st)},names[st]),counter(ts.length,color(st),names[st])),ts.map(t=>taskRow(t,true))):'';},
age:function(s){return s<120?s+' s':s<7200?Math.round(s/60)+' min':s<172800?Math.round(s/3600)+' h':Math.round(s/86400)+' d';},
problem:function(p){const e=!p.alive?(p.age==null?'no status.json: service not running':'status.json '+age(p.age)+' old: service not running?'):p.error?'service error: '+p.error:'';return e?h('button',{class:'project alert','data-a':'log','data-root':p.root,title:e,disabled:!p.log},h('span',null,'⚠ '+p.name+': '+e),h('span',{class:'meta'},p.log?'Log':''),h('span')):'';},
modeText:function(p){return {run:'Admitting: '+p.focus.join(' '),paused:'Paused: running tasks finish, no new ones',watch:'Observation only: focus off'}[mode(p)];},
admission:function(p){if(!ports.workspace)return api.operationLauncher('admission',p.root,'Review admission in '+p.name+'…');
  if(view.pending[p.root]&&!view.pending[p.root].unknown)return h('button',{class:'project',disabled:true},view.pending[p.root].text+'…');
  if(mode(p)==='run')return h('button',{class:'project','data-a':'pause','data-root':p.root},'Pause admission in '+p.name);
  if(p.saved)return h('button',{class:'project','data-a':'resume','data-root':p.root},'Resume admission in '+p.name);
  if(view.ask!==p.root)return h('button',{class:'project','data-a':'pick','data-root':p.root},'Resume: choose labels…');
  // no saved focus: offer labels, never `all` (M-A12)
  return f(h('div',{class:'body meta'},'No saved focus. Admit tasks with one label:'),h('div',{class:'options'},p.labels.length?p.labels.map(l=>h('button',{'data-a':'focus','data-root':p.root,'data-label':l},l)):h('span',{class:'meta'},'No labels on open tasks: use taskq service focus')));
},
incident:function(p,top=false){
  const i=p.incident,local=i&&['local','unsupported-auth'].includes(i.state);
  if(top&&(!p.alive||!local))return '';
  const line=!p.alive?'Incidents: unknown ('+(p.age==null?'no status.json':'stale status.json')+')':p.incident_line||'Incidents: unknown (no incident status)';
  const text=p.alive&&i&&(i.url||i.path)?line.replace(' · '+(i.url||i.path),''):line;
  return f(h('div',{class:'body '+(top?'incident-attention':'meta'),title:line},(top?p.name+': ':'')+text),
    p.alive&&local&&h('div',{class:'options'},h('button',{'data-a':'incident-report','data-root':p.root,disabled:!i.path},'Open report'),h('button',{'data-a':'incident-manual','data-root':p.root},'Report manually ↗')),
    p.alive&&i&&i.state==='sent'&&i.url&&h('a',{class:'project',href:i.url,'data-url':i.url},'Open incident ↗'));
},
incidentNotice:function(p){return p.incident_notice?h('details',{class:'group incident-notice','data-view-key':'incident:'+p.root},h('summary',null,'Incident reports: notice ',h('span',{class:'meta'},p.name)),h('div',{class:'body'},'Reports stay local until you choose.'),h('div',{class:'body meta'},'Destination: '+p.incident_repo+' on GitHub (public). Posting account: unknown here; verify the github.com account used by this project before enabling.'),h('div',{class:'body meta'},'Only sanitized fault codes, fingerprint, report ID, release, revision, contract, board/runtime/OS classes, source site and evidence codes are sent. No project, task, user, session, token or raw error text.'),h('div',{class:'body meta'},'Offline or unsupported login: keep a local JSON report. After confirming the destination and posting account, set incident_reports to true in taskq.json to enable, or false to opt out. TASKQ_INCIDENT_REPORTS=off also disables reporting.')):'';},
service:function(p){const s=view.S?.service;return h('details',{class:'group service','data-view-key':'service:'+p.root},h('summary',null,'⚙ Service ',h('span',{class:'meta'},p.name)),h('div',{class:'body meta',role:'status'},'Host service: '+(s?.state==='running'?'running · PID '+s.pid+(s.fresh?' · fresh observation':' · stale observation'):s?.state==='exited'?'recorded process exited; worker outcomes separate':'unknown')+(s?.reason?': '+s.reason:'')),incident(p),
  p.log?h('button',{class:'project','data-a':'log','data-root':p.root},'Open log'):h('div',{class:'body meta'},'Log path unknown'),
  h('button',{class:'project','data-a':'folder','data-root':p.root},'Open folder'),
  ports.workspace?[api.lifecycleControl(p),api.lifecycleControl(p,'stop'),api.lifecycleControl(p,'uninstall')]:api.operationLauncher('service',p.root,'Review service operations…'));},
when:function(t,tz){return new Date(t*1000).toLocaleString([],{timeZone:tz==='UTC'?'UTC':undefined,dateStyle:'short',timeStyle:'short'})+' '+(tz==='UTC'?'UTC':zone);},
local:function(t,tz){const d=new Date(t*1000);return (tz==='UTC'?d.toISOString():new Date(d-d.getTimezoneOffset()*60000).toISOString()).slice(0,16);},
custom:function(){
  const d=view.draft;
  return f(h('div',{class:'range'},h('label',{for:'hs'},'Start'),h('input',{id:'hs',type:'datetime-local','data-draft':'start',value:d.start}),h('label',{for:'he'},'End'),h('input',{id:'he',type:'datetime-local','data-draft':'end',value:d.end}),h('label',{for:'hz'},'Time zone'),h('select',{id:'hz','data-draft':'tz'},[['local','Local ('+zone+')'],['UTC','UTC']].map(([v,l])=>h('option',{value:v,selected:d.tz===v},l)))),
  d.error&&h('div',{class:'body alert',role:'alert'},d.error),h('button',{class:'project','data-a':'range'},'Show [start, end)'));},
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
  const total=[periods[view.S.period],...(H.since==null?kinds.map(k=>k+' —'):kinds.map((k,i)=>k+' '+tot[i]).concat(part?['incomplete']:[]))].map(t=>t.replace(/ /g,' ')).join(' · ');
  return h('details',{class:'group hist','data-view-key':'history'},h('summary',null,'▥ History ',h('span',{class:'meta'},total)),h('div',{class:'options periods'},Object.entries(periods).map(([k,l])=>h('button',{'data-set':'period','data-v':k,'aria-pressed':String(view.S.period===k||k==='custom'&&view.draft!=null)},l))),
  view.draft&&custom(),h('div',{class:'body meta'},'['+when(H.start,H.tz)+', '+when(H.end,H.tz)+')'+(view.S.now<H.end?' · last bin partial':'')+' · peak '+peak),
  h('div',{class:'bars',role:'img','aria-label':named(tot,', ')+'; '+n+' equal bins, oldest left'+(note?'; '+note:'')},H.bins.map((b,i)=>{const t=H.start+i*w,u=H.since==null||t+w<=H.since;
    return h('span',u?{class:'unknown',title:'unknown'}:{title:when(t,H.tz)+(view.S.now<t+w?' (partial)':'')+': '+named(b,', ')},[3,2,1,0].map(k=>h('i',{class:'k'+k,style:'height:'+b[k]/peak*100+'%'})));})),note&&h('div',{class:'body meta'},note));},
update:function(){if(!ports.workspace)return api.operationLauncher('update',null,'Review updates…');
  const up=view.S.projects.find(p=>p.update&&p.update!==p.version);
  if(view.updating&&view.S.projects.some(p=>p.root===view.updating.root&&p.alive&&Number.isFinite(p.age)&&p.age>=0&&p.age<=60&&p.version===view.updating.v))return h('div',{class:'row meta'},'Updated to '+view.updating.v+': read from status.json');
  if(view.updating&&view.S.now-view.updating.at<300)return h('button',{class:'project',disabled:true},'Updating to '+view.updating.v+'…');
  return f(view.updating&&h('div',{class:'row alert'},'Update to '+view.updating.v+' not confirmed: see the log'),!view.updating&&up&&h('button',{class:'project','data-a':'update','data-root':up.root,'data-v':up.update},'Update now to '+up.update));},
ai:function(){return h('button',{class:'project','data-a':'workspace','data-page':'request','data-root':view.aiRoot||null},'Ask AI · new session…');},
messagesBadge:function(){const m=view.S.messages,known=m?.complete===true&&Number.isSafeInteger(m.count);return f(h('button',{class:'project','data-a':'workspace','data-page':'messages','aria-label':known?'Messages · '+m.count:'Messages · unknown'},'✉ Messages · '+(known?m.count:'unknown')),!known&&h('p',{class:'body meta',role:'status'},m?.reason||'Message source unavailable; refresh status.'));},
addProject:function(){if(!ports.workspace)return api.operationLauncher('setup',null,'Add project in workspace…');
  const A=view.S.add;
  if(!A)return h('button',{class:'project','data-a':'add'},'Add project…');
  if(A.state==='picking')return f(h('div',{class:'row meta'},'Choose a folder…'),h('button',{class:'project','data-a':'add-cancel'},'Hide picker result'));
  if(A.state==='picking dismissed')return h('div',{class:'body meta'},'Folder selection is still being processed; its result will be ignored.');
  if(A.state==='installing dismissed')return h('div',{class:'body meta'},'Installation was submitted; its result is hidden.');
  if(A.state==='pick failed'||A.state==='add failed')return f(h('div',{class:'body alert',role:'alert'},(A.state==='pick failed'?'Folder picker failed':'Add project unavailable')+(A.reason?': '+A.reason:'')),h('button',{class:'project','data-a':'add-cancel'},'Done'));
  const failed=A.state==='install failed',known=view.S.projects.some(p=>p.root===A.path);
  const out=[h('div',{class:'body'},h('div',{class:'meta'},'Folder: '+A.path),h('div',{role:'status',class:A.kind==='old'||failed?'alert':''},(failed?'Install failed':addKinds[A.kind])+(A.reason?': '+A.reason:'')))];
  if(A.kind==='old')out.push(h('a',{class:'project',href:cutover,'data-url':cutover},h('span',null,'Repair or cutover: taskq.md § 9 ↗'),h('span'),h('span')));
  if(A.kind==='taskq'){
    if(A.state==='installed')out.push(h('div',{class:'row work',role:'status'},'Installed: read back from this host\'s projects'));
    else if(A.state==='installing')out.push(h('button',{class:'project',disabled:true},'Installing…'));
    else if(known)out.push(h('div',{class:'body meta'},'Already in this menu'));
    else if(view.addConfirm)out.push(h('div',{class:'body',role:'alert'},'Register '+A.path+' and install its service? It starts at logon.'),h('div',{class:'options'},h('button',{'data-a':'add-install'},'Confirm'),h('button',{'data-a':'add-back'},'Cancel')));
    else if(!failed)out.push(h('button',{class:'project','data-a':'add-ask'},'Install service…'));
  }
  if(A.kind==='new'||A.kind==='plain')out.push(h('button',{class:'project','data-a':'add-setup'},'Set up with AI…'));
  return f(out,h('button',{class:'project','data-a':'add-cancel'},A.state==='installing'?'Hide result; install continues':A.state==='installed'?'Done':'Cancel'));},
bulk:function(run,held){if(!ports.workspace)return run.length||held.length?api.operationLauncher('admission',null,'Review admission…'):'';
  const pausing=run.length>0;
const {state}=execution.module("tray.create.bulk.helpers",{state:p=>({run:pausing?'pause':'admitting, unchanged',paused:pausing?'paused, unchanged':'resume',watch:'observation, unchanged'}[mode(p)])});
  if(view.confirming)return f(h('div',{class:'body',role:'alert'},pausing?'Pause admission? Running tasks finish.':'Resume admission?',h('br'),lines(view.S.projects.map(p=>p.name+': '+state(p)))),h('div',{class:'options'},h('button',{'data-a':pausing?'pause-all':'resume-all'},'Confirm'),h('button',{'data-a':'cancel'},'Cancel')));
  return run.length?h('button',{class:'project','data-a':'confirm'},'Pause all…'):held.length?h('button',{class:'project','data-a':'confirm'},'Resume all…'):'';},
views:function(){return view.S.platform==='macos'?h('div',{class:'options views'},[['icon','Icon'],['full','Summary']].map(([v,l])=>h('button',{'data-set':'view','data-v':v,'aria-pressed':String(view.S.view===v)},h('img',{alt:'',src:view.S.icons[v]}),l))):'';},
appearance:function(){
  return f(h('div',{class:'body meta'},'Theme follows system appearance.'),views(),h('div',{class:'body'},'Text size · '+view.S.font+' px'),h('div',{class:'options'},[14,16,18,20].map(n=>h('button',{'data-set':'font','data-v':n,'aria-pressed':String(n===view.S.font)},n))));
},
agents:function(){
  const A=view.S.agents;
  return f(h('div',{class:'body meta'},view.S.host+' · '+(A?'checked '+age(Math.max(0,view.S.now-A.at))+' ago':'unknown: no service probe yet')),A?A.rows.map(r=>h('div',{class:'body'},r)):h('div',{class:'body meta'},'Unknown: no status.json has agents'));
},
about:function(){
  return f(h('div',{class:'body'},'Tray '+view.S.version+' · '+view.S.host),view.S.projects.map(p=>h('div',{class:'body'},p.name+': '+(p.version||'version unknown'),h('span',{class:'meta'},' · '+(p.age==null?'status unknown':age(p.age)+' old')))),update());
},
setup:function(){
  const s=view.S.setup;if(!s)return '';
  if(!ports.workspace)return f(h('div',{class:'body meta'},'Setup · '+(s.kind||'unknown')),api.operationLauncher('setup',null,'Review setup status…'));
  const link=s.url&&/^(https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+|codex:\/\/threads\/[A-Za-z0-9-]+)$/.test(s.url)&&[' ',h('a',{href:'#','data-url':s.url},'Open session')];
  return h('div',{class:'group'},h('div',{class:'row'},'Setup · '+s.kind,link),s.reason&&h('div',{class:'body meta'},s.reason),h('div',{class:'body'},s.next||'Setup state unknown; run `taskq service setup`'));
},
attention:function(){
  const all=unique(view.S.projects.flatMap(tasks));
  const parts=[setup(),...view.S.projects.flatMap(p=>[problem(p),incident(p,true),incidentNotice(p)]),globalGroup('ask',all),globalGroup('failed',all),answerNotices()].filter(Boolean);
  return parts.length?f(parts):'';
},
projects:function(){
  return f(raw(TaskqProjectBlocks.aggregate(view.S.projects.map(blockProject),[])),view.S.projects.length?view.S.projects.map(p=>[raw(TaskqProjectBlocks.summary(blockProject(p))),h('div',{class:'body meta'},modeText(p)),h('div',{class:'body meta'},'You asked: '+(p.request||'not recorded'))]):h('div',{class:'body meta'},'No projects yet'),addProject());
},
controls:function(){
  const run=view.S.projects.filter(p=>mode(p)==='run'),held=view.S.projects.filter(p=>mode(p)==='paused');
  return f(workEntry(),held.length>0&&run.length>0&&h('div',{class:'row meta'},'Paused '+held.length+' of '+(held.length+run.length)+': '+held.map(p=>p.name).join(', ')),bulk(run,held));
},
runningAgents:function(){
  const boards=new Map(),hosts=new Map();
  for(const p of view.S.projects){
    const key=p.issues||p.root,old=boards.get(key);
    if(!old||p.observed>old.observed)boards.set(key,p);
  }
  let partial=false,doing=0,review=0;
  const reasons=new Set(),seen=new Map();
  for(const p of boards.values()){
    const elapsed=Number.isFinite(p.age)&&p.age>=0&&Number.isFinite(p.tasks_age_seconds)&&p.tasks_age_seconds>=0?p.age+p.tasks_age_seconds:null;
    const fresh=p.complete===true&&elapsed!==null&&elapsed<=60&&Number.isFinite(p.observed);
    partial ||= !fresh||!Array.isArray(p.hosts)||!Array.isArray(p.tasks);
    if(!fresh)reasons.add(!p.complete?'partial board snapshot':elapsed===null?'missing task observation':'stale task observation');
    if(!Array.isArray(p.hosts)||!Array.isArray(p.tasks))reasons.add('missing host/task data');
    for(const h of Array.isArray(p.hosts)?p.hosts:[]){
      if(!h||typeof h.name!=='string'||!h.name){partial=true;continue;}
      const age=Number.isFinite(h.updated)&&Number.isFinite(p.observed)&&elapsed!==null?p.observed-h.updated+elapsed:null;
      const old=hosts.get(h.name);
      if(!old)hosts.set(h.name,{name:h.name,age,unknown:!fresh||age===null||age<0||age>60,counts:new Map()});
      else {old.unknown ||= !fresh||age===null||age<0||age>60;old.age=old.age===null||age===null?null:Math.max(old.age,age);}
    }
    for(const t of Array.isArray(p.tasks)?p.tasks:[]){
      doing+=t.state==='doing'?1:0;review+=t.state==='review'?1:0;
      if(t.executions!=null&&!Array.isArray(t.executions))partial=true;
      for(const x of Array.isArray(t.executions)?t.executions:[]){
        if(!x||typeof x.host!=='string'){partial=true;reasons.add('invalid execution host');continue;}
        const name=x.host||'Unknown host';
        if(!hosts.has(name))hosts.set(name,{name,age:null,unknown:true,counts:new Map()});
        const h=hosts.get(name),age=Number.isFinite(x.observed)&&Number.isFinite(p.observed)&&elapsed!==null?p.observed-x.observed+elapsed:null;
        if(!fresh||age===null||age<0||age>60||x.state!=='running'||!['worker','reviewer'].includes(x.role)||typeof x.runtime!=='string'||!x.runtime||!Number.isSafeInteger(x.id)||x.id<=0){h.unknown=true;h.reason='Run state unknown; see service log';continue;}
        const identity=JSON.stringify([p.issues||p.root,t.n,x.id]),prior=seen.get(identity);
        if(prior){if(prior.host!==x.host||prior.runtime!==x.runtime||prior.role!==x.role){h.unknown=true;hosts.get(prior.host).unknown=true;reasons.add('conflicting execution observations');}continue;}
        seen.set(identity,x);
        const counts=h.counts.get(x.runtime)||{worker:0,reviewer:0};counts[x.role]++;h.counts.set(x.runtime,counts);
      }
    }
  }
  const out=[];
  for(const host of [...hosts.values()].sort((a,b)=>a.name.localeCompare(b.name))){
    const unknown=partial||host.unknown;
    const runtimes=[...host.counts].sort(([a],[b])=>a.localeCompare(b));
    if(!unknown&&runtimes.length)out.push(h('details',{'data-view-key':'host:'+host.name,key:'host:'+host.name},h('summary',null,h('strong',null,host.name),' · '+runtimes.map(([rt,c])=>rt+' '+(c.worker+c.reviewer)).join(' · ')),h('div',{class:'body'},runtimes.map(([rt,c])=>h('div',null,rt+': '+c.worker+' worker'+(c.worker===1?'':'s')+' · '+c.reviewer+' reviewer'+(c.reviewer===1?'':'s'))))));
    else out.push(h('div',{class:'body'},h('strong',null,host.name),unknown?' · unknown'+(host.age!==null&&host.age>=0?' · '+age(host.age)+' old':'')+': '+([...reasons].join('; ')||host.reason||'host observation missing or stale'):' · idle'));
  }
  if(!hosts.size)out.push(h('div',{class:'body meta'},'Hosts unknown: no observation'));
  const local=[...boards.values()].filter(p=>p.host===view.S.host),latest=local.filter(p=>p.capacity).sort((a,b)=>a.age-b.age)[0];
  const cap=latest?.capacity?.host_runtime_used,capFresh=latest&&Number.isFinite(latest.age)&&latest.age>=0&&latest.age<=60;
  const occupied=cap&&typeof cap==='object'&&!Array.isArray(cap)&&Object.values(cap).every(n=>Number.isInteger(n)&&n>=0);
  const counted=[h('div',{class:'body meta'},'Registered project boards · '+(partial?'Tasks unknown':'Tasks: Doing '+doing+' · Review '+review))];
  if(capFresh&&occupied){
    counted.push(h('div',{class:'body meta'},view.S.host+' · Occupied slots: '+(Object.entries(cap).map(([rt,n])=>rt+' '+n).join(' · ')||'0')+' · '+age(latest.age)+' old'));
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
    counted.push(h('div',{class:'body meta'},'Reservations: '+(known?([...counts].map(([rt,n])=>rt+' '+n).join(' · ')||'none'):'unknown')));
  }else counted.push(h('div',{class:'body meta'},view.S.host+' · Capacity unknown'));
  return h('section',{class:'group running-agents','aria-label':'Running agents'},h('div',{class:'row'},'Running agents'),out,h('details',{'data-view-key':'counts'},h('summary',null,'Reconcile counts'),counted,h('div',{class:'body meta'},'Reservations include pending or unconfirmed work. Task states differ from running processes; occupied slots can include other projects. Remote reservations: unknown.')));
},
home:function(){
  const faults=[setup(),...view.S.projects.flatMap(p=>[problem(p),incident(p,true),incidentNotice(p)])].filter(Boolean);
  return f(api.runningAgents(),controls(),ai(),api.messagesBadge(),
    faults.length>0&&h('section',{class:'group','aria-label':'Attention'},faults),filteredTasks(),
    h('div',{class:'group'},routeButton('projects'),routeButton('appearance')));
},
routeBody:function(){
  switch(view.page){
    case 'projects':return projects();
    case 'attention':return attention()||h('div',{class:'body meta'},known(view.S.projects)?'No reported questions or failures':'Status incomplete; check Services');
    case 'settings':return f(h('div',{class:'row meta'},'Display and notices'),['appearance','cadence','notifications'].map(r=>routeButton(r)),h('div',{class:'row meta'},'Host and maintenance'),['agents','services','about'].map(r=>routeButton(r)),workEntry());
    case 'appearance':return appearance();
    case 'cadence':return history();
    case 'notifications':return notifications();
    case 'agents':return agents();
    case 'stop':return api.stopPanel();
    case 'recovery':return api.recoveryPanel();
    case 'services':return f(api.operationLauncher('service',null,'Host maintenance in workspace…'),view.S.projects.map(p=>[h('div',{class:'body'},p.name+' · '+(p.host||'host unknown'),h('span',{class:'meta'},' · '+(p.age==null?'status unknown':age(p.age)+' old'))),problem(p),incidentNotice(p),service(p)]));
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
capacity:function(p){if(!ports.workspace)return p.resources?api.operationLauncher('resources',p.root,'Review capacity…'):'';
  if(!p.resources)return '';
  return h('details',{class:'group','data-view-key':'capacity:'+p.root},h('summary',null,'▤ Capacity ',h('span',{class:'meta'},(p.cap_host||'this host')+' · '+TaskqResources.headline(resourceGroup(p),view.S.now))),h('div',{'data-resources':p.root}));},
workEntry:function(root){return view.S.projects.some(p=>p.resources)?h('button',{class:'project','data-a':'work','data-root':root||null},'◎ Work mode…'):'';},
workPage:function(){if(!ports.workspace)return api.operationLauncher('resources',Object.keys(view.working||{})[0]||null,'Review host limits in workspace…');const ps=view.S.projects.filter(p=>p.resources),p=ps.find(p=>view.working?.[p.root])||ps[0];return p?.resources.targets?.some(t=>t.supported?.some(s=>s.policy_schema==='taskq.host-capacity/1'))?h('div',{'data-resources':p.root}):h('div',{'data-work':''});},
mountResources:function(){
  const ws=menu.querySelector('[data-work]');
  if(ws){
    if(!view.workCard){const el=document.createElement('div');view.workCard={el,ui:TaskqResources.mountWork(el,{capability:resourceCapability,send:resourceSend,now:()=>view.S.now,host:view.S.host},view.working)};}
    if(ws.firstElementChild!==view.workCard.el)ws.replaceChildren(view.workCard.el);
    view.workCard.ui.update(view.S.projects.filter(p=>p.resources).map(resourceGroup));
  }
  for(const slot of menu.querySelectorAll('[data-resources]')){
    const p=view.S.projects.find(p=>p.root===slot.dataset.resources);
    if(!p)continue;
    const scope=p.resources.targets?.[0]?.supported?.[0]?.policy_schema;
    let card=resourceCards.get(p.root);
    if(!card||card.scope!==scope){card?.el.remove();const el=document.createElement('div');card={el,scope,ui:TaskqResources.mount(el,{capability:resourceCapability,send:resourceSend,now:()=>view.S.now})};resourceCards.set(p.root,card);}
    if(card.el.parentNode!==slot)slot.append(card.el);
    card.ui.update(resourceGroup(p));
  }
},
notifications:function(){
  const N=view.S.notify,st=N.status,line={allowed:'Allowed by the OS',blocked:'Blocked by the OS: nothing is posted','not asked':'Not asked yet: the first banner setting asks the OS',asking:'Asked: answer the OS prompt','request failed':'OS permission request failed: '+N.failure,unknown:'OS permission unknown',unsupported:'Not available on this host'}[st]||st;
  return f(h('div',{class:'body'+(st==='blocked'||st==='request failed'?' alert':' meta'),role:'status'},line),st==='blocked'&&h('button',{class:'project','data-a':'notify-os'},'Open OS setting ↗'),
    kinds.map(([k,l])=>[h('div',{class:'body'},l),h('div',{class:'options'},modes.map(([v,m])=>h('button',{'data-a':'notify','data-kind':k,'data-v':v,'aria-pressed':String(N.host[k]===v)},m)),h('button',{'data-a':'notify-test','data-kind':k,disabled:st!=='allowed'},'Send test'))]));
},
notifyGroup:function(p){
  const N=view.S.notify.projects[p.root];if(!N)return '';
  const {name}=execution.module("tray.create.notifyGroup",{name:v=>modes.find(m=>m[0]===v)[1]});
  return h('details',{class:'group','data-view-key':'notifications:'+p.root},h('summary',null,'◉ Notifications ',h('span',{class:'meta'},kinds.map(([k])=>name(N[k][1])).join(' · '))),kinds.map(([k,l])=>[h('div',{class:'body'},l+' ',h('span',{class:'meta'},'· '+name(N[k][1]))),h('div',{class:'options'},[['inherit','Inherit'],...modes].map(([v,m])=>h('button',{'data-a':'notify','data-kind':k,'data-root':p.root,'data-v':v,'aria-pressed':String((N[k][0]||'inherit')===v)},m)))]));
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
notedCard:function(p){return view.noted&&view.noted.root===p.root?h('div',{class:'group'},h('div',{class:'row meta'},h('span',null,'Notification')),h('div',{'data-card':view.noted.key,tabindex:'-1'},raw(TaskqProjectBlocks.task(blockProject(p),view.noted)))):'';},
project:function(p){
  const ts=tasks(p),counters=TaskqProjectBlocks.counters(TaskqProjectBlocks.observation([blockProject(p)]).counts,p.root,p.name);
  const groups=['ask','failed','doing','review','ready','waiting','later','held'].map(st=>{const r=ts.filter(t=>t.state===st);return r.length?h('div',{class:'group',key:st},h('div',{class:'row meta'},h('span',null,names[st]),counter(r.length,color(st),names[st]),r.length>1&&startButton(r)),r.map(t=>taskRow(t))):'';}).filter(Boolean);
  return f(h('section',{class:'tqp',dangerouslySetInnerHTML:{__html:counters}}),h('div',{class:'body meta'},modeText(p)),problem(p),incident(p,true),incidentNotice(p),notedCard(p),
  groups.length?groups:h('div',{class:'body muted'},'No open tasks'),answerNotices(p.issues),
  h('div',{class:'group'},admission(p)),capacity(p),notifyGroup(p),workEntry(p.root),service(p));},
filteredTasks:function(p){
  const ps=p?[p]:view.S.projects,selected=view.taskFilter||[],states=TaskqProjectBlocks.states;
  const rows=unique(ps.flatMap(tasks)).filter(t=>!selected.length||selected.includes(t.state)).sort(taskOrder);
  const listing=!p&&!selected.length?ps.filter((p,i)=>ps.findIndex(q=>q.issues===p.issues)===i).map(p=>{
      const counts=TaskqProjectBlocks.observation([blockProject(p)]).counts;
      return Object.values(counts).some(n=>n===null||n>0)?h('section',{class:'all-tasks-project',key:p.issues},raw(TaskqProjectBlocks.summary(blockProject(p))),tasks(p).sort(taskOrder).map(t=>taskRow(t))):'';
    }).filter(Boolean):rows.map(t=>taskRow(t,!p));
  return f(h('header',{class:'filter-header'},p?h('section',{class:'tqp',dangerouslySetInnerHTML:{__html:TaskqProjectBlocks.counters(TaskqProjectBlocks.observation([blockProject(p)]).counts,p.root,p.name,p.complete===true?[]:[p.name],selected,TaskqProjectBlocks.observation(view.S.projects.map(blockProject)).open)}}):raw(TaskqProjectBlocks.aggregate(ps.map(blockProject),selected)),
    h('div',{class:'body meta',role:'status'},(selected.length?states.filter(([s])=>selected.includes(s)).map(([,name])=>name).join(', '):'All states')+' · '+rows.length+' loaded'+(known(ps)?'':' · Partial'))),
    !known(ps)&&h('div',{class:'body meta'},'Partial: refresh the affected project or open its Board.'),
    rows.filter(t=>t.state==='held').length>1&&h('div',{class:'row'},startButton(rows)),answerNotices(p?.issues),
    listing.length?listing:h('div',{class:'body meta'},(known(ps)?'No matching loaded tasks':'Tasks unknown')+' ',h('button',{'data-a':'filter-reset'},'Reset')));
},
head:function(){
  const mode=view.S.size_anchor||(view.S.size?'fixed':'top'),label={fixed:'Fixed size',bottom:'Keep bottom edge',top:'Keep top edge'}[mode],p=view.S.projects?.find(p=>p.root===view.page);
  const title=view.working?'Work mode':view.taskFilter?'Tasks':p?p.name:routes[view.page]||'Unavailable';
  const crumbs=p?[h('a',{href:'#','data-route':'projects'},'Projects'),(view.taskFilter||view.working)&&[' › ',h('a',{href:'#','data-route':p.root,title:p.name},p.name)]]:['appearance','cadence','notifications','agents','services','about'].includes(view.page)?h('a',{href:'#','data-route':'settings'},'Settings'):'';
  const actions=[p?.issues&&h('a',{href:p.issues,'data-url':p.issues,'aria-label':'Open '+p.name+' Board',title:'Open Board'},'Board ↗'),h('details',{class:'shell-overflow','data-view-key':'navigation-actions'},h('summary',{title:'Navigation and window options','aria-label':'Navigation and window options'},'⋯'),h('nav',{class:'overflow-actions','aria-label':'Menu'},h('button',{'data-route':'home',title:'Home'},'Home'),h('button',{'data-route':'settings',title:'Settings'},'Settings'),h('button',{'data-a':'workspace',title:'Open workspace'},'Workspace'),view.S.stop_modes&&h('button',{'data-a':'stop-review',title:'Emergency stop'},'Emergency stop…'),h('button',{'data-a':'size-anchor',title:label,'aria-label':'Size anchor: '+label},label),h('button',{'data-a':'top','aria-pressed':String(!!view.S.top),title:'Keep above other windows','aria-label':'Keep above other windows'},'Keep above other windows'),h('button',{'data-a':'pin','aria-pressed':String(!!view.S.pinned),title:view.S.pinned?'Unpin: back to the popover':'Pin: keep open as a movable window','aria-label':view.S.pinned?'Unpin window':'Pin window'},view.S.pinned?'Unpin window':'Pin window')))];
  return shell.bar({title,crumbs,back:view.page!=='home'||!!view.taskFilter||!!view.working,actions,windows:h('button',{'data-a':'hide',title:'Close window','aria-label':'Close window'},'×')});
},
draw:function(navigating=false){
  if(!view.S||view.resizing)return;
  const firstRender=!view.drawn;
  const active=document.activeElement,answerFocus=active&&active.closest('[data-answer]'),scroll=(menu.querySelector('.content')||document.scrollingElement).scrollTop;
  const bounds=new Map([...menu.querySelectorAll('[data-bound]')].map(e=>[e.dataset.bound,e.scrollTop]));
  const focusKey=!navigating&&active&&menu.contains(active)?JSON.stringify(active.dataset):null;
  // a refresh rebuilds the menu: refocus the same control, link or heading, by tag, data and position (U-A4)
  const {same}=execution.module("tray.create.draw.helpers",{same:e=>e.tagName===active.tagName&&JSON.stringify(e.dataset)===focusKey});
const focusAt=focusKey?[...menu.querySelectorAll(active.tagName)].filter(same).indexOf(active):-1;
  const focusedSummary=!navigating&&active?.tagName==='SUMMARY'?[...menu.querySelectorAll('summary')].indexOf(active):-1;
  const caret=answerFocus&&active.tagName==='TEXTAREA'?[active.selectionStart,active.selectionEnd]:null;
  const answerSelector=answerFocus&&(caret?'textarea':active.dataset.choice?'[data-choice="'+active.dataset.choice+'"]':active.dataset.qac?'[data-qac="'+active.dataset.qac+'"]':'button[type=submit]');
  const open=navigating||view.restoreScroll!=null?view.disclosures:viewState.groups(),p=view.S.projects.find(p=>p.root===view.page);
  if(!p&&!routes[view.page])view.page='home';
  document.documentElement.style.colorScheme=view.S.theme||'light dark';
  menu.style.setProperty('--size',view.S.font+'px');
  menu.classList.toggle('pinned',!!view.S.pinned);const edge=view.S.attached_edge||'top';if(menu.dataset.edge!==edge)menu.dataset.edge=edge;
  const settingsError=view.persistenceError||view.S.settings_error;
  const title=view.working?'Work mode':view.taskFilter?'Tasks':p?p.name:routes[view.page];
  const tree=[head(),h('div',{class:'content'},h('div',{class:'content-body'},view.unavailable&&h('div',{class:'body',role:'status'},view.unavailable+' The surviving destination is '+title+'.'),actionNotices(),settingsError&&h('div',{class:'body alert',role:'alert'},'Settings unknown: '+settingsError+(view.persistenceError?'':'; defaults shown')),view.working?workPage():view.taskFilter?filteredTasks(p):p?project(p):routeBody())),h('div',{class:'grip','data-resize':'',role:'button','aria-label':'Resize window with arrow keys',title:'Resize window with arrow keys',tabindex:'0'}),answerToast()];
  // #1430: Preact keeps unchanged nodes, so a refresh leaves the textarea being typed in attached.
  renderer.render(tree,p?'/projects/'+encodeURIComponent(p.issues)+(view.taskFilter?'/tasks':''):view.working?'/work':'/'+view.page);
  view.drawn=true;
  if(navigating)menu.querySelector('.shell-overflow')?.removeAttribute('open');
  mountAnswers();mountResources();
  menu.querySelectorAll('details[data-view-key]:not(.shell-overflow)').forEach(d=>d.open=!d.classList.contains('answered')&&['cadence','agents','services','attention'].includes(view.page));
  viewState.restore(open);
  if(view.S.page!==undefined&&(navigating||firstRender))menu.querySelectorAll('.hist,.agents,.service,.incident-notice').forEach(d=>d.open=!(p&&d.classList.contains('service')));
  const restored=answerFocus&&menu.querySelector('[data-answer="'+CSS.escape(answerFocus.dataset.answer)+'"] '+answerSelector);
  if(restored&&!navigating){restored.focus({preventScroll:true});if(caret)restored.setSelectionRange(...caret);}
  else if(!navigating){const target=focusedSummary>=0?menu.querySelectorAll('summary')[focusedSummary]:focusKey&&active.tagName==='H1'?menu.querySelector('h1'):focusKey&&focusKey!=='{}'?[...menu.querySelectorAll(active.tagName)].filter(same)[focusAt]:null;target?.focus({preventScroll:true});}
  (menu.querySelector('.content')||document.scrollingElement).scrollTop=view.restoreScroll??scroll;view.restoreScroll=null;
  if(firstRender)viewState.focus(view.restoreFocus);
  fit();
  // after fit() sets the width: a new exchange or text size opens at the newest entry; a tick keeps the reader's place;
  // a block that drops a cut line asks for the smaller height
  const bound=[...menu.querySelectorAll('[data-bound]')];
  bound.forEach(c=>{c.scrollTop=bounds.has(c.dataset.bound)?bounds.get(c.dataset.bound):0;wholeLines(c);});
  if(bound.some(c=>c.style.maxHeight))fit();
  viewState.flush();},
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
  if(ports.workspace&&s.projects){view.S=s;focusReadback();api.lifecycleReadback();present();return;}
  if(s.answerReceipt){startOperations.answerReceipt(s.answerReceipt);return;}
  if(s.workspaceError!==undefined){view.workspaceError=s.workspaceError;present();return;}
  if(s.viewStateError!==undefined){view.persistenceError=s.viewStateError;present();return;}
  if(s.diagnose){const d=s.diagnose,remaining=d.until_ms-Date.now();
    if(d.id!==view.diagnosticId&&remaining>0){execution.diagnose(d.selector,Math.min(60000,remaining),d.limit);view.diagnosticId=d.id;}
    return;
  }
  if(s.aiError){view.aiSending=false;view.aiError=s.aiError;present();return;}
  if(!ports.workspace&&['action_error','focus_receipt','resources_receipt','capmsg','aiError'].some(k=>s[k]))return;
  if(s.action_error){actionReceipt(s.action_error);return;}
  if(s.focus_receipt){focusReceipt(s.focus_receipt);return;}
    if(s.resources_receipt){view.resourceNotice=s.resources_receipt;present();return;}
  if(s.capmsg){if(api.recoveryReceipt(s.capmsg))return;if(view.hostCommand?.id===s.capmsg.id)api.lifecycleReceipt(s.capmsg);else resourceReceipt(s.capmsg);return;}
  if(s.open){openCard(s.open);return;}
  if(view.S?.size_anchor!==s.size_anchor)view.manualSize=null;
  viewState.hydrate(s);view.S=s;view.receivedAt=performance.now();focusReadback();api.lifecycleReadback();
  // snapshot mode names the page
  if(s.page!==undefined&&s.page!==view.page){view.page=s.page;view.taskFilter=null;view.working=null;view.workCard=null;renderer.clear();view.disclosures={};present(true);return;}
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
ui.listen(menu,'toggle',()=>viewState.schedule(),true);
ui.listen(menu,'scroll',e=>{const c=e.target;if(c.classList?.contains('content'))viewState.schedule();if(!c.dataset?.bound)return;cancel(c.settling);c.settling=defer(()=>settle(c),150);},true);
// a refresh between press and release rebuilds the pressed control and its click is lost (U-A4):
// the refresh waits for the release, at most 5 s, then runs after the click


ui.listen(menu,'pointerdown',()=>{view.pressedAt=performance.now();});
for(const event of ['pointerup','pointercancel'])ui.listen(window,event,release,true);
ui.listen(window,'blur',release);
// Press end.

ui.listen(menu,'click',e=>{
  const crumb=e.target.closest('a[data-route]');if(crumb){e.preventDefault();if(crumb.dataset.route!==view.page||view.working||view.taskFilter)navigate(crumb.dataset.route);return;}
  const cardLink=e.target.closest('.qac a');if(cardLink){e.preventDefault();send({a:'open',url:cardLink.getAttribute('href')});return;}
  if(e.target.closest('.qac,.rsc'))return;
  if(TaskqProjectBlocks.handle(e,{open:url=>send({a:'open',url}),project:id=>navigate(id),tasks:(id,state)=>{if(view.taskFilter&&view.page===(id||'home')){view.taskFilter=view.taskFilter.includes(state)?view.taskFilter.filter(s=>s!==state):[...view.taskFilter,state];present();}else navigate(id||'home',state);}}))return;
  const a=e.target.closest('a[data-url]');if(a){e.preventDefault();send({a:'open',url:a.dataset.url});return;}
  const b=e.target.closest('button');if(!b||b.disabled)return;const d=b.dataset;
  if(d.a==='row-menu'){view.rowMenu=view.rowMenu===d.key?null:d.key;present();return;}
  if(d.a==='control'||d.a==='start'||d.a==='workspace')view.rowMenu=null;
  if(!ports.workspace&&api.operationTarget(d)){send({a:'workspace',open:{page:'operations',operation:api.operationTarget(d),root:d.root||null}});return;}
  if(d.a==='filter-reset'){view.taskFilter=[];present();return;}
  if(d.a==='all-tasks'){if(view.page==='home'&&view.taskFilter){view.taskFilter=[];present();}else navigate('home',[]);return;}
  if(d.a==='hide')viewState.flush();
  if(d.a==='workspace'){viewState.flush();send({a:'workspace',open:{page:d.page||'messages',operation:d.operation||null,root:d.root||null,board:d.board||null,task:Number(d.task)||null,revision:Number(d.revision)||null}});return;}
  if(d.route){if(d.route!==view.page||view.working||view.taskFilter)navigate(d.route);return;}
  if(d.a==='answer-dismiss'){startOperations.dismiss(d.key);return;}
  if(d.a==='toast-dismiss'){view.toast=null;present();return;}
  if(d.a==='control'){const t=unique(view.S.projects.flatMap(tasks)).find(t=>t.key===d.key);if(t&&(t.controls||[]).includes(d.control))ui.effect('task.control',()=>answerSend(t.key.slice(0,-String(t.n).length-1),TaskqTaskControls.op(t,d.control,'tray')),()=>{});present();return;}
  if(d.a==='start'){for(const key of d.keys.split(' ')){const t=unique(view.S.projects.flatMap(tasks)).find(t=>t.key===key);if(t&&t.state==='held')ui.effect('task.start',()=>answerSend(t.key.slice(0,-String(t.n).length-1),{id:'tray.start.'+t.n+'.'+t.revision,task:t.n,revision:t.revision,action:'start'}),()=>{});}present();return;}
  if(d.a==='answer-retry'){startOperations.retry(d.key);present();return;}
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
  if(api.operationAction(d))return;
  if(d.a){actionErrors.delete(d.root);send({a:d.a,root:d.root,label:d.label});}present();});
ui.listen(menu,'input',e=>{api.operationInput(e);const k=e.target.dataset.draft;if(k&&view.draft)view.draft[k]=e.target.value;});
ui.listen(menu,'focusout',e=>{const {f}=execution.module("tray.create.mount.focusout",{f:x=>x&&(x.id==='ai'||x.dataset&&(x.dataset.draft||x.dataset.rscField||x.dataset.recoveryEvidence))});if(f(e.target))defer(()=>{if(!f(document.activeElement))present();});});
ui.listen(menu,'mousedown',e=>{if(view.S&&e.button===0&&e.target.closest('.head')&&!e.target.closest('button,a,summary'))send({a:'drag'});});
ui.listen(menu,'pointerdown',e=>{if(e.button!==0||!e.target.closest('.grip'))return;view.resizing={x:e.screenX,y:e.screenY,w:window.innerWidth,h:window.innerHeight};e.target.setPointerCapture(e.pointerId);e.preventDefault();});
ui.listen(menu,'pointermove',e=>{if(!view.resizing)return;view.manualSize=[Math.max(240,view.resizing.w+e.screenX-view.resizing.x),Math.max(120,view.resizing.h+e.screenY-view.resizing.y)];menu.style.width='100%';send({a:'resize',d:window.devicePixelRatio,w:view.manualSize[0],h:view.manualSize[1]});});
for(const event of ['pointerup','pointercancel','lostpointercapture'])ui.listen(menu,event,()=>{if(view.resizing)send({a:'resize-end'});view.resizing=null;});
ui.listen(menu,'keydown',e=>{if(!e.target.closest('.grip')||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();view.manualSize=[Math.max(240,window.innerWidth+(e.key==='ArrowRight'?10:e.key==='ArrowLeft'?-10:0)),Math.max(120,window.innerHeight+(e.key==='ArrowDown'?10:e.key==='ArrowUp'?-10:0))];send({a:'resize',d:window.devicePixelRatio,w:view.manualSize[0],h:view.manualSize[1]});send({a:'resize-end'});});
ui.listen(document,'keydown',e=>{if(e.key==='Escape'&&view.rowMenu){const key=view.rowMenu;view.rowMenu=null;present();menu.querySelector('[data-a="row-menu"][data-key="'+CSS.escape(key)+'"]')?.focus();return;}if(e.key==='Escape'){const overflow=menu.querySelector('.shell-overflow[open]');if(overflow){overflow.open=false;overflow.querySelector('summary').focus();return;}viewState.flush();send({a:'hide'});}if(e.altKey&&e.key==='ArrowLeft'){e.preventDefault();if(view.page!=='home'||view.taskFilter||view.working)back();}});
ui.listen(menu,'toggle',()=>fit(),true);
// § 4 Tray: row ages tick in place while the menu is open; a redraw would move the reading position
const {ages}=execution.module("tray.create.mount.ages",{ages:()=>{if(view.S){const now=view.S.now+Math.round((performance.now()-view.receivedAt)/1000);for(const e of menu.querySelectorAll('[data-since]'))e.textContent=TaskqProjectBlocks.age(now-Number(e.dataset.since));}defer(ages,30000);}});
defer(ages,30000);
ui.effect('ui.fonts',()=>document.fonts.ready,()=>{if(view.S)fit();});
send({a:'ready'});
}
});
const {natural,gutter,fit,actionReceipt,actionNotices,focusStart,focusReceipt,focusReadback,remember,navigate,back,routeButton,answerSlot,answerNotices,answerToast,mountAnswers,tasks,blockProject,unique,counts,mode,counter,known,taskRow,taskKey,taskOrder,startButton,globalGroup,age,problem,modeText,admission,incident,incidentNotice,service,when,local,custom,history,update,ai,addProject,bulk,views,appearance,agents,about,setup,attention,projects,controls,home,routeBody,resourceGroup,resourceCapability,resourceReceipt,capacity,workEntry,workPage,mountResources,notifications,notifyGroup,openCard,notedCard,project,filteredTasks,head,draw,wholeLines,settle,refresh,release,receive,measure,mount}=api;

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
const TaskqAnswerCard=require('./answer-card.js'),TaskqResources=require('./resource-controls.js'),TaskqProjectBlocks=require('./project-blocks.js'),TaskqTaskControls=require('./task-controls.js');
const send=ports.send||ui.send;
const performance=window.performance||globalThis.performance,crypto=window.crypto||globalThis.crypto,CSS=window.CSS||globalThis.CSS;
const present=execution.action('ui.render',(...args)=>(ports.render||draw)(...args));
const renderer=shell.create(menu);
const names={doing:'In work',review:'Review',ask:'Waiting for answer',failed:'Failed',ready:'Ready',waiting:'Dependencies',later:'Later',held:'Held'};
const {color}=execution.module("tray.create",{color:st=>({doing:'work',review:'work',ask:'ask',failed:'failed',ready:'ready'}[st]||'muted')});
const periods={'5m':'5 min','1h':'Hour','1d':'Day','1w':'7 days','1mo':'30 days',custom:'Custom'};
const zone=Intl.DateTimeFormat().resolvedOptions().timeZone;
// Pages are Preact vnodes; shared module HTML enters only as an island the module owns.
const {f,raw,lines}=execution.module("tray.create",{f:(...children)=>h(Fragment,null,...children),raw:html=>html?h('div',{style:'display:contents',dangerouslySetInnerHTML:{__html:html}}):'',lines:items=>items.flatMap((x,i)=>i?[h('br'),x]:[x])});

// measure at the target width: after a font change the window still has the old width, so text wraps differently
// g: this renderer's scrollbar width; a menu taller than its window scrolls and the window adds it (#1073)



const actionErrors=new Map();





// Navigation is view state only: every route consumes the same pushed snapshot.
const routes={recovery:'Reviewed recovery',stop:'Stop and hold',home:'Home',projects:'Projects',attention:'Attention',settings:'Settings',appearance:'Appearance',cadence:'Activity history',notifications:'Notifications',agents:'Agents',services:'Services',about:'About'};
const trail=renderer.entries();
const viewState=require('./tray-view-state.js').create(view,{menu,document,send,defer,cancel,trail,routes:Object.fromEntries(Object.keys(routes).map(page=>[page,{parent:['appearance','cadence','notifications','agents','services','about'].includes(page)?'settings':'home'}]))});




// Task answers: the native bridge owns credentials and canonical dispatch/readback.




// A status refresh that reads the question past an answer op's revision ends Sending: reconcile once with the same op,
// so the CLI returns its first receipt (accepted) or names another writer (stale); the refresh itself never accepts.
// ponytail: fallback actions (#1021) keep Check delivery; their revision fence refuses another revision.




// one task per board and issue: `issues` names provider, host and repo (M2)






// an incomplete or stale snapshot has unknown counts, never zero (§ 4)




// service error and stale status.json are not failed tasks (M-A10)






// bins, range and since come from Rust (the icon draws the same); unknown is never drawn or counted as zero (M6)




// `Update now` only for a newer version in status.json; success only once a status.json reads it back (M9)

// AI requests (menu.md M7): one input, `↶` history and commands

// every value the command gets is shown before confirm

// R6 item 7 `Next: 1 <action> ★ · 2 <action>` as the last line: numbered 1..n in order, at most one ★; else null (raw text only)


// Add project (menu.md M10): the folder comes from the OS picker; four cases
const addKinds={new:'New project: no taskq.json',plain:'Not a repository: files stay',taskq:'taskq project: selftest passes',old:'Old taskq project: selftest fails'};
const cutover='https://github.com/taskqorg/taskq/blob/main/taskq.md#9-config-and-cutover';

// the command over all projects names each one and its effect before acting (M5, M-A4)










// Capacity (taskq.md § 4): the shared resource controls (docs/design/shared-resource-controls.md) over the tray's
// taskq.resources/1 projection; this adapter only maps an operation onto `service capacity` and its receipt
const {workPageFor}=execution.module('tray.operations',{workPageFor:p=>p.resources?.targets?.some(t=>t.supported?.some(s=>s.policy_schema==='taskq.host-capacity/1'))?h('div',{'data-resources':p.root}):h('p',{role:'status'},'Legacy capacity editor unavailable; refresh host status.')});
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






const startOperations=require('./workspace-answers.js').create(view,{menu,document,send,draw:present,defer,cancel});
const {answerSend}=startOperations;
return {...api,resourceSend};
}
});


module.exports={create,createState};
