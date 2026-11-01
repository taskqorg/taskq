'use strict';
const ui=require('./ui-runtime.js');
const execution=ui.execution;
const shell=require('./app-shell.js');
const {h,Fragment}=require('preact');
const {icon,button}=require('./ui-icons.js');
const syncView=require('./sync-view.js');

const {createState,create}=execution.module("tray",{
createState:function(snapshot=null){return {dashboard:null,popup:null,manualSize:null,agentqRuntime:null,agentqError:null,aiRoot:null,addConfirm:false,resourceNotice:null,persistenceError:null,toast:null,receivedAt:0,S:snapshot,page:'home',taskFilter:null,working:null,workCard:null,ask:null,noted:null,pending:{},hostCommand:null,recovery:{},confirming:null,updating:null,draft:null,disclosures:{},pressedAt:null,stale:false,resizing:null};},
create:function(view,ports){
const api=execution.module("tray.create",{

operationLauncher:function(operation,root,label){return h('button',{class:'project','data-a':'workspace','data-page':'operations','data-origin':ports.settings?view.page:undefined,'data-operation':operation,'data-root':root||null},label);},
operationTarget:function(d){return ({work:'resources',confirm:'admission','pause-all':'admission','resume-all':'admission',pause:'admission',resume:'admission',focus:'admission',pick:'admission',add:'setup','add-ask':'setup','add-install':'setup','add-setup':'setup','add-cancel':'setup','stop-review':'stop',stop:'stop','stop-full':'stop',restart:'service',uninstall:'service',update:'update','recovery-read':'recovery','recovery-apply':'recovery','recovery-reconcile':'recovery','recovery-pending':'recovery'})[d.a];},
operationBody:function(context){
  const c=context||{},ps=view.S.projects,p=ps.find(p=>p.root===c.root),host=view.S.host||'unknown';
  if(c.root&&!p)return f(actionNotices(),h('p',{role:'status'},'Project unavailable; no operation can be applied. Return to Messages or Refresh.'));
  const selected=p?[p]:ps;
  const heading=f(h('p',{class:'meta'},'Host '+host+' · '+(p?p.name:selected.length+' registered projects')),h('details',{'data-view-key':'operation-targets'},h('summary',null,'Targets'),selected.map(p=>h('div',{class:'body meta'},p.name+' · '+p.root))));
  let body;
  switch(c.operation){
   case 'service':body=[h('button',{class:'project','data-a':'resources'},'Open resource monitor'),view.resourceNotice&&h('p',{role:'status'},view.resourceNotice.reason),api.hostService(),p?[service(p,false),capacity(p),workEntry(p.root)]:ps.map(p=>service(p,false))];break;
   case 'resources':{const target=p||ps.find(p=>p.resources);body=target?[h('p',null,'Host limits affect every registered project; project ceilings remain read-only.'),api.workPage(target.root)]:h('p',{role:'status'},'Capacity unavailable: no supported host projection.');break;}
   case 'stop':body=view.S.stop_modes?api.stopPanel():h('p',{role:'status'},'Emergency stop unavailable on this host. Normal stop is under Services.');break;
   case 'recovery':body=view.S.stop_modes?api.recoveryPanel():h('p',{role:'status'},'Reviewed recovery unavailable on this host.');break;
   case 'admission':{const selected=p?[p]:ps;body=[h('p',{class:'body',role:'status'},api.admissionText(selected)),h('p',null,'Pause holds new admission; current tasks finish. Resume restores saved focus and cannot clear an emergency hold.'),selected.map(p=>h('section',{key:p.root},h('p',null,p.name+' · '+modeText(p)),admission(p))),!p&&bulk(selected.filter(p=>mode(p)==='run'),selected.filter(p=>mode(p)==='paused'))];break;}
   case 'update':body=[h('p',null,'Request the advertised update on this host. Running work is preserved; success requires the version read back from service status.'),update()];break;
   case 'agentq':body=[api.agentq(true)];break;
   case 'setup':body=[h('p',null,'Choose a folder using the native picker. Opening or leaving this review registers nothing. Setup must confirm account, repository, visibility and exact external effects under § 6 before applying them.'),addProject(),setup()];break;
   default:body=h('p',{role:'status'},'Operation unavailable; no effect was submitted.');
  }
  return f(heading,actionNotices(),body);
},
operationAction:function(d){
  if(d.a==='agentq-runtime'){view.agentqRuntime=d.runtime;present();return true;}
  if(d.a==='agentq-dismiss'||d.a==='agentq-check'){send({a:d.a});return true;}
  if(d.a==='agentq-setup'){
    const A=view.S.agentq,rows=A?.runtimes||[],runtime=view.agentqRuntime||rows.find(r=>r.eligible)?.runtime;
    if(!rows.some(r=>r.runtime===runtime&&r.eligible))return true;
    const id=Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');
    send({a:d.a,id,runtime});present();return true;
  }
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
fit:function(){if(ports.workspace||ports.settings||ports.views)return;const size=view.manualSize||view.S.size,fixed=(view.S.size_anchor||(size?'fixed':'top'))==='fixed',w=view.page==='recovery'?Math.min(720,window.screen?.availWidth||720):size?size[0]:view.S.font>=18?360:320;document.body.style.width='100%';menu.style.width=w+'px';send({a:'size',d:window.devicePixelRatio,g:fixed&&size?0:gutter(),w,h:fixed&&size?size[1]:natural()});menu.style.width='100%';},
actionReceipt:function(r){if(!view.S?.projects.some(p=>p.root===r.root))return;actionErrors.set(r.root,{text:r.text});present();},
actionNotices:function(){if(!ports.workspace)return view.workspaceError?h('p',{role:'alert'},view.workspaceError):'';const r=view.hostCommand||view.S?.lifecycle;const alert=text=>h('div',{class:'body alert',role:'alert'},text);
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
    const current=view.S.service?.state==='running'&&typeof p.service_pid==='number'&&typeof p.host_instance==='string'&&p.alive===true&&p.service_pid===view.S.service?.pid&&p.host_instance===view.S.service?.instance;
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
  const scope=action==='uninstall'?p.name:'all registered projects: '+(targets?projects.map(p=>p.name).join(', ')||'none registered':'unknown');
  return h('details',{class:'host-'+action,'data-view-key':p.root+':'+action},h('summary',null,icon({restart:'rotate-cw',stop:'square',uninstall:'trash-2'}[action]),title+' '+(action==='uninstall'?p.name:'host service')),h('div',{class:'body meta'},'Affects '+scope+'. '+effect),h('details',{'data-view-key':p.root+':'+action+':targets'},h('summary',null,'Exact targets'),targets&&targets.map(t=>h('div',{class:'body meta'},t))),!review?.instance&&h('div',{class:'body alert'},review?.reason||title+' protocol unavailable'),h('button',{class:'project','data-a':action,'data-root':p.root,disabled:!!(pending||!review?.instance||!targets)},'Confirm '+(action==='uninstall'?'uninstall '+p.name:'host '+action)),pending&&r&&api.lifecycleWaits(),(r?.id||r?.reason)&&h('div',{class:'body meta',role:'status'},title+' '+(r.id||'')+': '+(r.state||'unknown')+(r.reason?' · '+r.reason:'')));
},
stopPanel:function(){if(!ports.workspace)return api.operationLauncher('stop',null,'Review Stop and hold…');
  const projects=view.S.projects||[],root=projects[0]?.root,held=view.S.stop_hold;
  const pending=!!held&&!held.selected_count||view.hostCommand&&view.hostCommand.state!=='observed'||['queued','applied'].includes(view.S.lifecycle?.state);
  const names=projects.length+' registered projects';
  const host=projects[0]?.host||'unknown host',command=view.hostCommand||view.S.lifecycle;
  const rows=projects.map(p=>h('div',{class:'body'},h('strong',null,p.name),h('div',{class:'meta'},view.S.service?.state!=='running'||p.alive!==true||p.service_pid!==view.S.service?.pid||p.host_instance!==view.S.service?.instance?'Process state unknown: service observation stale or from another generation.':Array.isArray(p.stop_progress)&&p.stop_progress.length?lines(p.stop_progress.map(k=>'#'+k.task+' '+k.rt+': '+k.state+(k.reason?' · '+k.reason:''))):'No stop acknowledgement; running or unknown work may remain.')));
  return f(h('div',{class:'body'},'Stop on '+host+' · '+names),h('div',{class:'body meta'},'Only TaskQ-owned local work is targeted. Remote hosts and unrelated agents are unaffected. Dirty files and session references are retained; unsaved buffers and interrupted external effects may be lost. Automatic rollback to older releases is disabled.'),
    h('div',{class:'body'},h('strong',null,'Drain'),h('div',{class:'meta'},'Hold new admission and let current work finish. Retain autostart and the hold after service exit.')),h('button',{class:'project','data-a':'stop','data-root':root||'',disabled:!!(pending||!root||!view.S.stop_review?.instance)},'Confirm Drain'),
    h('div',{class:'body'},h('strong',null,'Full stop'),h('div',{class:'meta'},'Request cancellation now, including offline. Unknown termination stays fenced. Board reconciliation may keep the service alive.')),h('button',{class:'project','data-a':'stop-full','data-root':root||'',disabled:!!(pending||!root||!view.S.full_stop_review?.instance)},'Confirm Full stop'),
    !view.S.full_stop_review?.instance&&h('div',{class:'body alert'},view.S.full_stop_review?.reason||'Full stop capability unknown'),
    held&&h('div',{class:'body alert',role:'status'},'Admission held: '+(held.mode||'unknown')+'; ordinary Play cannot clear this hold.'),held&&h('button',{class:'project','data-route':'recovery'},'Review recovery'),command?.action==='stop'&&h('div',{class:'body meta',role:'status'},'Stop '+(command.id||'')+': '+(command.state||'unknown')+(command.reason?' · '+command.reason:'')),h('details',{'data-view-key':'stop-projects'},api.disclosure('folder','Project observations'),rows),api.lifecycleWaits());
},
recoveryPanel:function(){if(!ports.workspace)return api.operationLauncher('recovery',null,'Review local task recovery…');
  return f(h('div',{class:'body'},'Selected local tasks only · Keep held by default. Unknown effects must remain held.'),h('details',{'data-view-key':'recovery-scope'},h('summary',null,'Recovery scope and effects'),h('div',{class:'body meta'},'Paused projects, other tasks and operators stay held. Inspect native release, work and external effects before confirming. Restart discards the old result and starts from preserved files.')),view.S.projects.map(p=>{
    const item=view.recovery[p.root],review=item?.review;
    const read=item?.pending?'Reading…':review&&item.readAt?'Read at '+when(item.readAt):item?.notice?'Outcome unknown':'Not read';
    const next=item?.pending?'wait for the receipt':!review?'read current recovery':review.rows.some(r=>r.eligible&&!r.selected)?'select tasks, record effects, then confirm':'nothing eligible; work stays held';
    return h('section',{class:'group','data-recovery-project':p.root,key:p.root},h('div',{class:'body'},h('strong',null,p.name),h('div',{class:'meta',role:'status'},read+' · Next: '+next)),h('button',{'data-a':'recovery-read','data-root':p.root,disabled:!!item?.pending},'Read current recovery'),item?.notice&&h('div',{class:'body alert',role:'status'},item.notice),review&&[
      h('div',{class:'body meta'},review.mode+' · '+review.rows.length+' tasks · '+review.rows.filter(r=>r.eligible&&!r.selected).length+' eligible'),h('details',{'data-view-key':'recovery-policy:'+p.root},h('summary',null,'Focus, limits and access'),h('div',{class:'body meta'},'Focus '+((review.focus||[]).join(', ')||'off')),
      h('div',{class:'body meta'},'Limits: '+(review.limits?.unknown||Object.entries(review.limits?.limits||{}).map(([k,v])=>k+' '+v).join(', ')||'unknown'),h('br'),'Access: '+((review.access||[]).map(a=>a.runtime+': '+(a.refused||a.effective||'unknown')).join('; ')||'unknown'))),
      review.rows.map(r=>{
        const draft=item.drafts?.[r.task]||{},blocked=!r.eligible||r.selected||item.pending||(review.pending||[]).some(q=>q.rows.some(s=>s.task===r.task));
        return h('div',{class:'body',key:r.task},h('label',null,h('input',{type:'checkbox','data-recovery-task':r.task,'data-root':p.root,checked:!!draft.selected,disabled:!!blocked}),' #'+r.task+' '+(r.title||'Task')),h('div',{class:'meta'},(r.runtime||'unknown runtime')+' · '+(r.host||'unknown host')+' · '+(r.native_exit||'unknown'),h('br'),r.workspace?.state||'work unknown',r.blocker&&[h('br'),r.blocker],r.selected&&[h('br'),'Selected admission confirmed; running state is separate.']),h('details',{'data-view-key':'recovery-execution:'+p.root+':'+r.task},h('summary',null,'Execution details'),h('div',{class:'body meta'},'Execution '+(r.execution??'unknown')+' · revision '+(r.revision??'unknown')),h('a',{href:r.url||'#','data-url':r.url||''},'Inspect task and receipts ↗')),h('label',{class:'meta recovery-evidence'},'Reconciled effect outcomes',h('input',{type:'text',maxlength:'160','data-recovery-evidence':r.task,'data-root':p.root,'aria-label':'Effect evidence for task '+r.task,value:draft.evidence||'',disabled:!!blocked})));
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
  if(r.state==='review'&&Array.isArray(r.rows)){item.review=r;item.drafts={};item.readAt=view.S.now;item.notice='Review current evidence; nothing is selected.';}
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
    if(item?.receipt&&p.alive===true&&ack?.state==='observed'&&ack.applied_receipt===item.receipt&&typeof ack.observed_receipt==='string'&&/^[a-f0-9]{32}$/.test(ack.observed_receipt)){
      delete view.pending[p.root];if(actionErrors.get(p.root)?.id===item.id)actionErrors.delete(p.root);
    }
  }
  for(const root of Object.keys(view.pending))if(!view.S.projects.some(p=>p.root===root))delete view.pending[root];
},
remember:function(){return {...viewState.capture(),workCard:view.workCard};},
navigate:function(next,filter=null,work=null){
  view.popup=null;
  if(next==='notifications'){send({a:'workspace',open:{page:'conversations',filter:'new'}});return;}
  // R22: from the menu, Settings opens or raises its own window; plain Settings keeps that window's section
  if(view.settingsWindow&&!ports.settings&&settingsPages.includes(next)){viewState.flush();send({a:'settings-open',page:next==='settings'?null:next});return;}
  if(ports.settings&&next==='home')next='settings';
  if(!ports.workspace&&(work||next==='stop'||next==='recovery'&&!ports.settings)){send({a:'workspace',open:{page:'operations',operation:work?'resources':next,root:work?Object.keys(work)[0]||null:null}});return;}
  renderer.push(remember());view.page=next;view.taskFilter=filter===null?null:Array.isArray(filter)?filter:[filter];view.working=work;view.workCard=null;view.ask=null;view.noted=null;view.disclosures={};view.restoreScroll=0;
  view.unavailable=null;
  present(true);(menu.querySelector('.content')||document.scrollingElement).scrollTop=0;
  menu.querySelector('h1')?.focus();
},
back:function(){
  const prior=renderer.pop()||{page:ports.settings||settingsPages.includes(view.page)&&view.page!=='settings'?'settings':view.S?.projects.some(p=>p.root===view.page)?view.taskFilter||view.working?view.page:'projects':'home',taskFilter:null,working:null,workCard:null,scroll:0,open:{}};
  ({page:view.page,taskFilter:view.taskFilter,working:view.working,workCard:view.workCard}=prior);view.noted=null;view.disclosures=prior.open;view.aiRoot=prior.aiRoot;view.restoreScroll=prior.scroll;
  present(true);(menu.querySelector('.content')||document.scrollingElement).scrollTop=prior.scroll;
  viewState.focus(prior.focus);
},
routeButton:function(route,label=routes[route]){return h('button',{class:'project','data-route':route},label);},
answerSlot:function(t,compact){const label=t.state==='ask'?'Open question':'View agent';return h('button',{'data-a':'workspace','data-page':'answer','data-root':t.root||'','data-board':t.key.slice(0,t.key.lastIndexOf('/')),'data-task':t.n,'data-revision':t.revision,...(compact&&{class:'compact-action',title:label+' #'+t.n,'aria-label':label+' #'+t.n})},compact?icon('messages-square'):[icon(t.state==='ask'?'messages-square':'terminal'),label]);},
answerNotices:function(board){return startOperations.answerNotices(board);},
answerToast:function(){return startOperations.answerToast();},
tasks:function(p){return p.tasks.map(t=>({...t,root:p.root,project:p.name,boardIdentity:require('./task-views.js').identity(p),key:p.issues+'/'+t.n,url:t.url||p.issues+'/'+t.n}));},
blockProject:function(p){return {id:p.root,name:p.name,board:{url:p.issues,kind:p.board_kind},counts:p.counts,partial:p.partial,tasks:tasks(p)};},
// #1592: the display model marks every repeat registration of a board task `dup`; rows come in model order.
unique:function(ts){return ts.filter(t=>!t.dup).sort((a,b)=>require('./task-views.js').compare(a,b));},
mode:function(p){return p.mode;},
counter:function(v,kind,label){return h('span',{class:'counter '+(v>0?kind+' filled':'zero'),'aria-label':label+': '+v,title:label},v);},
known:function(){return !view.S.display.all.partial.length;},
// O1: identity anchors details/actions; title remains a full-width issue link.
taskRow:function(t,global){const p=view.S.projects.find(p=>p.root===t.root);return h('div',{class:'task-row','data-card':t.key,tabindex:-1,key:t.key},raw(TaskqProjectBlocks.task(blockProject(p),t,global,true)),api.syncLine(t.sync),dashboard?.attention(t));},
// #1819: a row's shared pending operation; only a failed one offers Retry, which resends the same operation.
syncLine:function(o){return syncView.row(o)||'';},
staleLine:function(){return syncView.staleRow(view.S)||'';},
// #1819: the snapshot as received, with the shared pending store laid over it.
adopt:function(s){view.S=s;},
taskOrder:function(a,b){return require('./task-views.js').compare(a,b);},
startButton:function(rows){
  const held=rows.filter(t=>t.state==='held');
  if(!held.length)return '';
  const label=held.length===1&&rows.length===1?'Start #'+held[0].n:'Start '+held.length+' held';
  return h('button',{'data-a':'start','data-targets':JSON.stringify(held.map(({key,revision})=>({key,revision}))),disabled:held.some(api.startPending),title:label,'aria-label':label},icon('play'),h('span',null,label));
},
startPending:function(t){return startOperations.startPending(t.key.slice(0,t.key.lastIndexOf('/')),t);},
startHeld:function(targets){
  const live=unique(view.S.projects.flatMap(tasks)),rows=targets.map(x=>live.find(t=>t.key===x.key&&t.revision===x.revision&&t.state==='held'));
  if(!rows.length||rows.some(t=>!t||api.startPending(t)||!Number.isSafeInteger(t.revision)||!view.S.projects.some(p=>p.root===t.root&&p.alive===true&&p.complete===true&&p.tasks_live!==false)))return false;
  for(const t of rows)ui.effect('task.start',()=>answerSend(t.key.slice(0,t.key.lastIndexOf('/')),{id:'tray.start.'+t.n+'.'+t.revision,task:t.n,revision:t.revision,action:'start'}),()=>{});
  return true;
},
globalGroup:function(st,ts){ts=ts.filter(t=>t.state===st);const name=ts[0]?.state_name;return ts.length?h('details',{class:'group','data-view-key':'state:'+st,key:'state:'+st},h('summary',null,h('span',{class:color(st)},name),counter(ts.length,color(st),name)),ts.map(t=>taskRow(t,true))):'';},
problem:function(p){const e=!p.alive?(p.age==null?'no status.json: service not running':'status.json '+p.age_text+' old: service not running?'):p.error?'service error: '+p.error:'';return e?h('button',{class:'project alert','data-a':'log','data-root':p.root,title:e,disabled:!p.log},h('span',null,icon('circle-alert'),p.name+': '+e),h('span',{class:'meta'},p.log?'Log':''),h('span')):'';},
modeText:function(p){return {run:'Configured intake: '+(Array.isArray(p.focus)?p.focus.join(' ')||'off':'unknown'),paused:'Paused: running tasks finish, no new ones',watch:'Observation only: focus off'}[mode(p)];},
admission:function(p){if(!ports.workspace)return api.operationLauncher('admission',p.root,'Review admission in '+p.name+'…');
  if(view.pending[p.root]&&!view.pending[p.root].unknown)return h('button',{class:'project',disabled:true},view.pending[p.root].text+'…');
  if(mode(p)==='run')return h('button',{class:'project','data-a':'pause','data-root':p.root},icon('pause'),'Pause admission in '+p.name);
  if(p.saved)return h('button',{class:'project','data-a':'resume','data-root':p.root},icon('play'),'Resume admission in '+p.name);
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
service:function(p,host=true){return h('details',{class:'group service','data-view-key':'service:'+p.root},api.disclosure('activity',[host?'Service ':'Project ',h('span',{class:'meta'},p.name+(host?'':' · '+(p.age==null?'status unknown':p.age_text+' old')))]),host&&api.hostService(),incident(p),
  p.log?h('button',{class:'project','data-a':'log','data-root':p.root},icon('file-text'),'Open log'):h('div',{class:'body meta'},'Log path unknown'),
  h('button',{class:'project','data-a':'folder','data-root':p.root},'Open folder'),
  ports.workspace?[api.lifecycleControl(p),api.lifecycleControl(p,'stop'),api.lifecycleControl(p,'uninstall')]:api.operationLauncher('service',p.root,'Review service operations…'));},
hostService:function(){const s=view.S?.service;return h('div',{class:'body meta',role:'status'},'Host service: '+(s?.state==='running'?'running · PID '+s.pid+(s.fresh?' · fresh observation':' · stale observation'):s?.state==='exited'?'recorded process exited; worker outcomes separate':'unknown')+(s?.reason?': '+s.reason:''));},
when:function(t,tz){return new Date(t*1000).toLocaleString([],{timeZone:tz==='UTC'?'UTC':undefined,dateStyle:'short',timeStyle:'short'})+' '+(tz==='UTC'?'UTC':zone);},
local:function(t,tz){const d=new Date(t*1000);return (tz==='UTC'?d.toISOString():new Date(d-d.getTimezoneOffset()*60000).toISOString()).slice(0,16);},
custom:function(){
  const d=view.draft;
  return f(h('div',{class:'range'},h('label',{for:'hs'},'Start'),h('input',{id:'hs',type:'datetime-local','data-draft':'start',value:d.start}),h('label',{for:'he'},'End'),h('input',{id:'he',type:'datetime-local','data-draft':'end',value:d.end}),h('label',{for:'hz'},'Time zone'),h('select',{id:'hz','data-draft':'tz'},[['local','Local ('+zone+')'],['UTC','UTC']].map(([v,l])=>h('option',{value:v,selected:d.tz===v},l)))),
  d.error&&h('div',{class:'body alert',role:'alert'},d.error),h('button',{class:'project','data-a':'range'},'Show [start, end)'));},
history:function(){
  const H=view.S.history,rows=H.bins,n=rows.length;
  const kinds=['completed','new','ask','failed','blocked'],colors=['work','new','ask','failed','ready'];
  const values=b=>[b.counts[0],b.incoming??0,b.counts[2],b.counts[1],b.counts[3]],missing=(b,k)=>b.missing.length||(k===1&&(b.incoming_missing?.length??1));
  const identity=b=>b.start+'/'+b.end,stamp=t=>new Date(t*1000).toLocaleString([],{timeZone:H.tz==='UTC'?'UTC':undefined,year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit',...(Number.isInteger(t)?{}:{fractionalSecondDigits:3})})+' '+(H.tz==='UTC'?'UTC':zone),quantity=(count,missing)=>missing?(count?'≥'+count:'? (0 observed)'):String(count);
  const selected=Math.max(0,rows.findIndex(b=>identity(b)===view.historyBin));
  const incomplete=rows.some(b=>b.missing.length||b.incoming_missing?.length),tot=kinds.map((_,k)=>rows.reduce((a,b)=>a+values(b)[k],0)),partial=kinds.map((_,k)=>rows.some(b=>missing(b,k)));
  const detail=b=>'['+stamp(b.start)+', '+stamp(b.end)+') · '+kinds.map((k,i)=>k+' '+quantity(values(b)[i],missing(b,i))).join(', ')+(b.ongoing?' · ongoing':'')+(b.missing.length||b.incoming_missing?.length?' · incomplete: '+[...b.missing,...(b.incoming_missing||[])].join(', '):'');
  return h('details',{class:'group hist','data-view-key':'history'},api.disclosure('chart-no-axes-column',['History ',h('span',{class:'meta'},view.S.display.periods.find(x=>x.id===view.S.period)?.name)]),
    h('div',{class:'options periods'},view.S.display.periods.map(({id:k,name:l})=>h('button',{'data-set':'period','data-v':k,'aria-pressed':String(view.S.period===k||k==='custom'&&view.draft!=null)},l))),
    view.draft&&custom(),h('div',{class:'body meta'},when(H.start,H.tz)+' – '+when(H.end,H.tz)),
    h('div',{class:'history-legend body meta'},kinds.map((k,i)=>h('span',null,h('i',{class:colors[i]}),k+' '+quantity(tot[i],partial[i])))),
    h('div',{class:'body meta history-label'},'Events · oldest → newest'),
    h('div',{class:'history-plot'},raw(H.svg?.replace(/viewBox="0 0 [^"]+"/,'viewBox="1 10.5 20 11" preserveAspectRatio="none"')),
      h('div',{class:'history-targets',role:'group','aria-label':'Event history; six equal bins'},rows.map((b,i)=>h('button',{'data-history-bin':identity(b),'aria-label':detail(b),'aria-pressed':String(i===selected),title:detail(b)})))),
    h('div',{class:'body meta history-detail',role:'status'},rows[selected]?when(rows[selected].start,H.tz)+' – '+when(rows[selected].end,H.tz)+' · '+kinds.map((k,i)=>k+' '+quantity(values(rows[selected])[i],missing(rows[selected],i))).join(' · ')+(rows[selected].ongoing?' · ongoing':''):'History unavailable'),
    incomplete&&h('div',{class:'body meta'},'Incomplete · observed lower bounds'),h('details',{'data-view-key':'history-source'},api.disclosure('info','Source and exact interval'),h('div',{class:'body meta history-exact'},rows[selected]?detail(rows[selected]):'History unavailable'),
      h('div',{class:'body meta history-sources'},'Sources: '+(view.S.projects.map(p=>p.name+(p.since!=null?' events since '+stamp(p.since):' unknown: '+(p.events_error||'no history'))+(p.incoming?.schema===1?' · New coverage '+(p.incoming.start==null?'unknown':stamp(p.incoming.start)+' – '+stamp(p.incoming.end)):' · New not collected')).join('; ')||'none registered')),h('div',{class:'body meta'},'Counts are events, not current tasks. Peak '+(H.peak??'unknown')+' events (Completed + available New + Ask + Failed). Counts are not unique-task productivity. Collection start does not prove uninterrupted collection.')));},
update:function(){if(!ports.workspace)return api.operationLauncher('update',null,'Review updates…');
  const up=view.S.projects.find(p=>p.update&&p.update!==p.version);
  if(view.updating&&view.S.projects.some(p=>p.root===view.updating.root&&p.alive===true&&p.version===view.updating.v))return h('div',{class:'row meta'},'Updated to '+view.updating.v+': read from status.json');
  if(view.updating&&view.S.now-view.updating.at<300)return h('button',{class:'project',disabled:true},'Updating to '+view.updating.v+'…');
  return f(view.updating&&h('div',{class:'row alert'},'Update to '+view.updating.v+' not confirmed: see the log'),!view.updating&&up&&h('button',{class:'project','data-a':'update','data-root':up.root,'data-v':up.update},'Update now to '+up.update));},
ai:function(){return h('button',{class:'project','data-a':'workspace','data-page':'conversations','data-fresh':'true','data-root':view.aiRoot||null},'Ask AI · new session…');},
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
const state=p=>({run:pausing?'pause':'admitting, unchanged',paused:pausing?'paused, unchanged':'resume',watch:'observation, unchanged'}[mode(p)]);
  if(view.confirming)return f(h('div',{class:'body',role:'alert'},pausing?'Pause admission? Running tasks finish.':'Resume admission?',h('br'),lines(view.S.projects.map(p=>p.name+': '+state(p)))),h('div',{class:'options'},h('button',{'data-a':pausing?'pause-all':'resume-all'},'Confirm'),h('button',{'data-a':'cancel'},'Cancel')));
  return run.length?h('button',{class:'project','data-a':'confirm'},'Pause all…'):held.length?h('button',{class:'project','data-a':'confirm'},'Resume all…'):'';},
views:function(){return h('div',{class:'options views'},[['icon','Icon'],['full','Summary']].map(([v,l])=>h('button',{'data-set':'view','data-v':v,'aria-pressed':String(view.S.view===v)},v==='full'?h('span',{class:'summary-preview'},raw(view.S.icons?.full)):view.S.icons?.icon&&h('img',{alt:'',src:view.S.icons.icon}),l)));},
appearance:function(){
  const reduced=window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches,background=view.S.opacity_mode==='background';
  // each choice previews itself: the text size at its own size, the icon and the growth direction
  return f(api.heading('Text size · '+view.S.font+' px','type'),h('div',{class:'body meta'},'Theme follows system appearance.'),h('div',{class:'options'},[14,16,18,20].map(n=>h('button',{'data-set':'font','data-v':n,'aria-pressed':String(n===view.S.font),style:'font-size:'+n+'px'},n))),
    api.heading('Tray icon','panels-top-left'),views(),api.heading('Window transparency','sliders-horizontal'),h('div',{class:'options'},[['whole','Whole window'],['background','Background only']].map(([v,label])=>h('button',{'data-set':'opacity_mode','data-v':v,'aria-pressed':String((view.S.opacity_mode||'whole')===v)},label))),
    background?f(h('div',{class:'body opacity-setting'},h('label',{for:'background-opacity'},'Background opacity · '+(view.S.background_opacity??100)+'%'),h('input',{id:'background-opacity',type:'range',min:0,max:100,step:1,value:view.S.background_opacity??100,'data-opacity':'background_opacity','aria-valuetext':(view.S.background_opacity??100)+'% opaque'})),
      h('div',{class:'body meta'},'The task filter background stays opaque. Other fills fade; text and controls stay opaque. At 0%, the desktop shows through.')):
      f(h('div',{class:'body opacity-setting'},h('label',{for:'menu-opacity'},'Menu opacity · '+(view.S.opacity??100)+'%'),h('button',{'data-set':'opacity','data-v':100,'aria-pressed':String((view.S.opacity??100)===100)},'Off'),h('input',{id:'menu-opacity',type:'range',min:60,max:100,step:1,value:view.S.opacity??100,'data-opacity':'opacity','aria-valuetext':(view.S.opacity??100)===100?'Off · 100% opaque':view.S.opacity+'% opaque'})),
      h('div',{class:'body meta'},'Whole window: 60–100% opaque, including text and controls.')),api.heading('Window','maximize-2'),api.windowSettings(),reduced&&h('div',{class:'body meta',role:'status'},'Reduced motion: growth previews show a still outline and arrow.'));

},
agentq:function(settings=false){
  const A=view.S.agentq;if(!A)return '';
  if(A.kind==='running')return settings?h('div',{class:'body',role:'status'},'agentq running · '+(A.version||'version unknown')):'';
  if(!settings&&A.kind==='shown'&&A.dismissed&&!view.agentqError)return '';
  const rows=Array.isArray(A.runtimes)?A.runtimes:[],runtime=view.agentqRuntime||rows.find(r=>r.eligible)?.runtime,selected=rows.find(r=>r.runtime===runtime),targets=A.targets?.[runtime]||[];
  // #1702 A2: off or unreadable AgentQ is a status with its reason, not a setup prompt; taskq works without it
  if(A.kind==='stopped'||A.kind==='unknown')return h('section',{class:'group agentq','aria-label':'agentq '+A.kind},
    view.agentqError&&h('div',{class:'body alert',role:'alert'},view.agentqError),
    h('div',{class:'row'},A.kind==='stopped'?'agentq off or unreachable':'agentq status unknown'),
    h('div',{class:'body meta'},A.host+' · '+A.platform),
    A.reason&&h('div',{class:'body',role:'status'},A.reason),
    h('div',{class:'body meta'},'taskq runs without agentq. Start or repair it from agentq itself; nothing is installed or changed here.'),
    h('div',{class:'options'},h('button',{'data-a':'agentq-check'},'Check again')));
  const session=A.setup?.url&&/^(https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+|codex:\/\/threads\/[A-Za-z0-9-]+)$/.test(A.setup.url)?h('a',{href:'#','data-url':A.setup.url},'Open session'):null;
  return h('section',{class:'group agentq','aria-label':A.kind==='shown'?'agentq is not installed':'agentq '+A.kind},
    view.agentqError&&h('div',{class:'body alert',role:'alert'},view.agentqError),
    h('div',{class:'row'},A.kind==='shown'?(settings?'agentq not installed':'Save on repeated context'):A.kind==='setting-up'?'Setting up agentq…':A.kind==='stopped'?'agentq installed · service stopped':'agentq '+(A.kind==='unknown'?'status unknown':A.kind)),
    h('div',{class:'body meta'},A.host+' · '+A.platform),
    A.reason&&h('div',{class:'body',role:'status'},A.reason),
    A.setup&&h('div',{class:'body',role:'status'},'Setup · '+A.setup.kind,A.setup.reason&&' · '+A.setup.reason,A.setup.next&&' · '+A.setup.next,' ',session),
    A.kind==='shown'&&h('div',{class:'body'},'agentq can reduce repeated context and preserve cache reuse, which may lower model costs. Savings depend on your client and workload; no saving is guaranteed.'),
    h('div',{class:'options'},rows.map(r=>h('button',{'data-a':'agentq-runtime','data-runtime':r.runtime,disabled:!r.eligible,'aria-pressed':String(runtime===r.runtime)},r.client))),
    rows.filter(r=>!r.eligible).map(r=>h('div',{class:'body meta'},r.client+' · '+(r.reason||'unknown'))),
    selected&&h('div',{class:'body'},'Set up agentq on this host for '+selected.client+' using '+runtime+'. This installs its verified release, engines and host service, and connects this client’s supported configuration, hooks and MCP. Existing sessions keep running. New sessions may be needed to verify the connection.'),
    selected&&h('div',{class:'body meta'},'Approved targets: '+targets.join(', ')),
    h('div',{class:'options'},h('button',{'data-a':'agentq-setup',disabled:!selected?.eligible||targets.length!==2},settings?'Set up agentq…':'OK — set up agentq'),A.kind==='shown'&&!A.dismissed&&h('button',{'data-a':'agentq-dismiss'},'Not now'),h('button',{'data-a':'agentq-check'},'Check again')),
    !selected?.eligible&&h('div',{class:'body meta'},'Use an existing client: Install agentq from https://github.com/taskqorg/agentq and set it up for this client. No runtime or permissions are changed here.'));
},
agents:function(){
  const A=view.S.agents,local=view.S.projects.filter(p=>p.host===view.S.host),remote=[...new Set(view.S.projects.flatMap(p=>[p.host,...(Array.isArray(p.hosts)?p.hosts:[]).map(h=>h?.name),...(p.tasks||[]).flatMap(t=>(t.executions||[]).map(x=>x.host))]).filter(x=>x&&x!==view.S.host))].sort();
  const prior=r=>r.state==='signed-in'?(A.fresh?'Signed in':'Signed in at '+when(A.at)):signState[r.state]||'Unknown';
  // AS5 Checking: the service marked a probe in progress; the prior result stays in the detail with its age
  const label=r=>A.checking?'Checking':prior(r),checked=A?.at==null?'no earlier check':'checked '+A.age+' ago';
  const rows=A?.rows.length?A.rows.map(r=>h('details',{class:'group agent-signin','data-view-key':'agent:'+r.runtime},api.disclosure(r.runtime==='claude'?'claude':r.runtime==='antigravity'?'bot':'openai',[runtimeName(r.runtime)+' '+(r.version||'version unknown')+' · ',h('span',{class:!A.checking&&signAlert.includes(r.state)?'alert':''},label(r)),h('span',{class:'meta'},' · '+(r.account||'account unknown'))]),
    h('div',{class:'body meta'},lines([A.checking&&'Last result: '+prior(r),r.method&&'Method: '+r.method,r.expires?'Expires '+when(r.expires):'Expiry unknown',r.refreshed&&'Last refresh '+r.refreshed+'; not an expiry',r.reason&&'Reason: '+r.reason,checked].filter(Boolean))),
    r.next&&h('div',{class:'body'},'Sign in: '+r.next+'. A check never signs in.'))):[h('div',{class:'body meta'},A?.checking?'Checking: no earlier result':'Unknown: no status.json has agents')];
  return f(api.heading('Clients on this computer','bot'),h('div',{class:'row'},h('span',null,h('strong',null,view.S.host||'This host'),' · sign-in '+(A?(A.checking?'checking now · ':'')+checked:'unknown: no service probe yet'))),
    h('div',{class:'body meta'},'Tray '+(view.S.version||'version unknown')+' · service '+(grouped(local.map(p=>p.version||'version unknown'))||'none on this host')),rows,
    // #1702 A3: a remote host's only source here is its board heartbeat (freshness); client, sign-in, account and access read unknown with the missing source
    remote.length>0&&api.heading('Other computers','network'),remote.map(n=>{const beat=view.S.projects.flatMap(p=>Array.isArray(p.hosts)?p.hosts:[]).find(x=>x?.name===n);
      return h('details',{class:'group remote-agents','data-view-key':'agents:'+n},api.disclosure('network',n+' · '+(beat?.age_text?'heartbeat '+beat.age_text+' old':beat?'heartbeat age unknown':'heartbeat unknown: no host row')+' · clients unknown'),
        ['claude','codex','antigravity','codex-app'].map(rt=>h('div',{class:'body meta'},runtimeName(rt)+' · unknown: '+n+' heartbeat publishes no client version, sign-in, account or access')),
        h('div',{class:'body meta'},'Source: board heartbeat only; no runtime-client or access probe of '+n+' reaches this host.'));}),
    api.heading('Project access','shield'),api.accessFacts(),
    h('div',{class:'body meta'},'Editing is not available here: the operator changes grants with `taskq service permissions --runtime <runtime> --mode <mode>`.'),view.S.agentq&&api.heading('AgentQ','bot'),api.agentq(true));
},
// R29/R32: per project on this host, effective or refused access per runtime, or the one reason it is unknown
accessFacts:function(){
  const local=view.S.projects.filter(p=>p.host===view.S.host),runtimes=[...new Set(local.flatMap(p=>(p.access||[]).map(a=>a.runtime)))];
  return f(local.map(p=>h('section',{class:'access-project','data-access-project':p.root},h('div',{class:'body'},p.name),
    Array.isArray(p.access)&&p.access.length?h('div',{class:'access-grid',style:{'--access-columns':Math.max(1,runtimes.length)}},runtimes.map(rt=>{
      const a=p.access.find(a=>a.runtime===rt),state=!a?'unknown':a.refused?'refused':({'workspace-write':'Workspace write',workspace:'Workspace',full:'Full','read-only':'Read only'}[a.effective]||a.effective||'unknown');
      return h('details',{class:'access','data-view-key':'access:'+p.root+':'+rt},api.disclosure('shield',runtimeName(rt)+' · '+(p.alive!==true?'at capture: ':'')+state),
        h('div',{class:'body meta'},!a?'Unknown: no runtime access in status.json':runtimeName(rt)+': '+(a.refused?'refused: '+a.refused:'requested '+(a.requested||'unknown')+' · saved grant '+(a.grant||'none')+' · applied '+(a.effective||'unknown')+(a.enforcement?' · '+a.enforcement:''))));
    })):h('div',{class:'body meta'},'Access unknown for '+p.name+': no access in status.json'),
    h('div',{class:'body meta'},'Applied at the last service pass · '+(p.age==null?'age unknown':p.age_text+' old')))),
    !local.length&&h('div',{class:'body meta'},'Access unknown: no project on this host'));
},
about:function(){
  const S=view.S,up=S.projects.find(p=>p.update&&p.update!==p.version),versions=new Map();
  for(const p of S.projects)versions.set(p.version||'version unknown',[...(versions.get(p.version||'version unknown')||[]),p]);
  // R31: equal versions form one group; installed, running and available versions stay apart
  return f(api.heading('Versions and updates','info'),h('div',{class:'body'},'Tray installed '+(S.version||'version unknown')+' · '+S.host),[...versions].map(([v,ps])=>h('details',{class:'group','data-view-key':'version:'+v},api.disclosure('info',['Service '+v,h('span',{class:'meta'},' · '+ps.length+' project'+(ps.length===1?'':'s'))]),ps.map(p=>h('div',{class:'body meta'},p.name+' · '+(p.age==null?'status unknown':p.age_text+' old'))))),
    h('div',{class:'body meta',role:'status'},up?'Update available: '+up.update:S.projects.some(p=>p.alive===true)?'No newer version reported':'Update check unknown'),update());
},
setup:function(){
  const s=view.S.setup;if(!s||view.S.agentq?.setup?.id&&view.S.agentq.setup.id===s.id)return '';
  if(!ports.workspace)return f(h('div',{class:'body meta'},'Setup · '+(s.kind||'unknown')),api.operationLauncher('setup',null,'Review setup status…'));
  const link=s.url&&/^(https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+|codex:\/\/threads\/[A-Za-z0-9-]+)$/.test(s.url)&&[' ',h('a',{href:'#','data-url':s.url},'Open session')];
  return h('div',{class:'group'},h('div',{class:'row'},'Setup · '+s.kind,link),s.reason&&h('div',{class:'body meta'},s.reason),h('div',{class:'body'},s.next||'Setup state unknown; run `taskq service setup`'));
},
attention:function(){
  const all=unique(view.S.projects.flatMap(tasks));
  const parts=[api.agentq(),setup(),...view.S.projects.flatMap(p=>[problem(p),incident(p,true),incidentNotice(p)]),globalGroup('ask',all),globalGroup('failed',all),answerNotices()].filter(Boolean);
  return parts.length?f(parts):'';
},
projects:function(){
  return f(api.heading('Registered projects','folder'),view.S.projects.length?view.S.projects.map(p=>h('section',{class:'settings-project'},
    h('div',{class:'project-shortcuts'},h('button',{class:'project-overview','data-route':p.root,title:'Open '+p.name+' project overview'},p.name),h('div',{class:'project-actions'},
      p.issues?h('a',{class:'compact-action',href:p.issues,'data-url':p.issues,title:'Open '+p.name+' board','aria-label':'Open '+p.name+' board'},icon(p.board_kind)):button('link','Board unavailable: registered board missing',{disabled:true}),
      button('folder',p.root?'Open '+p.name+' project folder on this computer':'Project folder unavailable: registered root missing',{'data-a':'folder','data-root':p.root,disabled:!p.root,class:'compact-action'}))),
    (p.alive!==true||p.error||mode(p)!=='run')&&h('div',{class:'body meta'},p.error||(p.alive!==true?'Status unknown: '+(p.age==null?'no status.json':p.age_text+' old'):modeText(p)||'Intake unknown')),
    h('details',{'data-view-key':'project-settings:'+p.root},api.disclosure('focus','Intake details'),h('div',{class:'body meta'},modeText(p)||'Configured intake unknown'),h('div',{class:'body meta'},'You asked: '+(p.request||'not recorded')),h('div',{class:'body meta'},'Readback: '+(p.age==null?'status unknown':p.age_text+' old')),api.operationLauncher('admission',p.root,'Review project admission…')))):h('div',{class:'body meta'},'No projects yet'),addProject());
},
controls:function(){
  const run=view.S.projects.filter(p=>mode(p)==='run'),held=view.S.projects.filter(p=>mode(p)==='paused');
  return f(workEntry(),held.length>0&&run.length>0&&h('div',{class:'row meta'},'Paused '+held.length+' of '+(held.length+run.length)+': '+held.map(p=>p.name).join(', ')),bulk(run,held));
},
runningAgents:function(compact=false){
  // #1592: task counts and the board-task identity (`key`, one per board and number) come from the display model.
  const hosts=new Map(),all=view.S.display.all.counts;
  // #1775 A3/R16: an unread run, or a stale read beside a fresh one, marks only its hosts partial (confirmed minima); a host is
  // unknown while no fresh project read holds a live heartbeat for it, so a stale read never shows zero. `partial` stays for the board task totals.
  let partial=false;
  const sources=new Set(),seen=new Map();
  const add=(name,why)=>{if(!hosts.has(name))hosts.set(name,{name,age:null,age_text:null,live:false,reasons:new Set(),counts:new Map()});const host=hosts.get(name);if(why)host.reasons.add(why);return host;};
  for(const p of view.S.projects){
    const elapsed=Number.isFinite(p.age)&&p.age>=0&&Number.isFinite(p.tasks_age_seconds)&&p.tasks_age_seconds>=0?p.age+p.tasks_age_seconds:null;
    const fresh=p.complete===true&&p.tasks_live===true&&Number.isFinite(p.observed);
    const read=Array.isArray(p.tasks)&&p.tasks.every(t=>t?.executions==null||Array.isArray(t.executions));
    const stale=fresh&&read?null:(!read?'missing host/task data':!p.complete?'partial board snapshot':elapsed===null?'missing task observation':'stale task observation')+' ('+(p.name||p.root)+')'+(elapsed===null?' · age unknown':' · '+Math.round(elapsed)+'s old');
    partial ||= !fresh||!Array.isArray(p.hosts)||!Array.isArray(p.tasks);
    if(stale)sources.add(stale);
    if(!Array.isArray(p.hosts))sources.add((p.name||'Project')+': host observations missing');
    for(const h of Array.isArray(p.hosts)?p.hosts:[]){
      if(!h||typeof h.name!=='string'||!h.name){partial=true;continue;}
      // #1592: host age and its text come from the display model; #1775: the freshest observation names the host age.
      const age=Number.isFinite(h.age)?h.age:null,host=add(h.name,stale);
      host.beat=true;host.beatLive ||= h.live===true;host.live ||= h.live===true&&!stale;
      if(age!==null&&(host.age===null||age<host.age))Object.assign(host,{age,age_text:h.age_text});
    }
    for(const t of Array.isArray(p.tasks)?p.tasks:[]){
      if(t.executions!=null&&!Array.isArray(t.executions))partial=true;
      for(const x of Array.isArray(t.executions)?t.executions:[]){
        if(!x||typeof x.host!=='string'){partial=true;sources.add((p.name||'Project')+': invalid execution observation; host unknown');continue;}
        const h=add(x.host||'Unknown host',stale);
        // #1775 R16: a fresh ended or waiting execution is known and not running; only an unknown or stale one marks its host ?
        if(fresh&&['ended','waiting'].includes(x.state))continue;
        if(!fresh||x.live!==true||x.state!=='running'||!['worker','reviewer'].includes(x.role)||typeof x.runtime!=='string'||!x.runtime||!Number.isSafeInteger(x.id)||x.id<=0){h.reasons.add('run state unknown; see service log');continue;}
        if(!h.counts.has(x.runtime))h.counts.set(x.runtime,{worker:0,reviewer:0});
        const identity=JSON.stringify([t.key,x.id]),prior=seen.get(identity);
        if(prior){if(prior.host!==x.host||prior.runtime!==x.runtime||prior.role!==x.role){h.reasons.add('conflicting execution observations');hosts.get(prior.host).reasons.add('conflicting execution observations');}continue;}
        seen.set(identity,x);
        const counts=h.counts.get(x.runtime)||{worker:0,reviewer:0};counts[x.role]++;h.counts.set(x.runtime,counts);
      }
    }
  }
  // R16: this host is named even before an observation; unknown never reads as zero
  if(view.S.host&&!hosts.has(view.S.host))add(view.S.host,'no host observation');
  const local=view.S.projects.filter(p=>p.host===view.S.host),latest=local.filter(p=>p.capacity).sort((a,b)=>a.age-b.age)[0];
  const cap=latest?.capacity?.host_runtime_used,capFresh=latest?.alive===true,load=latest?.capacity;
  const occupied=cap&&typeof cap==='object'&&!Array.isArray(cap)&&Object.values(cap).every(n=>Number.isInteger(n)&&n>=0);
  // R17: CPU and memory with units and age; slots, reservations and board tasks stay apart
  const facts=[capFresh&&Number.isFinite(load?.cpu_percent)&&Number.isFinite(load?.memory_available_percent)&&latest.sample_age_text?'CPU '+Math.round(load.cpu_percent)+'% busy · memory '+Math.round(load.memory_available_percent)+'% available · sampled '+latest.sample_age_text+' ago':'CPU and memory: no sample; sampled only while the load gate is on'];
  if(capFresh&&occupied){
    facts.push('Occupied slots: '+(Object.entries(cap).map(([rt,n])=>rt+' '+n).join(' · ')||'0')+' · '+latest.age_text+' old');
    const reserved=new Map();let known=local.length>0;
    for(const p of local){
      if(p.alive!==true||p.complete!==true||p.tasks_live!==true||!Array.isArray(p.capacity?.reservations)){known=false;continue;}
      for(const r of p.capacity.reservations){
        if(!r||typeof r.rt!=='string'||!r.rt||!['held','requested'].includes(r.state)){known=false;continue;}
        const running=(p.tasks||[]).some(t=>t.n===r.task&&(Array.isArray(t.executions)?t.executions:[]).some(x=>x&&x.attempt===r.attempt&&x.host===view.S.host&&x.runtime===r.rt&&x.state==='running'&&x.live===true));
        if(!running)reserved.set(p.issues+'/'+r.task+'/'+r.attempt,r.rt);
      }
    }
    const counts=new Map();for(const rt of reserved.values())counts.set(rt,(counts.get(rt)||0)+1);
    facts.push('Reservations: '+(known?([...counts].map(([rt,n])=>rt+' '+n).join(' · ')||'none'):'unknown'));
  }else facts.push('Capacity unknown');
  const alerts=(view.S.agents?.rows||[]).filter(r=>signAlert.includes(r.state)&&r.state!=='check-failed');
  const totals=host=>{const t=new Map([['Claude',0],['Codex',0],['Antigravity',0]]);for(const [rt,c] of host.counts)t.set(family(rt),(t.get(family(rt))||0)+c.worker+c.reviewer);return t;};
  for(const host of hosts.values())if(!host.live)host.reasons.add(!host.beat?'host observation missing':host.beatLive?'current execution data unavailable':host.age===null?'heartbeat freshness unknown':'stale heartbeat; current runs unknown');
  const known=host=>host.live&&!host.reasons.has('conflicting execution observations');
  const sum=counts=>[...counts.values()].reduce((n,x)=>n+x,0);
  const value=(n,known,partial)=>!known||partial&&!n?'?':partial?'≥'+n:String(n);
  const mark=rt=>rt==='Codex'?'openai':rt==='Claude'?'claude':'bot';
  const runtime=(rt,n,known,partial)=>{const shown=value(n,known,partial),label=rt+': '+(shown==='?'?'unknown':partial?'at least '+n+'; additional agents unknown':n);
    return h('span',{class:'host-runtime','data-runtime':rt,title:label,'aria-label':label},icon(mark(rt)),!['Codex','Claude'].includes(rt)&&h('span',null,rt),h('span',{class:'host-number'},shown));};
  const out=[],sorted=[...hosts.values()].sort((a,b)=>a.name.localeCompare(b.name));
  const numberWidth=Math.max(3,...sorted.flatMap(host=>{const t=totals(host);return [sum(t),...t.values()].map(n=>value(n,known(host),host.reasons.size>0).length);}))+'ch';
  for(const host of sorted){
    const unknown=!known(host),here=host.name===view.S.host,age=host.age!==null&&host.age>=0&&host.age_text?' · '+host.age_text+' old':' · age unknown',why=[...host.reasons].join('; '),t=totals(host),partial=host.reasons.size>0,total=value(sum(t),!unknown,partial),warnings=here?alerts.map(r=>family(r.runtime)+' '+signState[r.state].toLowerCase()):[];
    const runtimes=[...host.counts].sort(([a],[b])=>a.localeCompare(b));
    out.push(h('details',{class:'host-line','data-view-key':'host:'+host.name,key:'host:'+host.name},
      h('summary',{class:'host-breakdown',title:'Heartbeat'+age},h('strong',{class:'host-name'},host.name),
        h('span',{class:'host-runtime-split'},['Codex','Claude','Antigravity',...[...t.keys()].filter(rt=>!['Codex','Claude','Antigravity'].includes(rt))].map(rt=>runtime(rt,t.get(rt)||0,!unknown,partial))),
        h('span',{class:'host-row-total','aria-label':'Total running agents: '+total},h('span',{class:'host-number'},total)),
        (unknown||partial||warnings.length>0)&&h('span',{class:'host-observation'},unknown?'Status unknown'+age:partial?'Partial observation'+age:'',why&&h('span',null,why),warnings.length>0&&h('span',{class:'host-warning'},'Sign-in: '+warnings.join(' · ')))),
      h('div',{class:'body meta'},'Heartbeat'+age),unknown?h('div',{class:'body meta'},'Unknown: '+(why||'host observation missing or stale')):why&&h('div',{class:'body meta'},'Partial: '+why),
      !unknown&&(runtimes.length?h('div',{class:'body'},runtimes.map(([rt,c])=>h('div',null,rt+': '+c.worker+' worker'+(c.worker===1?'':'s')+' · '+c.reviewer+' reviewer'+(c.reviewer===1?'':'s')))):h('div',{class:'body meta'},partial?'No confirmed running agents; additional agents unknown':'No running agents')),
      here&&api.resourceMeters(latest),h('details',{'data-view-key':'host-facts:'+host.name},api.disclosure('sliders-horizontal','Resource details'),(here?facts:['CPU and memory: not collected for remote hosts','Slots and reservations: unknown for remote hosts']).map(t=>h('div',{class:'body meta'},t)))));
  }
  // M3.1: only open-heartbeat hosts enter the strip; unobserved local and execution-only hosts stay in details.
  const strip=sorted.filter(host=>host.beat),names=strip.map(host=>host.name);
  // #1781: three letters, longer while another strip host shares the prefix (menu.md, #1729); the full name stays in the tooltip
  const short=new Map(names.map(n=>{let k=3;while(k<n.length&&names.some(o=>o!==n&&o.slice(0,k)===n.slice(0,k)))k++;return [n,n.slice(0,k)];}));
  const warn=host=>host.name===view.S.host?alerts.map(r=>family(r.runtime)+' '+signState[r.state].toLowerCase()):[];
  const text=host=>{const age=host.age!==null&&host.age>=0?host.age_text:'',busy=[...totals(host)].filter(([,n])=>n>0);
   return [host.name,...(!known(host)?['runs unknown',...(age?[age+' old']:[]),...(host.reasons.size?[[...host.reasons].join('; ')]:[])]:[...(busy.length?busy.map(([k,n])=>k+' '+n):[host.reasons.size?'no confirmed running agents':'idle']),...(host.reasons.size?['partial: '+[...host.reasons].join('; ')]:[])]),...warn(host)].join(' · ');};
  // UX30-06: host totals precede runtime totals; only nonconflicting fresh hosts establish a minimum.
  const overall=new Map([['Codex',0],['Claude',0],['Antigravity',0]]);let anyKnown=false,uncertain=partial||!sorted.length;
  for(const host of sorted){const t=totals(host);for(const rt of t.keys())if(!overall.has(rt))overall.set(rt,0);uncertain ||= !known(host)||host.reasons.size>0;if(!known(host))continue;anyKnown=true;for(const [rt,n] of t)overall.set(rt,overall.get(rt)+n);}
  const chip=host=>{const shown=value(sum(totals(host)),known(host),host.reasons.size>0),label=text(host)+' · Total running agents: '+shown,w=warn(host);
    return h('span',{class:'host-chip'+(!known(host)?' unknown':''),'data-host':host.name,role:'img','aria-label':label,title:label,key:host.name},h('strong',null,short.get(host.name)),h('span',{class:'host-number'},shown),
      !known(host)&&host.age_text&&h('span',{class:'meta'},host.age_text),w.length>0&&h('span',{class:'host-warning',title:'Sign-in: '+w.join(' · '),'aria-label':'Sign-in: '+w.join(' · ')},icon('circle-alert')));};
  const completeness=anyKnown&&[...overall.values()].some(n=>n>0)?'Partial':'Unknown';
  const explanation='All observed hosts · running agents'+(uncertain?' · '+completeness+': confirmed counts are lower bounds; additional agents unknown. '+[...sorted.filter(host=>!known(host)||host.reasons.size).map(host=>host.name+' · '+(host.age_text||'age unknown')+' · '+[...host.reasons].join('; ')),...sources].join('; '):' · complete fresh observations');
  if(compact)return {label:strip.map(text).join('; ')||'Host unknown',meta:strip.length+' hosts · '+(uncertain?'observations incomplete':'current observations'),strip:h('span',{class:'host-overview'},h('span',{class:'host-strip'},strip.length?strip.map(chip):h('span',{class:'host-chip unknown'},'Hosts ?')),h('span',{class:'host-runtime-totals',title:explanation,'aria-label':explanation},[...overall].map(([rt,n])=>runtime(rt,n,anyKnown,uncertain)),uncertain&&h('span',{class:'host-completeness'},completeness)))};
  return h('section',{class:'group running-agents',style:{'--host-number-width':numberWidth},'aria-label':'Running agents'},h('div',{class:'host-grid-head','aria-label':'Host, Codex, Claude, Antigravity, Total running agents'},h('span',null,'Host'),['Codex','Claude','Antigravity','Total'].map(rt=>h('span',{title:rt,'aria-label':rt,role:'img'},icon(mark(rt))))),out,h('div',{class:'body meta'},'Registered project boards · '+(partial||all.some(c=>c.n===null)?'Tasks unknown':'Tasks: '+all.filter(c=>['doing','review'].includes(c.state)).map(c=>c.name+' '+c.n).join(' · '))),
    h('details',{'data-view-key':'run-definitions'},api.disclosure('info','Count definitions'),h('div',{class:'body meta'},'Running processes, occupied slots, reservations and board tasks are counted apart. ≥ means a confirmed minimum; Partial means additional agents are unknown; ? means no confirmed total. Reservations include pending or unconfirmed work; occupied slots can include other projects.')));
},
home:function(){
  const faults=[api.agentq(),setup(),...view.S.projects.flatMap(p=>[problem(p),incident(p,true),incidentNotice(p)])].filter(Boolean);
  const line=api.runningAgents(true);
  return f(api.staleLine(),h('details',{class:'dashboard-status','data-view-key':'dashboard-status'},h('summary',null,line.strip,faults.length>0&&h('span',{class:'attention',role:'img','aria-label':'Attention · '+faults.length,title:'Attention · '+faults.length},icon('circle-alert'),String(faults.length))),api.runningAgents(),controls(),faults),dashboard.list());
},
routeBody:function(){
  switch(view.page){
    case 'agentq-shown':case 'agentq-setting-up':case 'agentq-running':case 'agentq-unknown':return api.agentq(view.page==='agentq-running');
    case 'projects':return projects();
    case 'attention':return attention()||h('div',{class:'body meta'},known()?'No reported questions or failures':'Status incomplete; check Services');
    case 'settings':return api.settings();
    case 'hosts':return f(api.heading('Hosts and resources','network'),api.runningAgents());
    case 'delivery':return f(h('p',{class:'meta'},'Delivery is project policy, separate from Access. Exact-candidate approval and publication remain on the board.'),view.S.projects.map(p=>h('details',{class:'group','data-view-key':'delivery:'+p.root},api.disclosure('git-pull-request',[p.name+' · ',h('span',{class:'meta'},p.delivery?.preset||'policy unavailable')]),h('div',{class:'body'},({'auto-merge':'After review and required checks, publish and merge automatically.','review-pr':'Open a pull request; the owner must approve its exact candidate before publication.','direct':'Publish to main after review and required checks, using the board’s direct route (a fast-forward MR on GitLab).'})[p.delivery?.preset]||'Delivery policy unavailable; no publication authority is inferred.'),h('details',{'data-view-key':'delivery-policy:'+p.root},api.disclosure('info','Policy details'),h('div',{class:'body meta'},'Revision '+(p.delivery?.revision??'unknown'))),p.issues&&h('a',{class:'project',href:p.issues,'data-url':p.issues},'Open Board'))));
    case 'appearance':return appearance();
    case 'cadence':return history();
    case 'notifications':return h('button',{'data-a':'workspace','data-page':'conversations','data-filter':'new'},'Open New in Conversations');
    case 'notification-settings':return notifications();
    case 'agents':return agents();
    case 'work':return f(h('div',{class:'body',role:'status'},api.admissionText(view.S.projects,true)),api.balancingSections());
    case 'stop':return api.stopPanel();
    case 'recovery':return f(api.heading('Hold on '+(view.S.host||'unknown host'),'rotate-ccw'),h('div',{class:'body',role:'status'},view.S.stop_hold?'Admission held · '+(view.S.stop_hold.mode||'mode unknown')+(view.S.stop_hold.selected_count!=null?' · '+view.S.stop_hold.selected_count+' selected':''):'No emergency hold reported'),api.hostService(),h('div',{class:'body meta'},'Recovery is for selected local tasks. Ordinary Resume cannot clear an emergency hold; unknown effects stay held.'),api.recoveryPanel());
    case 'services':return f(api.heading(view.S.host||'This host','activity'),api.hostService(),view.S.projects.map(p=>problem(p)),api.operationLauncher('service',null,'Host maintenance in workspace…'),h('details',{'data-view-key':'service-projects'},api.disclosure('folder','Project observations · '+view.S.projects.length),view.S.projects.map(p=>[incidentNotice(p),service(p,false)])),view.S.agentq&&api.heading('AgentQ','bot'),view.S.agentq&&api.agentq(true));
    case 'about':return about();
    default:return home();
  }
},
resourceGroup:function(p){return {...p.resources,key:p.root,labels:{host:p.cap_host||'this host',project:p.name}};},
resourceCapability:function(g){const p=view.S&&view.S.projects.find(p=>p.root===g.key),e=p&&p.cap_saved&&p.cap_saved.error;return e||!p?{edit:false,reason:'Read-only here',note:e?'Config: '+e:null}:p.resources?.targets?.some(t=>t.supported?.some(s=>s.policy_schema==='taskq.host-capacity/1'))?{edit:require('./resource-controls.js').fields(g).length>0,reason:'Editing unavailable here',note:'Host limits affect all registered projects. Project ceilings are read-only here.'}:{edit:false,reason:'Legacy editor unavailable; refresh host status'};},

resourceReceipt:function(m){
  const key=m.root+'#'+m.id,r=resourceRequests.get(key);
  if(!r)return;
  cancel(r.timer);resourceRequests.delete(key);
  const receipt=['saved','rejected','unknown'].includes(m.state)&&(m.state!=='saved'||typeof m.saved_revision==='string'&&/^[a-f0-9]{32}$/.test(m.saved_revision))?{state:m.state,reason:m.reason,saved_revision:m.saved_revision}:{state:'unknown',reason:'Invalid capacity receipt'};
  r.resolve(receipt);
  defer(fit,0);},
capacity:function(p){if(!ports.workspace)return p.resources?api.operationLauncher('resources',p.root,'Review capacity…'):'';
  if(!p.resources)return '';
  return h('details',{class:'group','data-view-key':'capacity:'+p.root},h('summary',null,icon('sliders-horizontal'),'Capacity ',h('span',{class:'meta'},(p.cap_host||'this host')+' · '+require('./resource-controls.js').headline(resourceGroup(p),view.S.now))),h('div',{'data-resources':p.root}));},
workEntry:function(root){return view.S.projects.some(p=>p.resources)?h('button',{class:'project','data-a':'work','data-root':root||null},icon('focus'),'Balancing…'):'';},
balancingSections:function(hostControls,projectControls){
      const availability=p=>p.capacity?.reasons?.filter(r=>['usage-limit','login','capability','access'].includes(r.code)).map(r=>r.text||r.code).join(' · ')||(p.alive===true&&Array.isArray(p.capacity?.reasons)?'':'Availability unknown');
      const exceptions=view.S.projects.filter(p=>availability(p));
      return f(
      api.heading('Task routing','focus'),h('div',{class:'body meta'},'run-* pins stay strict. prefer-* and project routing are soft preferences; eligible subscription CLIs provide fallback.'),
      h('details',{'data-view-key':'balancing-routing'},api.disclosure('focus','Project routing · '+view.S.projects.length+' projects'),view.S.projects.map(p=>h('div',{class:'body meta'},p.name+' · '+(Array.isArray(p.routing)?p.routing.length?p.routing.map(r=>r.labels.join(' + ')+' → '+r.prefer).join('; '):'Host default':'Routing unknown')))),
      api.heading('Runtime availability','bot'),exceptions.length?exceptions.map(p=>h('div',{class:'body meta'},p.name+' · '+availability(p))):h('div',{class:'body meta'},'No reported availability blockers · '+view.S.projects.length+' projects'),h('details',{'data-view-key':'balancing-access'},api.disclosure('info','Runtime access details'),api.accessFacts()),
      api.heading('Host balancing','focus'),h('div',{class:'body meta'},'Host '+(view.S.host||'unknown')+' · limits and load gate affect all registered projects. Quota holds never change configured limits. Current tasks continue.'),hostControls||(view.S.projects.some(p=>p.resources)?api.operationLauncher('resources',null,'Review host limits…'):h('div',{class:'body meta'},'Capacity unavailable: no supported host projection.')),
      api.heading('Project policies','folder'),projectControls,api.operationLauncher('admission',null,'Review project admission…'),
      h('details',{'data-view-key':'settings-admission'},api.disclosure('focus','Project admission details'),h('div',{class:'body meta'},api.admissionText()),view.S.projects.map(p=>h('div',{class:'body meta'},p.name+' · '+(modeText(p)||'Intake unknown')+' · '+(p.capacity?.reasons?.map(r=>r.text||r.code).join(' · ')||(p.alive===true&&Array.isArray(p.capacity?.reasons)?'No reported blockers':'Admission observation unknown'))))));
},
workPage:function(root){
  const ps=view.S.projects.filter(p=>p.resources),p=ps.find(p=>p.root===root||view.working?.[p.root])||ps[0],run=view.S.projects.filter(p=>mode(p)==='run'),held=view.S.projects.filter(p=>mode(p)==='paused');
  const editor=ports.workspace?f(api.resourceMeters(p),p?.resources.targets?.some(t=>t.supported?.some(s=>s.policy_schema==='taskq.host-capacity/1'))?h('div',{'data-resources':p.root}):h('div',{'data-work':''})):api.operationLauncher('resources',Object.keys(view.working||{})[0]||null,'Review host limits in workspace…');
  return f(h('div',{class:'body',role:'status'},'Admission · '+api.admissionText()),api.balancingSections(editor,bulk(run,held)));
},
admissionText:function(ps=view.S.projects,compact=false){
  if(!ps.length)return 'No projects';
  // R32: capacity reasons describe observed work in this project, not every runtime on the host.
  const rows=ps.map(p=>{
    const c=p.capacity,reasons=Array.isArray(c?.reasons)?c.reasons:null;
    const unknown=p.alive!==true||!reasons||!['open','off','held'].includes(c?.gate)||c.gate==='held'&&!reasons.length;
    return {name:p.name,state:unknown?'unknown':reasons.length?'blocked':'clear',text:unknown?'observation unknown':reasons.length?[...new Set(reasons.map(r=>r.code||'unknown'))].join(', '):'no reported blockers'};
  });
  if(compact)return ps.length+' projects · '+(rows.every(r=>r.state==='clear')?'no reported blockers':[['blocked','blocked'],['unknown','unknown']].map(([state,label])=>[rows.filter(r=>r.state===state).length,label]).filter(([n])=>n).map(([n,label])=>n+' '+label).join(' · '));
  const states=new Set(rows.map(r=>r.state)),lead=states.has('unknown')?'Unknown':states.size>1?'Mixed':states.has('blocked')?'Blocked work':'No reported blockers';
  const intake=[['run','configured'],['paused','paused'],['watch','observation']].map(([m,l])=>[l,ps.filter(p=>mode(p)===m).length]).filter(([,n])=>n).map(([l,n])=>n+' '+l);
  const missing=ps.filter(p=>!['run','paused','watch'].includes(mode(p))).length;if(missing)intake.push(missing+' unknown');
  const same=rows.every(r=>r.text===rows[0].text);
  const scope=same&&rows.length>1?'all '+rows.length+' projects'+(rows[0].state==='blocked'?' ('+rows[0].text+')':''):rows.map(r=>r.name+' ('+r.text+')').join(' · ');
  return lead+': '+scope+' · Intake: '+intake.join(' · ');
},
resourceMeters:function(p){
  const c=p?.capacity,known=p?.alive===true&&!!p.sample_age_text&&!c?.reasons?.some(r=>r.code==='sample');
  return h('div',{class:'resource-meters'},[['CPU busy',c?.cpu_percent],['Memory available',c?.memory_available_percent]].map(([label,value])=>{
    const valid=known&&Number.isFinite(value)&&value>=0&&value<=100;
    return h('div',{class:'resource-meter'},h('span',null,label),valid&&h('meter',{min:0,max:100,value,'aria-label':label}),h('span',null,valid?Math.round(value)+'%':'Unknown'));
  }),h('small',{class:'meta'},known?'Sample '+p.sample_age_text+' ago':'No current sample'));
},
mountResources:function(){
  const ws=menu.querySelector('[data-work]');
  if(ws){
    if(!view.workCard){const el=document.createElement('div');view.workCard={el,ui:require('./resource-controls.js').mountWork(el,{capability:resourceCapability,send:resourceSend,now:()=>view.S.now,host:view.S.host},view.working)};}
    if(ws.firstElementChild!==view.workCard.el)ws.replaceChildren(view.workCard.el);
    view.workCard.ui.update(view.S.projects.filter(p=>p.resources).map(resourceGroup));
  }
  for(const slot of menu.querySelectorAll('[data-resources]')){
    const p=view.S.projects.find(p=>p.root===slot.dataset.resources);
    if(!p)continue;
    const scope=p.resources.targets?.[0]?.supported?.[0]?.policy_schema;
    let card=resourceCards.get(p.root);
    if(!card||card.scope!==scope){card?.el.remove();const el=document.createElement('div');card={el,scope,ui:require('./resource-controls.js').mount(el,{capability:resourceCapability,send:resourceSend,now:()=>view.S.now})};resourceCards.set(p.root,card);}
    if(card.el.parentNode!==slot)slot.append(card.el);
    card.ui.update(resourceGroup(p));
  }
},
notifications:function(){
  const N=view.S.notify,st=N.status,line={allowed:'Allowed by the OS',blocked:'Blocked by the OS: nothing is posted','not asked':'Not asked yet: the first banner setting asks the OS',asking:'Asked: answer the OS prompt','request failed':'OS permission request failed: '+N.failure,unknown:'OS permission unknown',unsupported:'Not available on this host'}[st]||'OS permission unknown';
  return f(api.heading('OS permission','shield'),h('div',{class:'body'+(st==='blocked'||st==='request failed'?' alert':' meta'),role:'status'},line),st==='blocked'&&h('button',{class:'project','data-a':'notify-os'},'Open OS setting ↗'),
    api.heading('Per event on this host','bell'),h('p',{class:'meta notification-legend'},'Off: no OS delivery · Silent: no OS banner · Banner: OS banner · Sound: banner with sound. History is independent; banners and sound need OS permission.'),
    view.S.display.notify.map(x=>api.notifyRow(x,null,N.host[x.id],st)),
    api.heading('Project overrides','folder'),view.S.projects.map(p=>api.notifyGroup(p)));
},
notifyRow:function(event,root,value,status){
  const choices=[...(root?[['inherit','Inherit']]:[]),...modes.map(([v,label])=>[v,v==='sound'?'Sound':label])];
  return h('div',{class:'notification-row'},h('label',null,h('span',null,event.name),h('select',{'data-notify':event.id,'data-root':root||null,value:value||'unknown','aria-label':event.name+(root?' for '+(view.S.projects.find(p=>p.root===root)?.name||'project'):' on this host')},
    !choices.some(([v])=>v===value)&&h('option',{value:'unknown',disabled:true},'Unknown'),choices.map(([v,label])=>h('option',{value:v,selected:v===value},label)))),!root&&h('button',{'data-a':'notify-test','data-kind':event.id,disabled:status!=='allowed',title:'Send '+event.name+' test notification'},'Test'));
},
notifyGroup:function(p){
  const N=view.S.notify?.projects?.[p.root];
  if(!N)return h('div',{class:'body meta'},p.name+': notification overrides unknown; no project readback');
  const name=v=>modes.find(m=>m[0]===v)?.[1]||'Unknown';
  return h('details',{class:'group','data-view-key':'notifications:'+p.root},api.disclosure('bell',[p.name,' · ',h('span',{class:'meta'},view.S.display.notify.some(x=>!N[x.id])?'Overrides unknown':view.S.display.notify.every(x=>!N[x.id][0])?'Inherit host':view.S.display.notify.filter(x=>N[x.id][0]).length+' override'+(view.S.display.notify.filter(x=>N[x.id][0]).length===1?'':'s'))]),
    view.S.display.notify.map(x=>f(api.notifyRow(x,p.root,N[x.id]?N[x.id][0]||'inherit':'unknown',view.S.notify.status),h('div',{class:'meta'},'Effective: '+name(N[x.id]?.[1])))));
},
openCard:function(o){const p=view.S?.projects.find(p=>p.root===o.root);if(p)send({a:'workspace',open:{page:'conversations',root:p.root,board:p.issues,task:o.n,...(o.sha?{sha:o.sha}:{}),...(o.id?{event:o.id}:{})}});},
project:function(p){
  const ts=tasks(p),counters=TaskqProjectBlocks.counters(p.counts,p.root,p.name);
  const groups=view.S.display.states.map(({id:st,name})=>{const r=ts.filter(t=>t.state===st);return r.length?h('div',{class:'group',key:st},h('h2',{class:'section-heading task-group-heading'},h('span',null,name),counter(r.length,color(st),name),r.length>1&&startButton(r)),r.map(t=>taskRow(t))):'';}).filter(Boolean);
  return f(h('section',{class:'tqp',dangerouslySetInnerHTML:{__html:counters}}),h('div',{class:'body meta'},modeText(p)),problem(p),incident(p,true),incidentNotice(p),
  groups.length?groups:h('div',{class:'body muted'},'No open tasks'),answerNotices(p.issues),
  h('div',{class:'group'},admission(p)),capacity(p),notifyGroup(p),workEntry(p.root),service(p));},
filteredTasks:function(p){if(p&&!view.dashboard){view.dashboard=require('./task-views.js').normalize(null,{taskFilter:view.taskFilter});require('./task-views.js').current(view.dashboard).working.projects=[require('./task-views.js').identity(p)];}return dashboard.list();},
heading:function(label,mark){return h('h2',{class:'section-heading'},icon(mark),label);},
disclosure:function(mark,children){return h('summary',{class:'settings-disclosure'},icon(mark),h('span',{class:'disclosure-label'},children));},
stickyFilters:function(){const content=menu.querySelector('.content'),row=content?.querySelector('.filter-header'),host=content?.querySelector('.dashboard-status>summary'),height=host?.getBoundingClientRect().height||0;if(content){content.style.setProperty('--host-strip-height',height+'px');content.style.scrollPaddingTop=row||host?height+(row?.getBoundingClientRect().height||0)+'px':'';}if(row)row.classList.toggle('pinned',content.scrollTop>0&&row.getBoundingClientRect().top<=content.getBoundingClientRect().top+height+1);},
settings:function(){
  const S=view.S,ps=S.projects,A=S.agents,st=S.notify?.status,svc=S.service?.state,up=ps.find(p=>p.update&&p.update!==p.version);
  const growth={auto:'Auto',top:'Down',bottom:'Up',fixed:'Fixed'}[S.size_anchor||'auto']||'Auto';
  const widget=(target,mark,label,meta)=>h('button',{class:'project settings-widget',...target},mark,h('span',null,label),h('span',{class:'meta'},meta));
  const sections=[['Display',[
    widget({'data-route':'appearance'},icon('type'),'Appearance',S.font+' px · '+(S.view==='full'?'Summary':'Icon')+' · growth '+growth),
    widget({'data-route':'notification-settings'},icon('bell'),'Notifications',{allowed:'OS allowed',blocked:'OS blocked','not asked':'OS not asked',asking:'OS asking','request failed':'OS request failed',unsupported:'OS unsupported'}[st]||'OS permission unknown'),
    widget({'data-route':'cadence'},icon('chart-no-axes-column'),'Activity history',S.display?.periods?.find(x=>x.id===S.period)?.name||'period unknown')]],
   ['Host and projects',[
    widget({'data-route':'hosts'},icon('network'),'Hosts and resources',api.runningAgents(true).meta),
    widget({'data-route':'work'},icon('focus'),'Balancing',api.admissionText(ps,true)+(ps.some(p=>p.resources)?'':' · capacity unavailable')),
    widget({'data-route':'agents'},icon('bot'),'Agents and access',A?.checking?'checking sign-in':A?A.rows.map(r=>runtimeName(r.runtime)+' '+(r.state==='signed-in'&&!A.fresh?'signed in at capture':(signState[r.state]||'Unknown').toLowerCase())).join(' · ')||'no runtime':'sign-in unknown'),
    widget({'data-route':'projects'},icon('folder'),'Projects',ps.length+' registered'),
    widget({'data-route':'delivery'},icon('git-pull-request'),'Delivery',ps.length+' projects · '+new Set(ps.map(p=>p.delivery?.preset||'unavailable')).size+' '+(new Set(ps.map(p=>p.delivery?.preset||'unavailable')).size===1?'policy':'policies')),
    widget({'data-route':'services'},icon('activity'),'Services and support','host service '+(svc==='running'?(S.service.fresh?'running':'stale observation'):svc==='exited'?'exited':'unknown')),
    S.stop_modes&&widget({'data-a':'workspace','data-page':'operations','data-operation':'stop'},icon('octagon-x'),'Emergency stop','Drain or Full stop'),
    S.stop_modes&&widget({'data-route':'recovery'},icon('rotate-ccw'),'Recovery',S.stop_hold?'admission held':'no hold reported'),
    widget({'data-route':'about'},icon('info'),'About and updates',(S.version?'v'+S.version:'version unknown')+(up?' · update '+up.update:''))]]];
  return f(sections.map(([label,rows])=>h('section',{class:'settings-section'},api.heading(label,label==='Display'?'type':'network'),rows)));},
windowSettings:function(){const mode=view.S.size_anchor||'auto';return f(h('p',{class:'meta'},'Auto follows the actual tray opening direction. Dragging pins the window; explicit growth choices stay selected.'),h('fieldset',null,h('legend',null,'Window growth'),h('div',{class:'growth-options'},[['auto','Auto'],['top','Down'],['bottom','Up'],['fixed','Fixed']].map(([value,label])=>h('button',{'data-a':'size-anchor','data-value':value,'aria-pressed':String(mode===value)},h('span',{class:'growth-miniature','data-growth':value,'data-opening':view.S.auto_anchor==='bottom'?'up':'down','aria-hidden':'true'},h('i')),h('span',null,label))))),h('div',{class:'options'},button(view.S.pinned?'pin-off':'pin','Keep menu open',{'data-a':'pin','aria-pressed':String(!!view.S.pinned)}),button('always-on-top','Menu always on top',{'data-a':'top','aria-pressed':String(!!view.S.top)})));},
head:function(){
  const p=view.S.projects?.find(p=>p.root===view.page),title=view.working?'Balancing':p?p.name:routes[view.page]||'Unavailable';
  const crumbs=p?h('a',{href:'#','data-route':'projects'},'Projects'):settingsPages.includes(view.page)&&view.page!=='settings'?h('a',{href:'#','data-route':'settings'},'Settings'):'';
  // R07/A4: a stable action row; Close is always last, Keep open, Always on top and the task pin have distinct names
  if(ports.settings){const W=view.settingsChrome||{};return shell.bar({title,crumbs,home:'Settings home',back:view.page!=='settings'||renderer.entries().length>0,actions:[],windows:[button('always-on-top','Always on top',{'data-a':'settings-top','aria-pressed':String(!!W.top)}),button(W.maximized?'minimize-2':'maximize-2',W.maximized?'Restore window':'Enlarge window',{'data-a':'settings-enlarge','aria-pressed':String(!!W.maximized)}),button('x','Close settings',{'data-a':'hide'})]});}
  if(ports.views)return shell.bar({title:'View · '+dashboard.current().name,actions:[],windows:[button('x','Close view window',{'data-a':'hide'})]});
  // R09/MP12, #1699 A2: one Conversations entry opens the thread screen at every count, the first unread thread with new events; the count is deduplicated, unknown never reads as zero; opening acknowledges nothing
  const actions=[p?.issues&&h('a',{href:p.issues,'data-url':p.issues,title:'Open Board','aria-label':'Open '+p.name+' Board'},icon(p.board_kind)),view.S.stop_modes&&button('octagon-x','Emergency stop',{'data-a':'stop-review'}),notificationPanel.entry(),button('settings','Settings',{'data-route':'settings'})];
  const windows=[button(view.S.pinned?'pin-off':'pin','Keep menu open',{'data-a':'pin','aria-pressed':String(!!view.S.pinned)}),button('always-on-top','Always on top',{'data-a':'top','aria-pressed':String(!!view.S.top)}),button('x','Close menu',{'data-a':'hide'})];
  return shell.bar({title:view.page==='home'?'Tasks':title,crumbs,home:'Home',back:view.page!=='home'||renderer.entries().length>0||!!view.working,actions,windows});
},
draw:function(navigating=false){
  if(!view.S||view.resizing||ports.views&&!view.dashboard)return;
  const firstRender=!view.drawn;
  const active=document.activeElement,scroll=(menu.querySelector('.content')||document.scrollingElement).scrollTop;
  const bounds=new Map([...menu.querySelectorAll('[data-bound]')].map(e=>[e.dataset.bound,e.scrollTop]));
  const focusKey=!navigating&&active&&menu.contains(active)?JSON.stringify(active.dataset):null;
  // a refresh rebuilds the menu: refocus the same control, link or heading, by tag, data and position (U-A4)
  const same=e=>e.tagName===active.tagName&&JSON.stringify(e.dataset)===focusKey;
const focusAt=focusKey?[...menu.querySelectorAll(active.tagName)].filter(same).indexOf(active):-1;
  const focusedSummary=!navigating&&active?.tagName==='SUMMARY'?[...menu.querySelectorAll('summary')].indexOf(active):-1;
  const open=navigating||view.restoreScroll!=null?view.disclosures:viewState.groups(),p=view.S.projects.find(p=>p.root===view.page);
  if(!p&&!routes[view.page])view.page='home';
  if(ports.settings&&view.page==='home')view.page='settings';
  document.documentElement.style.colorScheme=view.S.theme||'light dark';
  menu.style.setProperty('--size',view.S.font+'px');
  const smart=!ports.workspace&&!ports.settings&&!ports.views&&view.S.opacity_mode==='background';
  const translucent=!ports.workspace&&!ports.settings&&!ports.views&&!smart&&(view.S.opacity??100)<100;
  for(const entry of ((translucent?view.S.opacity_palette:view.S.palette)||'').split(';')){const [key,value]=entry.split(':');if(key&&value)menu.style.setProperty(key,value);}
  menu.classList.toggle('translucent',translucent);
  menu.classList.toggle('background-only',smart);
  menu.style.setProperty('--background-alpha',(view.S.background_opacity??100)+'%');
  menu.classList.toggle('pinned',!!view.S.pinned);const edge=view.S.attached_edge||'top';if(menu.dataset.edge!==edge)menu.dataset.edge=edge;
  const settingsError=view.persistenceError||view.S.opacity_error||view.S.settings_error;
  const title=view.working?'Balancing':view.taskFilter?'Tasks':p?p.name:routes[view.page];
  // P-W4: the View window is one scroll of compact sections; Display, Save, Reset and Done stay in its footer.
  const tree=ports.views?[head(),h('div',{class:'content'},h('div',{class:'content-body view-window'},dashboard.presets(),dashboard.conditions())),h('footer',{class:'view-window view-window-footer'},dashboard.actions())]:[head(),h('div',{class:'content'},h('div',{class:'content-body'+(settingsPages.includes(view.page)||view.page==='projects'?' settings-content':'')},view.unavailable&&h('div',{class:'body',role:'status'},view.unavailable+' The surviving destination is '+title+'.'),actionNotices(),settingsError&&h('div',{class:'body alert',role:'alert'},'Settings unknown: '+settingsError+(view.persistenceError||view.S.opacity_error?'':'; defaults shown')),view.working?workPage():view.taskFilter?filteredTasks(p):p?project(p):routeBody())),h('footer',{class:'status-footer'},h('span',null,'TaskQ ',view.S.version?'v'+view.S.version:'version unknown'),h('div',{class:'grip','data-resize':'',role:'button','aria-label':'Resize window with arrow keys',title:'Drag to resize · arrow keys',tabindex:'0'})),dashboard.popup(),answerToast()];
  // #1430: Preact keeps unchanged nodes, so a refresh leaves the textarea being typed in attached.
  renderer.render(tree,p?'/projects/'+encodeURIComponent(p.issues)+(view.taskFilter?'/tasks':''):view.working?'/work':'/'+view.page);
  view.drawn=true;
  if(navigating)menu.querySelector('.shell-overflow')?.removeAttribute('open');
  mountResources();
  menu.querySelectorAll('details[data-view-key]:not(.shell-overflow)').forEach(d=>d.open=!d.classList.contains('answered')&&(view.page==='attention'||view.page==='cadence'&&d.classList.contains('hist')));
  viewState.restore(open);
  if(view.S.page!==undefined&&(navigating||firstRender))menu.querySelectorAll('.hist,.agents,.service,.incident-notice').forEach(d=>d.open=!(p&&d.classList.contains('service')));
  if(!navigating){const target=focusedSummary>=0?menu.querySelectorAll('summary')[focusedSummary]:focusKey&&active.tagName==='H1'?menu.querySelector('h1'):focusKey&&focusKey!=='{}'?[...menu.querySelectorAll(active.tagName)].filter(same)[focusAt]:null;target?.focus({preventScroll:true});}
  (menu.querySelector('.content')||document.scrollingElement).scrollTop=view.restoreScroll??scroll;view.restoreScroll=null;api.stickyFilters();
  if(firstRender)viewState.focus(view.restoreFocus);
  fit();dashboard.place();
  // after fit() sets the width: a new exchange or text size opens at the newest entry; a tick keeps the reader's place;
  // a block that drops a cut line asks for the smaller height
  const bound=[...menu.querySelectorAll('[data-bound]')];
  bound.forEach(c=>{c.scrollTop=bounds.has(c.dataset.bound)?bounds.get(c.dataset.bound):0;wholeLines(c);});
  if(bound.some(c=>c.style.maxHeight))fit();
  if(ports.views){const text=JSON.stringify(view.dashboard);if(text!==view.sentDashboard){view.sentDashboard=text;send({a:'views-dashboard',dashboard:view.dashboard});}}
  viewState.flush();},
wholeLines:function(c){
  if(c.scrollHeight<=c.clientHeight)return;
  const top=c.getBoundingClientRect().top+c.clientTop;
const lines=()=>{const r=document.createRange(),w=document.createTreeWalker(c,NodeFilter.SHOW_TEXT),a=[];
    for(let n;(n=w.nextNode());){r.selectNodeContents(n);a.push(...r.getClientRects());}return a;};
  const shrink=d=>c.style.maxHeight=c.offsetHeight-d+'px';
const cut=edge=>Math.min(...lines().filter(l=>l.top<edge&&l.bottom>edge).map(l=>l.top));
  const head=cut(top);
  if(head<top&&c.scrollTop+c.clientHeight<c.scrollHeight-1)c.scrollTop-=top-head;
  else if(head<top){const d=Math.min(...lines().filter(l=>l.top>=top).map(l=>l.top))-top;if(d<c.clientHeight){shrink(d);c.scrollTop+=d;}}
  const bottom=top+c.clientHeight,tail=cut(bottom);if(tail>top&&tail<bottom)shrink(bottom-tail);},
settle:function(c){if(!c.isConnected)return;const s=c.scrollTop;c.style.maxHeight='';c.scrollTop=s;wholeLines(c);fit();},
refresh:function(now){if(view.pressedAt!==null&&now-view.pressedAt<5000){view.stale=true;return;}tick();},
release:function(){view.pressedAt=null;if(view.stale){view.stale=false;defer(present);}},
batch:function(updates){
  batching++;let complete=false;
  try {
    const snapshot=updates.filter(update=>Array.isArray(update?.projects)).pop();
    if(snapshot)receive(snapshot);
    for(const update of updates)if(!Array.isArray(update?.projects))receive(update);
    complete=true;
  }
  finally {if(!--batching&&staged){const args=staged;staged=null;if(complete){if(view.pressedAt!==null&&performance.now()-view.pressedAt<5000)view.stale=true;else present(...args);}}}
},
receive:function(s){
  if(Array.isArray(s.updates)){api.batch(s.updates);return;}
  if(s.notificationReceipt){notificationPanel.receive(s.notificationReceipt);return;}
  if(s.reportReceipt){reports.receive(s.reportReceipt);return;}
  if(Object.hasOwn(s,'settingsWindow')){if(ports.settings)view.settingsChrome=s.settingsWindow;else{view.settingsWindow=s.settingsWindow===true;view.viewsWindow=s.viewsWindow===true;}if(view.S)present();return;}
  // P-W4: the menu adopts the View window's edits; the View window takes the menu's model when opened or while it is not being used.
  // ponytail: focus decides the writer, so edits made in both windows within one push can race; add a revision if that matters.
  if(s.dashboard){if(!ports.views)dashboard.adopt(s.dashboard);else if(s.initial||!document.hasFocus()){view.dashboard=require('./task-views.js').normalize(s.dashboard);view.sentDashboard=JSON.stringify(view.dashboard);present();}return;}
  if(s.settingsOpen){if(ports.settings&&view.S&&s.settingsOpen.page!==view.page)navigate(s.settingsOpen.page);return;}
  if(Object.hasOwn(s,'agentqError')){view.agentqError=s.agentqError;present();return;}
  if(ports.workspace&&s.projects){api.adopt(s);focusReadback();api.lifecycleReadback();present();return;}
  if(s.answerReceipt){startOperations.answerReceipt(s.answerReceipt);return;}
  if(s.replyFileReceipt){startOperations.answerFileReceipt(s.replyFileReceipt);return;}
  if(s.workspaceError!==undefined){view.workspaceError=s.workspaceError;present();return;}
  if(s.viewStateError!==undefined){view.persistenceError=s.viewStateError;present();return;}
  if(s.diagnose){const d=s.diagnose,remaining=d.until_ms-Date.now();
    if(d.id!==view.diagnosticId&&remaining>0){execution.diagnose(d.selector,Math.min(60000,remaining),d.limit);view.diagnosticId=d.id;}
    return;
  }
  if(s.aiError){view.aiSending=false;view.aiError=s.aiError;present();return;}
  // MP14: one board history page for the Completed view (tray `completed` read).
  if(s.completed){dashboard?.receive(s.completed);return;}
  if(!ports.workspace&&['action_error','focus_receipt','resources_receipt','capmsg','aiError'].some(k=>s[k]))return;
  if(s.action_error){actionReceipt(s.action_error);return;}
  if(s.focus_receipt){focusReceipt(s.focus_receipt);return;}
    if(s.resources_receipt){view.resourceNotice=s.resources_receipt;present();return;}
  if(s.capmsg){if(api.recoveryReceipt(s.capmsg))return;if(view.hostCommand?.id===s.capmsg.id)api.lifecycleReceipt(s.capmsg);else resourceReceipt(s.capmsg);return;}
  if(s.open){openCard(s.open);return;}
  if(view.S?.size_anchor!==s.size_anchor)view.manualSize=null;
  if(!ports.settings&&!ports.views)viewState.hydrate(s);reports.sync(s.projects||[]);api.adopt(s);view.receivedAt=performance.now();focusReadback();api.lifecycleReadback();
  // snapshot mode names the page
  if(s.page!==undefined&&s.page!==view.page){view.popup=null;view.page=s.page;view.taskFilter=null;view.working=null;view.workCard=null;renderer.clear();view.disclosures={};present(true);return;}
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
window.taskq={settle,receive,measure,model:{unique,mode}};
dashboard.mount();
ui.listen(menu,'toggle',()=>viewState.schedule(),true);
ui.listen(menu,'scroll',e=>{const c=e.target;if(c.classList?.contains('content')){viewState.schedule();api.stickyFilters();}if(!c.dataset?.bound)return;cancel(c.settling);c.settling=defer(()=>settle(c),150);},true);
// a refresh between press and release rebuilds the pressed control and its click is lost (U-A4):
// the refresh waits for the release, at most 5 s, then runs after the click

ui.listen(menu,'pointerdown',()=>{view.pressedAt=performance.now();});
for(const event of ['pointerup','pointercancel'])ui.listen(window,event,release,true);
ui.listen(window,'blur',release);
ui.listen(window,'resize',()=>present());
// Press end.

ui.listen(menu,'click',e=>{
  if(dashboard.click(e))return;
  const crumb=e.target.closest('a[data-route]');if(crumb){e.preventDefault();if(crumb.dataset.route!==view.page||view.working||view.taskFilter)navigate(crumb.dataset.route);return;}
  const cardLink=e.target.closest('.qac a');if(cardLink){e.preventDefault();send({a:'open',url:cardLink.getAttribute('href')});return;}
  if(e.target.closest('.qac,.rsc'))return;
  if(TaskqProjectBlocks.handle(e,{open:url=>send({a:'open',url}),project:id=>navigate(id),tasks:(id,state)=>{const c=dashboard.criteria();if(view.page!=='home'){c.projects=id?[require('./task-views.js').identity(view.S.projects.find(p=>p.root===id))]:[];c.states=[state];navigate('home');}else dashboard.set({states:c.states.includes(state)?c.states.filter(s=>s!==state):[...c.states,state]});}}))return;
  const a=e.target.closest('a[data-url]');if(a){e.preventDefault();send({a:'open',url:a.dataset.url});return;}
  const b=e.target.closest('button');if(!b||b.disabled)return;const d=b.dataset;
  if(notificationPanel.click(b))return;
  if(reports.click(b,view.S.projects))return;
  if(d.historyBin!==undefined){view.historyBin=d.historyBin;present();return;}
  if(!ports.workspace&&api.operationTarget(d)){send({a:'workspace',open:{page:'operations',operation:api.operationTarget(d),root:d.root||null,...(ports.settings?{origin:{surface:'settings',page:view.page}}:{})}});return;}
  if(d.a==='filter-reset'){view.taskFilter=[];present();return;}
  if(d.a==='all-tasks'){if(view.page==='home'&&view.taskFilter){view.taskFilter=[];present();}else navigate('home',[]);return;}
  if(d.a==='hide')viewState.flush();
  if(d.a==='home'){const root=ports.settings?'settings':'home';if(view.page!==root||view.taskFilter||view.working)navigate(root);return;}
  if(d.a==='workspace'){viewState.flush();send({a:'workspace',open:{page:d.page||'conversations',...(d.thread!==undefined?{thread:d.thread}:{}),...(d.filter?{filter:d.filter}:{}),operation:d.operation||null,...(ports.settings?{origin:{surface:'settings',page:view.page}}:{}),root:d.root||null,board:d.board||null,task:Number(d.task)||null,revision:Number(d.revision)||null,...(d.fresh==='true'?{fresh:true}:{})}});return;}
  if(d.route){if(d.route!==view.page||view.working||view.taskFilter)navigate(d.route);return;}
  if(d.a==='answer-dismiss'){startOperations.dismiss(d.key);return;}
  if(d.a==='toast-dismiss'){view.toast=null;present();return;}
  if(d.a==='control'){const t=unique(view.S.projects.flatMap(tasks)).find(t=>t.key===d.key);if(t&&(t.controls||[]).includes(d.control))ui.effect('task.control',()=>answerSend(t.key.slice(0,-String(t.n).length-1),TaskqTaskControls.op(t,d.control,'tray')),()=>{});present();return;}
  if(d.a==='start'){api.startHeld(JSON.parse(d.targets));present();return;}
  if(d.a==='answer-retry'){startOperations.retry(d.key);present();return;}
  if(d.a==='sync-retry'){send({a:'sync-retry',root:d.root,id:d.id});return;}
  if(d.a==='notify'||d.a==='notify-test'){send({a:d.a,kind:d.kind,value:d.v,root:d.root});return;}
  if(d.a==='work'){navigate(view.page,null,d.root?{[d.root]:true}:{});return;}
  if(d.a==='back'){back();return;}
  if(d.set==='period'&&d.v==='custom'){const H=view.S.history;view.draft={start:local(H.start,H.tz),end:local(H.end,H.tz),tz:H.tz,error:''};present();return;}
  if(d.set){view.S[d.set]=['font','opacity','background_opacity'].includes(d.set)?Number(d.v):d.v;if(d.set==='period')view.draft=null;send({a:'set',[d.set]:view.S[d.set]});present();return;}
  if(d.a==='range'){
    const t=v=>v?Date.parse(view.draft.tz==='UTC'?v+'Z':v)/1000:NaN;
const a=t(view.draft.start);
const z=t(view.draft.end);
    view.draft.error=!(a<z)?'Refused: start must be before end.':'';
    if(!view.draft.error){view.S.period='custom';send({a:'set',period:'custom',start:a,end:z,tz:view.draft.tz});view.draft=null;}
    present();return;}
  if(api.operationAction(d))return;
  if(d.a){actionErrors.delete(d.root);send({a:d.a,root:d.root,label:d.label,value:d.value});}present();});
ui.listen(menu,'change',e=>{if(e.target.matches('[data-notify]'))send({a:'notify',kind:e.target.dataset.notify,value:e.target.value,root:e.target.dataset.root});});
ui.listen(menu,'input',e=>{if(e.target.matches('[data-opacity]')){const key=e.target.dataset.opacity;view.S[key]=Number(e.target.value);send({a:'set',[key]:view.S[key]});present();return;}if(e.target.matches('[data-history-select]')){view.historyBin=e.target.value;present();return;}api.operationInput(e);const k=e.target.dataset.draft;if(k&&view.draft)view.draft[k]=e.target.value;});
ui.listen(menu,'focusout',e=>{const f=x=>x&&(x.id==='ai'||x.dataset&&(x.dataset.draft||x.dataset.rscField||x.dataset.recoveryEvidence));if(f(e.target))defer(()=>{if(!f(document.activeElement))present();});});
ui.listen(menu,'pointerdown',e=>{if(view.S&&e.button===0&&e.target.closest('.head')&&!e.target.closest('button,a,summary'))view.headerDrag={x:e.screenX,y:e.screenY};});
ui.listen(window,'pointermove',e=>{const start=view.headerDrag;if(start&&Math.hypot(e.screenX-start.x,e.screenY-start.y)>=4){view.headerDrag=null;send({a:'drag'});}});
for(const event of ['pointerup','pointercancel'])ui.listen(window,event,()=>{view.headerDrag=null;});
ui.listen(menu,'pointerdown',e=>{if(e.button!==0||!e.target.closest('.grip'))return;view.resizing={x:e.screenX,y:e.screenY,w:window.innerWidth,h:window.innerHeight};e.target.setPointerCapture(e.pointerId);e.preventDefault();});
ui.listen(menu,'pointermove',e=>{if(!view.resizing)return;view.manualSize=[Math.max(240,view.resizing.w+e.screenX-view.resizing.x),Math.max(120,view.resizing.h+e.screenY-view.resizing.y)];menu.style.width='100%';send({a:'resize',d:window.devicePixelRatio,w:view.manualSize[0],h:view.manualSize[1]});});
for(const event of ['pointerup','pointercancel','lostpointercapture'])ui.listen(menu,event,()=>{if(view.resizing)send({a:'resize-end'});view.resizing=null;});
ui.listen(menu,'keydown',e=>{if(!e.target.closest('.grip')||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();view.manualSize=[Math.max(240,window.innerWidth+(e.key==='ArrowRight'?10:e.key==='ArrowLeft'?-10:0)),Math.max(120,window.innerHeight+(e.key==='ArrowDown'?10:e.key==='ArrowUp'?-10:0))];send({a:'resize',d:window.devicePixelRatio,w:view.manualSize[0],h:view.manualSize[1]});send({a:'resize-end'});});
ui.listen(document,'keydown',e=>{if(e.key==='Escape'&&view.popup){e.preventDefault();dashboard.close();return;}if(e.key==='Escape'){const overflow=menu.querySelector('.shell-overflow[open]');if(overflow){overflow.open=false;overflow.querySelector('summary').focus();return;}viewState.flush();send({a:'hide'});}if(e.altKey&&e.key==='ArrowLeft'){e.preventDefault();if(view.page!=='home'||view.taskFilter||view.working)back();}});
ui.listen(menu,'toggle',()=>fit(),true);
ui.effect('ui.fonts',()=>document.fonts.ready,()=>{if(view.S)fit();});
send({a:'ready'});
}
});
const {natural,gutter,fit,actionReceipt,actionNotices,focusStart,focusReceipt,focusReadback,remember,navigate,back,routeButton,answerSlot,answerNotices,answerToast,tasks,blockProject,unique,mode,counter,known,taskRow,taskOrder,startButton,globalGroup,problem,modeText,admission,incident,incidentNotice,service,when,local,custom,history,update,ai,addProject,bulk,views,appearance,agents,about,setup,attention,projects,controls,home,routeBody,resourceGroup,resourceCapability,resourceReceipt,capacity,workEntry,workPage,mountResources,notifications,notifyGroup,openCard,project,filteredTasks,head,draw,wholeLines,settle,refresh,release,receive,measure,mount}=api;

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
const {defer,cancel,repeat=defer}=ports.timers||ui;
const TaskqProjectBlocks=require('./project-blocks.js'),TaskqTaskControls=require('./task-controls.js');
const send=ports.send||ui.send;
const performance=window.performance||globalThis.performance,crypto=window.crypto||globalThis.crypto,CSS=window.CSS||globalThis.CSS;
let batching=0,staged=null;
// M3.26: native readback and its callbacks produce one DOM commit, fit and navigation save.
const paint=(...args)=>{if(batching){staged=[!!(staged?.[0]||args[0])];return;}(ports.render||draw)(...args);};
const present=execution.action('ui.render',paint);
const reports=require('./reports.js').create({send,draw:()=>present(),defer});
const notificationPanel=require('./notification-panel.js').create({view,reports,send,draw:()=>present()});
const {tick}=execution.module('ui.render',{tick:paint});
const renderer=shell.create(menu);
const color=st=>({doing:'work',review:'review',ask:'ask',failed:'failed',ready:'ready'}[st]||'muted');
const zone=Intl.DateTimeFormat().resolvedOptions().timeZone;
// Pages are Preact vnodes; shared module HTML enters only as an island the module owns.
const f=(...children)=>h(Fragment,null,...children),raw=html=>html?h('div',{style:'display:contents',dangerouslySetInnerHTML:{__html:html}}):'',lines=items=>items.flatMap((x,i)=>i?[h('br'),x]:[x]);
// R16/R18: Claude and Codex are counted and named apart; equal values form one group
const runtimeName=rt=>({'claude':'Claude','codex':'Codex','codex-app':'Codex app','antigravity':'Antigravity'}[rt]||rt||'Unknown runtime');
const family=rt=>/^codex/.test(rt||'')?'Codex':rt==='claude'?'Claude':runtimeName(rt),grouped=list=>{const m=new Map();for(const x of list)m.set(x,(m.get(x)||0)+1);return [...m].map(([k,n])=>n>1?k+' ('+n+')':k).join(' · ');};
// AS5 states from runtime::agent_view; a stale signed-in reads as "Signed in at <time>"
const signState={'signed-in':'Signed in',expired:'Expired',rejected:'Rejected','sign-in-required':'Sign in required','not-installed':'Not installed','check-failed':'Check failed',unknown:'Unknown'},signAlert=['expired','rejected','sign-in-required','check-failed'];

// measure at the target width: after a font change the window still has the old width, so text wraps differently
// g: this renderer's scrollbar width; a menu taller than its window scrolls and the window adds it (#1073)

const actionErrors=new Map();

// Navigation is view state only: every route consumes the same pushed snapshot.
const routes={hosts:'Hosts and resources',delivery:'Delivery','agentq-shown':'Agentq shown','agentq-setting-up':'Agentq setting up','agentq-running':'Agentq running','agentq-unknown':'Agentq unknown',work:'Balancing',recovery:'Recovery',stop:'Stop and hold',home:'Home',projects:'Projects',attention:'Attention',settings:'Settings',appearance:'Appearance',cadence:'Activity history',notifications:'Notifications','notification-settings':'Notifications',agents:'Agents and access',services:'Services and support',about:'About and updates'};
const settingsPages=['settings','appearance','hosts','delivery','cadence','notification-settings','work','recovery','agents','services','about'];
if(ports.settings)view.page='settings';
const viewState=require('./tray-view-state.js').create(view,{menu,document,send,defer,cancel,trail:renderer.entries(),routes:Object.fromEntries(Object.keys(routes).map(page=>[page,{parent:settingsPages.includes(page)&&page!=='settings'?'settings':'home'}]))});

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
const workPageFor=p=>p.resources?.targets?.some(t=>t.supported?.some(s=>s.policy_schema==='taskq.host-capacity/1'))?h('div',{'data-resources':p.root}):h('p',{role:'status'},'Legacy capacity editor unavailable; refresh host status.');
const resourceCards=new Map(),resourceRequests=new Map(),resourcePayloads=new Map();

// Balancing (menu M12): the shared editor over this host's projects; only the explicitly selected ones change

// Notifications (menu.md M14): host mode per kind, the OS permission, and per project overrides
const modes=[['off','Off'],['silent','Silent'],['banner','Banner'],['sound','Banner with sound']];

// a notification click: the project page scrolled to the card (for `ask`, its answer card); a task no longer in the
// open lists (closed): its card from the notification, kept on top of the page until navigation

// pin (menu.md M13): the same menu as a movable window; the header is its drag handle

// a bounded block shows whole lines: a line cut at the top scrolls into view, or at the scroll end (the newest text)
// moves out above; the bottom edge moves up to the first line it cuts

// a reader's scroll leaves cut lines: once it settles, the block takes its CSS bound again and realigns on whole lines

const dashboard=ports.workspace?null:require('./task-dashboard.js').create(view,{menu,present,send,app:api,owned:!!ports.views,window,document});
const startOperations=require('./workspace-answers.js').create(view,{menu,document,send,draw:present,defer,cancel});
const {answerSend}=startOperations;
return {...api,resourceSend};
}
});

module.exports={create,createState};
