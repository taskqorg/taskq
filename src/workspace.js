'use strict';
const ui=require('./ui-runtime.js'),shell=require('./app-shell.js'),{h}=require('preact'),{signal}=require('@preact/signals'),{execution}=ui;
const {create}=execution.module('workspace',{
 create:function(window=globalThis){
  const document=window.document,menu=document.querySelector('.menu'),renderer=shell.create(menu),snapshot=signal(null);
  const view={page:'messages',taskFilter:null,working:null,aiRoot:null,answer:null,disclosures:{},restoreScroll:0};
  const routes={conversations:{title:'Conversations'},messages:{title:'Inbox'},message:{title:'Message',parent:'messages'},request:{title:'New AI request',parent:'messages'},answer:{title:'Task answer',parent:'messages'},sessions:{title:'Work sessions',parent:'messages'},agents:{title:'Agent settings',parent:'messages'},operations:{title:'Operation review',parent:'messages'}};
  // A2: the recorded Wouter history is the Back stack; each entry keeps its scroll, disclosures and focus.
  const state=require('./tray-view-state.js').create(view,{menu,document,routes,trail:renderer.entries(),root:'messages',defer:ui.defer,cancel:ui.cancel,send:m=>ui.send({...m,a:'workspace-set'})});
  const local=signal({text:'',caret:[0,0],op:null,receipt:null});let hydrated=false,timer=null,composing=false;
  const answers=require('./workspace-answers.js').create(view,{menu,document,send:m=>{ui.send(m);if(m.a==='answer-local')state.flush();},draw:()=>api.draw()});
  const conversations=require('./conversations.js').create({window,document,menu,draw:()=>api.draw(),navigate:(...args)=>api.navigate(...args)});
  const operationView=require('./tray-app.js').createState();
  const operations=require('./tray-app.js').create(operationView,{window,document,menu,workspace:true,render:()=>api.draw()});
  const {session,keyed}=execution.module('workspace.links',{session:function(url){return !!url&&/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(url);},
   // Disclosures, islands and cards keep their DOM node when a notice above them appears or goes; empty text holes become null.
   keyed:function(v){if(Array.isArray(v))return v.map(keyed);if(!v?.props)return v===''?null:v;const p=v.props;if(v.key==null)v.key=p['data-answer']||p['data-resources']||p['data-card']||p['data-view-key']||p.id;if(p.children!=null)p.children=keyed(p.children);return v;}});
  // A1: each route returns its Preact vnodes; called directly, since signal-aware components skip renders when only the plain view changes.
  const pages=execution.module('workspace.routes',{
   conversations:function(){return conversations.body();},
   messages:function(){
    const m=snapshot.value.messages,filter=view.messageFilter||'all',rows=(m?.entries||[]).filter(r=>view.page!=='conversations'||r.kind!=='question').filter(r=>filter==='archived'?r.archived:!r.archived&&(filter!=='pending'||r.pending));
    return [h('div',{class:'options'},h('button',{'data-route':'request'},'New AI request'),h('button',{'data-route':'sessions'},'Work sessions'),h('button',{'data-route':'agents'},'Agent settings')),
     h('label',{for:'message-filter'},'Show messages'),h('select',{id:'message-filter','data-field':'message-filter'},[['all','All messages'],['pending','Pending'],['archived','Archived']].map(([v,label])=>h('option',{value:v,selected:filter===v},label))),
     m?.complete!==true&&h('p',{role:'status'},'Messages unknown: '+(m?.reason||'Source unavailable; refresh status.')),
     filter==='archived'&&[h('p',null,'Delete permanently removes TaskQ message records and owned copies. There is no undo. Board tasks, source files and native chats stay; sessions keep running. Requests awaiting launch cannot be deleted.'),h('button',{'data-a':'message-delete-all',disabled:!rows.length||!!view.messageBusy},'Delete all archived permanently')],
     rows.length?rows.map(r=>h('section',{class:'message-row','aria-label':r.title||'Message',key:r.key},h('button',{class:'project','data-a':'message-open','data-key':r.key},r.kind==='question'?(r.project||'Board')+' · #'+r.task+' '+r.title:r.title||'Request'),h('p',{class:'meta'},(r.kind==='question'?(r.state==='ask'?'Unanswered':'Archived')+' question · revision '+r.revision:(r.host||'Unknown host')+' · '+r.id+' · '+r.state)+(r.deleting?' · Deletion incomplete; retry Delete':r.read?' · Read':' · Unread')),h('div',{class:'options'},api.messageButtons(r)))):h('p',null,'No '+(filter==='archived'?'archived ':filter==='pending'?'pending ':'')+'messages in the available snapshot.'),
     answers.answerNotices()];
   },
   message:function(){
    const r=snapshot.value.messages?.entries?.find(r=>r.key===view.message);
    if(!r)return h('p',{role:'status'},'This message is unavailable or was deleted. Back returns to the list; Refresh reads current status.');
    const request=r.request,url=request?.url;
    return [h('p',null,r.title||'Message'),h('p',{class:'meta'},(r.host||'Unknown host')+' · '+r.id+' · '+r.state),
     h('p',null,request?.reason||request?.next||'Deletion incomplete; retry Delete to remove the remaining owned files.'),
     session(url)&&h('a',{'data-url':url,href:url},'Open native session'),
     h('p',null,'Replies stay in the native runtime. Opening this record never sends a request.'),h('div',{class:'options'},api.messageButtons(r))];
   },
   request:function(){
    const s=snapshot.value,ps=view.S.projects,r=s.ai?.request,bad=api.requestProblem(),l=local.value,busy=!!l.op&&['sending','unknown'].includes(l.receipt?.state);
    return [h('label',{for:'request-project'},'Project'),h('select',{id:'request-project','data-field':'project',disabled:busy},h('option',{value:''},'Choose a project'),!ps.some(p=>p.root===view.aiRoot)&&view.aiRoot&&h('option',{value:view.aiRoot,selected:true,disabled:true},'Unavailable project'),ps.map(p=>h('option',{value:p.root,selected:p.root===view.aiRoot},p.name))),
     h('p',{class:'meta'},'Runtime: '+(s.ai?.runtime?.name||'unknown')+'. The native session owns all replies. Codex sessions are unavailable here.'),
     h('form',{'data-form':'request'},h('label',{for:'request-text'},'Request to AI'),h('textarea',{id:'request-text','data-field':'request',rows:'4',readOnly:busy,value:l.text}),h('div',{class:'options'},h('button',{type:'submit','data-a':'request-send',disabled:!!bad,'aria-describedby':'request-reason'},'Send')),h('p',{id:'request-reason'},bad)),
     l.receipt&&h('p',{role:'status','aria-live':'polite'},l.receipt.state+(l.receipt.reason?': '+l.receipt.reason:'')),
     l.receipt?.state==='unknown'&&h('button',{'data-a':'request-retry'},'Check delivery'),
     h('details',{'data-view-key':'request-history'},h('summary',null,'Recent requests'),(s.ai?.history||[]).map((t,i)=>h('button',{class:'project','data-a':'request-pick','data-index':i},t))),
     r&&h('section',{'aria-label':'Latest native request'},h('p',null,'Request · '+(r.kind||'unknown')),h('p',null,r.reason||r.next||'Request state unknown; inspect the native runtime before retrying.'),session(r.url)&&h('a',{'data-url':r.url,href:r.url},'Open session'))];
   },
   answer:function(){
    const c=view.answer,p=view.S.projects.find(p=>p.issues===c?.board&&(!c.root||p.root===c.root)),t=p?.tasks?.find(t=>t.n===c.task);
    if(!p||!t)return [h('p',{role:'status'},'This task is unavailable in the current snapshot. Refresh or open its board; no answer was sent.'),c?.board&&h('a',{href:c.board+'/'+c.task,'data-url':c.board+'/'+c.task},'Open task on board')];
    return [h('p',{class:'meta'},p.name+' · ',h('a',{href:p.issues,'data-url':p.issues},'Board'),' · attempt '+(t.attempt??'unknown')+' · revision '+(t.revision??'unknown')),
     (!p.alive||p.complete===false)&&h('p',{role:'status'},'Project status is unavailable, stale or partial. Refresh or inspect the board; answers are disabled.'),
     !!c.revision&&c.revision!==t.revision&&h('p',{role:'status'},'The question changed since this launcher was opened. Read the current question before answering.'),
     t.state==='ask'?answers.answerSlot({...t,key:p.issues+'/'+t.n}):h('p',{role:'status'},'Task state: '+t.state+'. There is no open question.'),answers.answerNotices(p.issues)];
   },
   operations:function(){return operations.operationBody(view.operation);},
   sessions:function(){return conversations.sessions();},
   agents:function(){return h('p',null,'Agent settings are not available here. Use Settings → Agents in the tray. Opening this screen changes no runtime or permissions.');}
  });
  const api=execution.module('workspace.instance',{
   messageAction:function(action,keys){
    if(view.messageBusy||!keys.length)return;
    const id=Array.from(window.crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');view.messageBusy=id;view.messageNotice='Checking '+action+' readback…';
    ui.defer(()=>{if(view.messageBusy===id){view.messageBusy=null;view.messageNotice='Message action outcome unknown. Refresh to read back; no action was replayed.';api.draw();}},30000);
    ui.send({a:'message-action',id,action,keys});api.draw();
   },
   messageButtons:function(row){return h('button',{'data-a':row.archived?'message-delete':'message-archive','data-key':row.key,disabled:!!view.messageBusy},row.archived?'Delete permanently':'Archive');},
   persist:function(){ui.send({a:'ai-draft',text:local.value.text,root:view.aiRoot,local:local.value});},
   requestProblem:function(){
    if(!snapshot.value)return 'Loading project status; use Refresh.';
    if(snapshot.value.ai?.error)return snapshot.value.ai.error;
    if(local.value.op&&['sending','unknown'].includes(local.value.receipt?.state))return local.value.receipt.state==='sending'?'Sending request; wait for its receipt.':'Delivery unknown. Check delivery with the same request ID before sending another.';
    const p=view.S.projects.find(p=>p.root===view.aiRoot);
    if(!p)return 'Choose a registered project.';
    if(!p.alive)return 'Project service is unavailable or stale. Refresh or inspect Services in the tray.';
    if(!snapshot.value.ai?.runtime?.available)return snapshot.value.ai?.runtime?.reason||'Owner-session runtime is unknown. Refresh or inspect Agents in the tray.';
    if(!local.value.text.trim())return 'Write a request before sending.';
    return '';
   },
   draw:function(){
    const s=snapshot.value,active=document.activeElement,focus=state.capture().focus,scroll=view.restoreScroll??menu.querySelector('.content')?.scrollTop??0;
    document.documentElement.style.colorScheme=s?.theme||'light dark';menu.style.setProperty('--size',(s?.font||16)+'px');
    const icons=require('./ui-icons.js');
    const actions=[icons.button('rotate-ccw','Refresh workspace status',{'data-a':'refresh'}),h('details',{class:'shell-overflow','data-view-key':'navigation-actions'},h('summary',{title:'Workspace navigation','aria-label':'Workspace navigation'},'⋯'),h('nav',{class:'overflow-actions','aria-label':'Workspace'},[['messages','Inbox'],['conversations','Conversations'],['request','New AI request'],['sessions','Work sessions'],['agents','Agent settings']].map(([route,label])=>h('button',{'data-route':route},label))))];
    renderer.render(keyed([shell.bar({title:routes[view.page]?.title||'Unavailable',home:'Workspace home',back:view.page!=='messages',actions,windows:[icons.button('always-on-top','Always on top',{'data-a':'workspace-pin','aria-pressed':String(!!view.workspacePin)}),icons.button(view.workspaceMaximized?'minimize-2':'maximize-2',view.workspaceMaximized?'Restore window':'Enlarge window',{'data-a':'workspace-enlarge','aria-pressed':String(!!view.workspaceMaximized)}),icons.button('x','Close workspace',{'data-a':'hide'})]}),
     h('main',{class:'content'+(view.page==='conversations'?' conversation-content':'')},h('div',{class:'content-body'},view.unavailable&&h('p',{role:'status'},view.unavailable+' The surviving destination is Messages.'),s?(pages[view.page]||pages.agents)():h('p',null,'Loading workspace. Refresh if status does not arrive.'),[view.navigationError,view.localError,s?.ai?.error].filter(Boolean).map(error=>h('p',{role:'alert'},error)),h('p',{class:'meta',role:'status','aria-live':'polite','aria-atomic':'true','data-status':'workspace'},view.messageNotice||view.notice||''))),answers.answerToast(),h('footer',{class:'status-footer'},h('span',null,'TaskQ '+(s?.version?'v'+s.version:'version unknown')),h('button',{class:'workspace-resize','data-a':'workspace-resize','aria-label':'Resize workspace',title:'Resize workspace; arrow keys adjust size'}))]),'/'+view.page);
    answers.mountAnswers();if(view.page==='operations')operations.mountResources();state.restore(view.disclosures);menu.querySelector('.content').scrollTop=scroll;view.restoreScroll=null;
    if(active?.isConnected)active.focus({preventScroll:true});else if(focus)state.focus(focus);
    conversations.afterDraw();state.flush();
   },
   navigate:function(page,context){
    if(Object.hasOwn(routes,page))view.unavailable=null;
    else {view.unavailable='Requested destination is not available.';page='messages';}
    renderer.push(state.capture());view.page=page;
    if(page==='answer')view.answer=context;
    if(page==='operations')view.operation={operation:context?.operation,root:context?.root||null};
    if(page==='message')view.message=context?.key;
    if(page==='request'&&context?.root){
     if(local.value.op&&['sending','unknown'].includes(local.value.receipt?.state)){view.aiRoot=local.value.op.root;view.notice='A request is awaiting readback in its original project. Check delivery before changing project.';}
     else view.aiRoot=context.root;
    }
    view.disclosures={};view.restoreScroll=0;api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');menu.querySelector('h1').focus();state.flush();
    if(page==='request'){const input=menu.querySelector('[data-field=request]');input?.setSelectionRange(...local.value.caret);}
   },
   back:function(){const prior=renderer.pop()||{page:'messages',open:{},scroll:0};state.apply(prior);api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');state.focus(prior.focus);},
   submit:function(){
    if(api.requestProblem())return;
    const op={id:Array.from(window.crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join(''),root:view.aiRoot,text:local.value.text,runtime:snapshot.value.ai.runtime.id};
    local.value={...local.value,op,receipt:{state:'sending'}};api.persist();api.dispatch(op);api.draw();
   },
   dispatch:function(op,reconcile=false){
    ui.cancel(timer);timer=ui.defer(()=>api.receive({aiReceipt:{id:op.id,state:'unknown',reason:'No native receipt received. Check delivery with the same request ID.'}}),30000);
    ui.send({a:'ai-send',...op,reconcile});
   },
   receive:function(s){
    if(s.workspacePin!==undefined||s.workspaceMaximized!==undefined){if(s.workspacePin!==undefined)view.workspacePin=s.workspacePin;if(s.workspaceMaximized!==undefined)view.workspaceMaximized=s.workspaceMaximized;api.draw();return;}
    if(conversations.receive(s)) {api.draw();return;}
    if(s.messageReceipt){
     if(s.messageReceipt.id!==view.messageBusy)return;
     const r=s.messageReceipt;view.messageBusy=null;
     view.messageNotice=r.reason||r.outcomes.map(o=>o.state==='accepted'?({read:'Message marked read',archive:'Message archived',delete:'Message permanently deleted'}[o.action]||'Message updated'):'Unknown: '+o.reason).join('; ');
     api.draw();return;
    }
    if(s.diagnose){ui.execution.diagnose(s.diagnose.selector,Math.max(0,Math.min(60000,s.diagnose.until_ms-Date.now())),s.diagnose.limit);return;}
    if(['capmsg','focus_receipt','action_error','resources_receipt','aiError'].some(k=>s[k])){operations.receive(s);return;}
    if(s.answerReceipt){answers.answerReceipt(s.answerReceipt);return;}
    if(s.aiReceipt){
     const r=s.aiReceipt,l=local.value;if(r.id!==l.op?.id)return;
     ui.cancel(timer);local.value={...l,receipt:r,...(r.state==='accepted'?{text:'',caret:[0,0]}:{})};
     if(r.state==='accepted')local.value.op=null;
     api.persist();api.draw();return;
    }
    if(s.viewStateError!==undefined||s.workspaceLocalError!==undefined){if(s.viewStateError!==undefined)view.navigationError=s.viewStateError;if(s.workspaceLocalError!==undefined)view.localError=s.workspaceLocalError;api.draw();return;}
    if(s.open){const c=s.open;api.navigate(c.page,c);return;}
    const first=snapshot.value===null;snapshot.value=s;view.S=s;operationView.S=s;operations.focusReadback();operations.lifecycleReadback();
    if(!hydrated){hydrated=true;const l=s.ai?.local;local.value=l&&typeof l.text==='string'?{...l,caret:l.caret||[0,0]}:{text:s.ai?.draft||'',caret:[0,0],op:null,receipt:null};if(local.value.op&&local.value.receipt?.state==='sending')local.value={...local.value,receipt:{state:'unknown',reason:'Workspace reopened; inspect delivery with the same request ID.'}};}
    answers.hydrate(s.workspace_local);state.hydrate({...s,navigation:s.workspace_navigation});
    if(first&&view.aiRoot===null)view.aiRoot=s.ai?.root||s.projects?.[0]?.root||null;
    if(local.value.op&&['sending','unknown'].includes(local.value.receipt?.state))view.aiRoot=local.value.op.root;
    const request=s.ai?.request;view.notice=request?'Native request: '+(request.kind||'unknown')+(request.reason?'. '+request.reason:''):'Workspace status received.';
    answers.answerReadback();api.draw();if(first){const input=menu.querySelector('[data-field=request]');input?.setSelectionRange(...local.value.caret);state.focus(view.restoreFocus);}
   },
   mount:function(){
    ui.connect();window.taskq={receive:api.receive};conversations.mount();api.draw();
    ui.listen(menu,'click',e=>{
     const link=e.target.closest('a');if(link){e.preventDefault();ui.send({a:'open',url:link.dataset.url||link.href});return;}
     const b=e.target.closest('button');if(!b||b.disabled)return;
     if(view.page==='operations'){
      if(b.dataset.route==='recovery'){api.navigate('operations',{operation:'recovery'});return;}
      if(operations.operationAction(b.dataset))return;
     }
     if(conversations.click(b))return;
     if(b.dataset.a==='home'){if(view.page!=='messages')api.navigate('messages');return;}
     if(b.dataset.a==='workspace-pin'||b.dataset.a==='workspace-enlarge'){ui.send({a:'workspace-window',action:b.dataset.a==='workspace-pin'?'pin':'enlarge'});return;}
     if(b.dataset.a==='toast-dismiss'){view.toast=null;api.draw();}
     else if(b.dataset.route)api.navigate(b.dataset.route);
     else if(b.dataset.a==='open-answer')api.navigate('answer',{root:b.dataset.root,board:b.dataset.board,task:Number(b.dataset.task),revision:Number(b.dataset.revision)});
     else if(b.dataset.a==='message-open'){
      const r=snapshot.value.messages?.entries?.find(r=>r.key===b.dataset.key);if(!r)return;
      api.messageAction('read',[r.key]);
      if(r.kind==='question')api.navigate('answer',{root:r.root,board:r.board,task:r.task,revision:r.revision});else api.navigate('message',{key:r.key});
     }
     else if(b.dataset.a==='message-archive')api.messageAction('archive',[b.dataset.key]);
     else if(b.dataset.a==='message-delete')api.messageAction('delete',[b.dataset.key]);
     else if(b.dataset.a==='message-delete-all')api.messageAction('delete',(snapshot.value.messages?.entries||[]).filter(r=>r.archived).map(r=>r.key));
     else if(b.dataset.a==='back')api.back();
     else if(b.dataset.a==='hide'){conversations.flush();api.persist();state.flush();ui.send({a:'hide'});}
     else if(b.dataset.a==='refresh'){view.notice='Refreshing workspace status…';ui.send({a:'refresh'});api.draw();}
     else if(b.dataset.a==='request-retry'){const l=local.value;if(l.op&&l.receipt?.state==='unknown'){local.value={...l,receipt:{state:'sending'}};api.persist();api.dispatch(l.op,true);api.draw();}}
     else if(b.dataset.a==='request-pick'&&!local.value.op){local.value={...local.value,text:snapshot.value.ai.history[Number(b.dataset.index)],caret:[0,0]};api.persist();api.draw();menu.querySelector('[data-field=request]')?.focus();}
     else if(b.dataset.a==='answer-retry')answers.retry(b.dataset.key);
     else if(b.dataset.a==='answer-dismiss')answers.dismiss(b.dataset.key);
    });
    ui.listen(menu,'submit',e=>{if(e.target.dataset.form==='request'){e.preventDefault();api.submit();}});
    ui.listen(menu,'input',e=>{if(view.page==='operations')operations.operationInput(e);if(e.target.dataset.field==='request'){local.value={...local.value,text:e.target.value,caret:[e.target.selectionStart,e.target.selectionEnd]};api.persist();api.draw();}});
    ui.listen(menu,'change',e=>{if(e.target.dataset.field==='project'){view.aiRoot=e.target.value;api.persist();api.draw();}else if(e.target.dataset.field==='message-filter'){view.messageFilter=e.target.value;view.messageNotice=null;api.draw();}});
    for(const event of ['select','keyup','click'])ui.listen(menu,event,e=>{if(e.target.dataset.field==='request'){local.value={...local.value,caret:[e.target.selectionStart,e.target.selectionEnd]};api.persist();}});
    ui.listen(menu,'compositionstart',()=>{composing=true;});ui.listen(menu,'compositionend',()=>{composing=false;});
    ui.listen(menu,'scroll',()=>state.schedule(),true);ui.listen(menu,'toggle',()=>state.schedule(),true);
    ui.listen(document,'keydown',e=>{
     if(e.target.dataset?.field==='request'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composing&&e.keyCode!==229){e.preventDefault();api.submit();return;}
     if(e.key==='Escape'){conversations.flush();const overflow=menu.querySelector('.shell-overflow[open]');if(overflow){overflow.open=false;overflow.querySelector('summary').focus();return;}api.persist();state.flush();ui.send({a:'hide'});}
     if(e.altKey&&e.key==='ArrowLeft'&&view.page!=='messages'){e.preventDefault();api.back();}
    });
    // P-W3: the header drags the window; its controls never start a drag
    let dragFrom=null;
    ui.listen(menu,'pointerdown',e=>{if(e.button===0&&e.target.closest('.head')&&!e.target.closest('button,a,summary,input,select,textarea'))dragFrom={x:e.screenX,y:e.screenY};});
    ui.listen(window,'pointermove',e=>{if(dragFrom&&Math.hypot(e.screenX-dragFrom.x,e.screenY-dragFrom.y)>=4){dragFrom=null;ui.send({a:'workspace-window',action:'drag'});}});
    for(const event of ['pointerup','pointercancel'])ui.listen(window,event,()=>{dragFrom=null;});
    ui.listen(window,'resize',()=>shell.fitTitle(menu));
    let resizing=null;
    ui.listen(menu,'pointerdown',e=>{if(e.target.closest('[data-a=workspace-resize]')){resizing={x:e.clientX,y:e.clientY,w:window.innerWidth,h:window.innerHeight};e.target.setPointerCapture?.(e.pointerId);e.preventDefault();}});
    ui.listen(menu,'pointermove',e=>{if(resizing)ui.send({a:'workspace-window',action:'size',width:resizing.w+e.clientX-resizing.x,height:resizing.h+e.clientY-resizing.y});});
    ui.listen(menu,'pointerup',()=>{resizing=null;});ui.listen(menu,'pointercancel',()=>{resizing=null;});
    ui.listen(menu,'keydown',e=>{if(e.target.dataset.a==='workspace-resize'&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();ui.send({a:'workspace-window',action:'size',width:window.innerWidth+(e.key==='ArrowRight'?20:e.key==='ArrowLeft'?-20:0),height:window.innerHeight+(e.key==='ArrowDown'?20:e.key==='ArrowUp'?-20:0)});}});
    ui.send({a:'ready'});
   }
  });
  return api;
 }
});
module.exports={create};
