'use strict';
const ui=require('./ui-runtime.js'),shell=require('./app-shell.js'),{h}=require('preact'),{signal}=require('@preact/signals'),{execution}=ui;
const {create}=execution.module('workspace',{
 create:function(window=globalThis){
  const document=window.document,menu=document.querySelector('.menu'),renderer=shell.create(menu),snapshot=signal(null);
  const view={page:'conversations',section:null,taskFilter:null,working:null,aiRoot:null,answer:null,disclosures:{},restoreScroll:0};
  // #1691 A3: Conversations is the one root; Inbox is its section, not a route
  const routes={conversations:{title:'Conversations'},message:{title:'Message',parent:'conversations'},answer:{title:'Task answer',parent:'conversations'},sessions:{title:'Work sessions',parent:'conversations'},operations:{title:'Operation review',parent:'conversations'}};
  // A2: the recorded Wouter history is the Back stack; each entry keeps its scroll, disclosures and focus.
  const state=require('./tray-view-state.js').create(view,{menu,document,routes,trail:renderer.entries(),root:'conversations',defer:ui.defer,cancel:ui.cancel,send:m=>ui.send({...m,a:'workspace-set'})});
  let composing=false,answerField=null,redraw=null;
  // #1662 A3: task answers use the workspace's one composer; a card change repaints it once per tick
  const answers=require('./workspace-answers.js').create(view,{menu,document,send:m=>{ui.send(m);if(m.a==='answer-local')state.flush();},draw:()=>api.draw(),composer:true,changed:()=>{if(view.page==='answer'&&!redraw)redraw=ui.defer(()=>{redraw=null;api.draw();},0);}});
  const pm=require('./pm-composer.js');
  const conversations=require('./conversations.js').create({window,document,menu,draw:()=>api.draw(),navigate:(...args)=>api.navigate(...args)});
  const operationView=require('./tray-app.js').createState();
  const operations=require('./tray-app.js').create(operationView,{window,document,menu,workspace:true,render:()=>api.draw()});
  const {session,keyed}=execution.module('workspace.links',{session:function(url){return !!url&&/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(url);},
   // Disclosures, islands and cards keep their DOM node when a notice above them appears or goes; empty text holes become null.
   keyed:function(v){if(Array.isArray(v))return v.map(keyed);if(!v?.props)return v===''?null:v;const p=v.props;if(v.key==null)v.key=p['data-answer']||p['data-resources']||p['data-card']||p['data-view-key']||p.id;if(p.children!=null)p.children=keyed(p.children);return v;}});
  // A1: each route returns its Preact vnodes; called directly, since signal-aware components skip renders when only the plain view changes.
  const pages=execution.module('workspace.routes',{
   // #1691 A4: the Inbox section keeps requests, task answers and outcomes with their actions
   conversations:function(){
    const m=snapshot.value.messages,count=m?.complete===true&&Number.isSafeInteger(m.count)?m.count:null,inbox=view.section==='inbox';
    // the Inbox entry sits in the conversation toolbar, so the composer keeps its height in narrow windows
    const label='Inbox · '+(count===null?'unknown':count+' new'),entry=h('button',{type:'button',title:label,'aria-label':label,'data-section':'inbox','aria-pressed':String(inbox)},require('./ui-icons.js').icon('inbox'),count?' '+count:'');
    return inbox?[h('div',{class:'conversation-tools',role:'group','aria-label':'Sections'},h('button',{'data-section':'','aria-pressed':'false'},'Conversations'),entry),h('section',{class:'conversation-inbox','aria-label':'Inbox'},pages.inbox())]:conversations.body(entry);
   },
   inbox:function(){
    const m=snapshot.value.messages,filter=view.messageFilter||'all',rows=(m?.entries||[]).filter(r=>filter==='archived'?r.archived:!r.archived&&(filter!=='pending'||r.pending));
    return [h('div',{class:'options'},h('button',{'data-a':'new-request'},'New request'),h('button',{'data-route':'sessions'},'Work sessions')),
     h('label',{for:'message-filter'},'Show messages'),h('select',{id:'message-filter','data-field':'message-filter'},[['all','All messages'],['pending','Pending'],['archived','Archived']].map(([v,label])=>h('option',{value:v,selected:filter===v},label))),
     m?.complete!==true&&h('p',{role:'status'},'Messages unknown: '+(m?.reason||'Source unavailable; refresh status.')),
     filter==='archived'&&[h('p',null,'Delete permanently removes TaskQ message records and owned copies. There is no undo. Board tasks, source files and native chats stay; sessions keep running. Requests awaiting launch cannot be deleted.'),h('button',{'data-a':'message-delete-all',disabled:!rows.length||!!view.messageBusy},'Delete all archived permanently')],
     // #1662 A1: a row reads as title and plain state; host, request id and revision stay behind its disclosure
     rows.length?rows.map(r=>h('section',{class:'message-row','aria-label':r.title||'Message',key:r.key},h('button',{class:'project','data-a':'message-open','data-key':r.key},r.kind==='question'?(r.project||'Board')+' · #'+r.task+' '+r.title:r.title||'Request'),h('p',{class:'meta'},(r.kind==='question'?(r.state==='ask'?'Unanswered':'Archived')+' question':api.plain(r.state))+(r.deleting?' · Deletion incomplete; retry Delete':r.read?' · Read':' · Unread')),api.ids(r),h('div',{class:'options'},api.messageButtons(r)))):h('p',null,'No '+(filter==='archived'?'archived ':filter==='pending'?'pending ':'')+'messages in the available snapshot.'),
     h('p',{class:'meta',role:'status'},view.messageNotice||''),answers.answerNotices()];
   },
   message:function(){
    const r=snapshot.value.messages?.entries?.find(r=>r.key===view.message);
    if(!r)return h('p',{role:'status'},'This message is unavailable or was deleted. Back returns to the list; Refresh reads current status.');
    const request=r.request,url=request?.url;
    return [h('p',null,r.title||'Message'),h('p',{class:'meta'},api.plain(r.state)),api.ids(r),
     h('p',null,request?.reason||request?.next||'Deletion incomplete; retry Delete to remove the remaining owned files.'),
     session(url)&&h('a',{'data-url':url,href:url},'Open native session'),
     request?.state==='launched'&&h('button',{'data-c':'reply-open','data-id':r.id},'Reply in Conversations'),
     h('p',null,request?.state==='launched'?'Replies go to this native session from its conversation. Opening this record never sends a request.':'Replies open in Conversations after the native launch. Opening this record never sends a request.'),h('div',{class:'options'},api.messageButtons(r))];
   },
   answer:function(){
    const c=view.answer,p=view.S.projects.find(p=>p.issues===c?.board&&(!c.root||p.root===c.root)),t=p?.tasks?.find(t=>t.n===c.task);
    if(!p||!t)return [h('p',{role:'status'},'This task is unavailable in the current snapshot. Refresh or open its board; no answer was sent.'),c?.board&&h('a',{href:c.board+'/'+c.task,'data-url':c.board+'/'+c.task},'Open task on board')];
    return [h('p',{class:'meta'},p.name+' · ',h('a',{href:p.issues,'data-url':p.issues},'Board')),h('details',{class:'ids','data-view-key':'answer-ids'},h('summary',null,'Details'),h('p',{class:'meta'},'Attempt '+(t.attempt??'unknown')+' · question revision '+(t.revision??'unknown'))),
     (!p.alive||p.complete===false)&&h('p',{role:'status'},'Project status is unavailable, stale or partial. Refresh or inspect the board; answers are disabled.'),
     !!c.revision&&c.revision!==t.revision&&h('p',{role:'status'},'The question changed since this launcher was opened. Read the current question before answering.'),
     t.state==='ask'?[answers.answerSlot({...t,key:p.issues+'/'+t.n}),api.answerComposer(p,t)]:h('p',{role:'status'},'Task state: '+t.state+'. There is no open question.'),answers.answerNotices(p.issues)];
   },
   operations:function(){return operations.operationBody(view.operation);},
   sessions:function(){return conversations.sessions();}
  });
  const api=execution.module('workspace.instance',{
   messageAction:function(action,keys){
    if(view.messageBusy||!keys.length)return;
    const id=Array.from(window.crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');view.messageBusy=id;view.messageNotice='Checking '+action+' readback…';
    ui.defer(()=>{if(view.messageBusy===id){view.messageBusy=null;view.messageNotice='Message action outcome unknown. Refresh to read back; no action was replayed.';api.draw();}},30000);
    ui.send({a:'message-action',id,action,keys});api.draw();
   },
   messageButtons:function(row){return h('button',{'data-a':row.archived?'message-delete':'message-archive','data-key':row.key,disabled:!!view.messageBusy},row.archived?'Delete permanently':'Archive');},
   // A1: a record's technical identity, read only on demand
   ids:function(r){return h('details',{class:'ids','data-view-key':'ids:'+r.key},h('summary',null,'Details'),h('p',{class:'meta'},r.kind==='question'?'Task #'+r.task+' · question revision '+r.revision:(r.host||'Unknown host')+' · request '+r.id+' · state '+r.state));},
   plain:function(state){return {launched:'Launched in the native session',launching:'Launching',pending:'Waiting to launch',failed:'Launch failed',unknown:'Delivery unknown'}[state]||(state?String(state).replace(/[-_]/g,' '):'State unknown');},
   // A3: one composer for a task answer: context, validation and Send; the card above keeps choices, receipt and recovery
   answerComposer:function(p,t){
    const c=answers.answerCompose(p.issues,t);if(!c)return null;
    const off=!c.live,why=c.live?c.problem||'':c.problem;
    return h('section',{class:'answer-composer','aria-label':'Answer #'+t.n},pm.composer({text:c.draft,disabled:off,id:'answer-draft',field:'answer',label:c.send==='Requeue'?'Requeue reason':'Your answer',placeholder:c.send==='Requeue'?'Reason to requeue':c.choices?'Or type an answer':'What you did',
     before:h('p',{class:'meta'},'Answering #'+t.n+' '+(t.title||'')+' · '+p.name),
     files:h('p',{class:'meta conversation-files'},'Attachments: none. Task answers take text only; add files or PDFs on the board.'),
     tools:h('div',{class:'conversation-tools'},h('label',{class:'attach-label','aria-disabled':'true'},'Attach',h('input',{type:'file',disabled:true,'aria-label':'Attach (not available for task answers)'})),c.requeue!=null&&h('button',{'data-a':'answer-requeue',disabled:off||!!c.requeue},'Requeue'),h('button',{'data-a':'answer-send',disabled:off||!!c.problem},c.send)),
     reason:why,after:c.choices&&h('p',{class:'meta'},'A number typed here is sent as text. Pick a numbered option with its button above.'),
     onInput:e=>{answers.answerInput(p.issues,t.n,e.currentTarget.value,[e.currentTarget.selectionStart,e.currentTarget.selectionEnd]);api.draw();}}));
   },
   answerAct:function(type){const c=view.answer;if(c)answers.answerAct(c.board,c.task,type);},
   draw:function(){
    const s=snapshot.value,active=document.activeElement,focus=state.capture().focus,scroll=view.restoreScroll??menu.querySelector('.content')?.scrollTop??0;
    document.documentElement.style.colorScheme=s?.theme||'light dark';menu.style.setProperty('--size',(s?.font||16)+'px');
    const icons=require('./ui-icons.js');
    const actions=[icons.button('rotate-ccw','Refresh workspace status',{'data-a':'refresh'}),h('details',{class:'shell-overflow','data-view-key':'navigation-actions'},h('summary',{title:'Workspace navigation','aria-label':'Workspace navigation'},icons.icon('ellipsis')),h('nav',{class:'overflow-actions','aria-label':'Workspace'},[['conversations','Conversations'],['sessions','Work sessions']].map(([route,label])=>h('button',{'data-route':route},label))))];
    renderer.render(keyed([shell.bar({title:routes[view.page]?.title||'Unavailable',home:'Workspace home',back:view.page!=='conversations',actions,windows:[icons.button('always-on-top','Always on top',{'data-a':'workspace-pin','aria-pressed':String(!!view.workspacePin)}),icons.button(view.workspaceMaximized?'minimize-2':'maximize-2',view.workspaceMaximized?'Restore window':'Enlarge window',{'data-a':'workspace-enlarge','aria-pressed':String(!!view.workspaceMaximized)}),icons.button('x','Close workspace',{'data-a':'hide'})]}),
     h('main',{class:'content'+(view.page==='conversations'?' conversation-content':'')},h('div',{class:'content-body'},view.unavailable&&h('p',{role:'status'},view.unavailable+' The surviving destination is Conversations.'),s?api.route():h('p',null,'Loading workspace. Refresh if status does not arrive.'),[view.navigationError,view.localError,s?.ai?.error].filter(Boolean).map(error=>h('p',{role:'alert'},error)),h('p',{class:'meta',role:'status','aria-live':'polite','aria-atomic':'true','data-status':'workspace'},view.messageNotice||view.notice||''))),answers.answerToast(),h('footer',{class:'status-footer'},h('span',null,'TaskQ '+(s?.version?'v'+s.version:'version unknown')),h('button',{class:'workspace-resize','data-a':'workspace-resize','aria-label':'Resize workspace',title:'Resize workspace; arrow keys adjust size'}))]),'/'+view.page);
    answers.mountAnswers();if(view.page==='operations')operations.mountResources();state.restore(view.disclosures);menu.querySelector('.content').scrollTop=scroll;view.restoreScroll=null;
    if(active?.isConnected)active.focus({preventScroll:true});else if(focus)state.focus(focus);
    // a new answer field takes its saved caret once
    const field=menu.querySelector('[data-c-field=answer]');if(field&&field!==answerField){answerField=field;const t=view.answer&&view.S?.projects.find(p=>p.issues===view.answer.board)?.tasks?.find(t=>t.n===view.answer.task),c=t&&answers.answerCompose(view.answer.board,t);if(Array.isArray(c?.caret))field.setSelectionRange(...c.caret);}
    conversations.afterDraw();state.flush();
   },
   // #1680 A3: a failing route keeps the shell (Home, Back, Close); its class and site reach tray stderr, not the page-wide fence
   route:function(){
    try{return (Object.hasOwn(routes,view.page)?pages[view.page]:pages.conversations)();}
    catch(error){
     const d=require('./tray-web.js').site(error),key=view.page+' '+d.site;
     if(view.routeFault!==key){view.routeFault=key;ui.send({a:'route-error',route:view.page,...d});}
     return h('p',{role:'alert'},'This page could not be shown. Home, Back and Close still work; refresh status before retrying a command.');
    }
   },
   navigate:function(page,context){
    // #1662 A3: the removed New AI request route opens a new Conversations draft for its project
    if(page==='request'){page='conversations';context={...context,fresh:true};}
    if(page==='messages'){page='conversations';context={...context,section:'inbox'};}
    if(Object.hasOwn(routes,page))view.unavailable=null;
    else {view.unavailable='Requested destination is not available.';page='conversations';}
    renderer.push(state.capture());view.page=page;
    if(page==='answer')view.answer=context;
    if(page==='operations')view.operation={operation:context?.operation,root:context?.root||null};
    if(page==='message')view.message=context?.key;
    // the header icon names a section ('' keeps the last one); new events open Inbox
    if(page==='conversations'&&context?.section==='inbox')view.section='inbox';
    if(page==='conversations'&&(context?.section==='conversations'||context?.fresh))view.section=null;
    view.disclosures={};view.restoreScroll=0;
    if(page==='conversations'&&context?.fresh)conversations.fresh({root:context.root||undefined});
    api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');if(!(page==='conversations'&&context?.fresh))menu.querySelector('h1').focus();state.flush();
   },
   back:function(){const prior=renderer.pop()||{page:'conversations',open:{},scroll:0};state.apply(prior);api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');state.focus(prior.focus);},
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
    if(s.viewStateError!==undefined||s.workspaceLocalError!==undefined){if(s.viewStateError!==undefined)view.navigationError=s.viewStateError;if(s.workspaceLocalError!==undefined)view.localError=s.workspaceLocalError;api.draw();return;}
    if(s.open){const c=s.open;api.navigate(c.page,c);return;}
    // #1698 A1: only a status snapshot replaces S; the tray also forwards agentq replies and Completed history pages here
    if(Object.hasOwn(s,'agentqError')){operations.receive(s);return;}
    if(!Array.isArray(s.projects))return;
    const first=snapshot.value===null;snapshot.value=s;view.S=s;operationView.S=s;operations.focusReadback();operations.lifecycleReadback();
    answers.hydrate(s.workspace_local);state.hydrate({...s,navigation:s.workspace_navigation});
    if(first&&view.aiRoot===null)view.aiRoot=s.ai?.root||s.projects?.[0]?.root||null;
    // #1680 A4: request status lives on its conversation (handoff), never on other pages
    view.notice='Workspace status received.';
    answers.answerReadback();api.draw();if(first)state.focus(view.restoreFocus);
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
     if(b.dataset.a==='home'){if(view.page!=='conversations')api.navigate('conversations');return;}
     if(b.dataset.section!==undefined){view.section=b.dataset.section||null;view.restoreScroll=0;api.draw();state.flush();return;}
     if(b.dataset.a==='workspace-pin'||b.dataset.a==='workspace-enlarge'){ui.send({a:'workspace-window',action:b.dataset.a==='workspace-pin'?'pin':'enlarge'});return;}
     if(b.dataset.a==='toast-dismiss'){view.toast=null;api.draw();}
     else if(b.dataset.route)api.navigate(b.dataset.route,{section:'conversations'});
     else if(b.dataset.a==='open-answer')api.navigate('answer',{root:b.dataset.root,board:b.dataset.board,task:Number(b.dataset.task),revision:Number(b.dataset.revision)});
     else if(b.dataset.a==='message-open'){
      const r=snapshot.value.messages?.entries?.find(r=>r.key===b.dataset.key);if(!r)return;
      api.messageAction('read',[r.key]);
      if(r.kind==='question')api.navigate('answer',{root:r.root,board:r.board,task:r.task,revision:r.revision});else api.navigate('message',{key:r.key});
     }
     else if(b.dataset.a==='message-archive')api.messageAction('archive',[b.dataset.key]);
     else if(b.dataset.a==='message-delete')api.messageAction('delete',[b.dataset.key]);
     else if(b.dataset.a==='message-delete-all')api.messageAction('delete',(snapshot.value.messages?.entries||[]).filter(r=>r.archived).map(r=>r.key));
     else if(b.dataset.a==='new-request')api.navigate('conversations',{fresh:true,root:view.aiRoot});
     else if(b.dataset.a==='answer-send')api.answerAct('submit');
     else if(b.dataset.a==='answer-requeue')api.answerAct('requeue');
     else if(b.dataset.a==='back')api.back();
     else if(b.dataset.a==='hide'){conversations.flush();state.flush();ui.send({a:'hide'});}
     else if(b.dataset.a==='refresh'){view.notice='Refreshing workspace status…';ui.send({a:'refresh'});api.draw();}
     else if(b.dataset.a==='answer-retry')answers.retry(b.dataset.key);
     else if(b.dataset.a==='answer-dismiss')answers.dismiss(b.dataset.key);
    });
    ui.listen(menu,'input',e=>{if(view.page==='operations')operations.operationInput(e);});
    ui.listen(menu,'change',e=>{if(e.target.dataset.field==='message-filter'){view.messageFilter=e.target.value;view.messageNotice=null;api.draw();}});
    for(const event of ['select','keyup','click'])ui.listen(menu,event,e=>{const c=view.answer;if(e.target.dataset.cField==='answer'&&c)answers.answerInput(c.board,c.task,null,[e.target.selectionStart,e.target.selectionEnd]);});
    ui.listen(menu,'compositionstart',()=>{composing=true;});ui.listen(menu,'compositionend',()=>{composing=false;});
    ui.listen(menu,'scroll',()=>state.schedule(),true);ui.listen(menu,'toggle',()=>state.schedule(),true);
    ui.listen(document,'keydown',e=>{
     if(e.target.dataset?.cField==='answer'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composing&&e.keyCode!==229){e.preventDefault();api.answerAct('submit');return;}
     if(e.key==='Escape'){conversations.flush();const overflow=menu.querySelector('.shell-overflow[open]');if(overflow){overflow.open=false;overflow.querySelector('summary').focus();return;}state.flush();ui.send({a:'hide'});}
     if(e.altKey&&e.key==='ArrowLeft'&&view.page!=='conversations'){e.preventDefault();api.back();}
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
