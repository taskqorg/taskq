'use strict';
const ui=require('./ui-runtime.js'),icons=require('./ui-icons.js'),shell=require('./app-shell.js'),{h}=require('preact'),{signal}=require('@preact/signals'),{execution}=ui;
const syncView=require('./sync-view.js');
const {create}=execution.module('workspace',{
 create:function(window=globalThis){
  const document=window.document,menu=document.querySelector('.menu'),renderer=shell.create(menu),snapshot=signal(null);
  const view={page:'conversations',section:null,taskFilter:null,working:null,aiRoot:null,answer:null,disclosures:{},restoreScroll:0};
  // #1699 A1: Conversations is the one thread screen; a task thread is view.answer, an event thread view.message, else an owner conversation
  const routes={conversations:{title:'Conversations'},sessions:{title:'Work sessions',parent:'conversations'},operations:{title:'Operation review',parent:'conversations'}};
  // A2: the recorded Wouter history is the Back stack; each entry keeps its scroll, disclosures and focus.
  const state=require('./tray-view-state.js').create(view,{menu,document,routes,trail:renderer.entries(),root:'conversations',defer:ui.defer,cancel:ui.cancel,send:m=>ui.send({...m,a:'workspace-set'})});
  let composing=false,answerField=null,redraw=null;
  // #1662 A3: task answers use the workspace's one composer; a card change repaints it once per tick
  const pm=require('./pm-composer.js');
  const answers=require('./workspace-answers.js').create(view,{menu,document,send:m=>{ui.send(m);if(m.a==='answer-local')state.flush();},draw:()=>api.draw(),composer:true,steer:pm.composer,changed:()=>{if(view.page==='conversations'&&view.answer&&!redraw)redraw=ui.defer(()=>{redraw=null;api.draw();},0);}});
  const conversations=require('./conversations.js').create({window,document,menu,draw:()=>api.draw(),navigate:(...args)=>api.navigate(...args),onSelect:id=>{const row=api.threads().find(t=>t.id===id);view.answer=row?.task?{root:row.root,board:row.board,task:row.task,revision:row.revision}:null;view.message=row?.event&&!row.task?row.event.key:null;}});
  const operationView=require('./tray-app.js').createState();
  const operations=require('./tray-app.js').create(operationView,{window,document,menu,workspace:true,render:()=>api.draw()});
  const {session,keyed}=execution.module('workspace.links',{session:function(url){return !!url&&/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(url);},
   // Disclosures, islands and cards keep their DOM node when a notice above them appears or goes; empty text holes become null.
   keyed:function(v){if(Array.isArray(v))return v.map(keyed);if(!v?.props)return v===''?null:v;const p=v.props;if(v.key==null)v.key=p['data-answer']||p['data-resources']||p['data-card']||p['data-view-key']||p.id;if(p.children!=null)p.children=keyed(p.children);return v;}});
  // A1: each route returns its Preact vnodes; called directly, since signal-aware components skip renders when only the plain view changes.
  const pages=execution.module('workspace.routes',{
   conversations:function(){
    const threads=api.threads(),id=conversations.sync(threads,view.answer?'task:'+view.answer.board+'/'+view.answer.task:view.message?'event:'+view.message:null),row=threads.find(t=>t.id===id),selected=row?.id;
    const marks=Object.fromEntries((snapshot.value.messages?.entries||[]).filter(r=>r.kind==='request'&&!r.archived&&!r.read).map(r=>[r.id,'Unread']));
    const ev=!selected&&(snapshot.value.messages?.entries||[]).find(r=>r.kind==='request'&&r.id===conversations.current()?.id);
    return conversations.body({rows:threads,selected,marks,busy:!!view.messageBusy,archive:r=>{if(r.event)api.messageAction(r.archived?'restore':'archive',[r.event.key],r.id);else conversations.sidebar(r.archived?'restore':'archive',r.id,undefined,r.root||'');},notice:ev&&[!ev.read&&!ev.archived&&h('button',{'data-a':'message-read','data-key':ev.key,class:'compact-action notice-read',title:'Mark read','aria-label':'Mark read',disabled:!!view.messageBusy},icons.icon('check')),api.threadNotice('conversation')],detail:selected&&(row?.task?pages.task(row):row?pages.message(row):h('p',{role:'status'},'This thread is unavailable or was deleted. Choose another thread; nothing was sent.')),
     // #1819: the window's one stale-data line sits in the status footer, which is already a live region
     footer:[api.staleText(),api.threadNotice('list'),view.undoMessage&&h('button',{'data-a':'message-restore','data-key':view.undoMessage,disabled:!!view.messageBusy},'Undo archive')]});
   },
   // #1699 A3: the task thread: question card with source choice ids, order and recommendation, then the shared answer composer
   task:function(row){
    const p=view.S.projects.find(p=>p.issues===row.board&&(!row.root||p.root===row.root)),t=p?.tasks?.find(t=>t.n===row.task);
    const composer=p&&t?.state==='ask'?api.answerComposer(p,t):{};
    // #1781: Task info above the title; the title row holds the board, Mark read and Archive icons (input-scroll 04)
    const ev=row.event,url=row.board+'/'+row.task,busy=!!view.messageBusy;
    return {header:[h('details',{class:'ids conversation-info','data-view-key':'answer-ids:'+row.id},h('summary',null,icons.icon('info'),'Task info'),ev?.archived&&h('div',{class:'conversation-tools'},api.eventButtons(ev)),h('p',{class:'meta'},'Task #'+row.task+' · attempt '+(t?.attempt??'unknown')+' · question revision '+(t?.revision??row.revision??'unknown'))),
     h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},(p?.name||row.project||'Board')+' · '+row.title),
      h('a',{class:'compact-action',href:url,'data-url':url,title:'Open #'+row.task+' on the board','aria-label':'Open #'+row.task+' on the board'},icons.icon(p?.board_kind==='gitlab'?'gitlab':'github')),
      ev&&!ev.archived&&!ev.read&&icons.button('check-check','Mark read',{'data-a':'message-read','data-key':ev.key,class:'compact-action',disabled:busy}),
      ev&&!ev.archived&&icons.button('archive','Archive',{'data-a':'message-archive','data-key':ev.key,class:'compact-action',disabled:busy}))],reading:[
     api.threadNotice(row.id),
     // #1819: the task's pending operation (Sending, Waiting GitHub, Done, Failed with Retry), as on the menu row
     syncView.row(t?.sync),
     !p||!t?h('p',{role:'status'},'This task is unavailable in the current snapshot. Refresh or open its board; no answer was sent.'):
     [(!p.alive||p.complete===false)&&h('p',{role:'status'},'Project status is unavailable, stale or partial. Refresh or inspect the board; answers are disabled.'),
      // #1773 A3: the task shows as one card in the chat: agent state, then the question; #1781: the thread title above
      // already names number and title (input-scroll 04), so the card keeps them only as its accessible name
      h('article',{class:'journey-card','data-phase':t.state,'aria-label':'#'+t.n+' · '+t.title},h('div',{class:'journey-heading'},h('span',{class:'journey-state','data-state':t.state},icons.icon(t.state==='ask'?'circle-question-mark':'activity'),{ask:'Agent asks you',doing:'Agent is working',review:'In review',failed:'Failed',held:'Held'}[t.state]||api.plain(t.state))),t.state==='ask'?h('div',{class:'thread-card'},answers.answerSlot({...t,key:p.issues+'/'+t.n})):answers.answerWon(p.issues,t)||h('p',{role:'status'},'Task state: '+t.state+'. There is no open question.')),t.state==='ask'&&composer.body,answers.answerSteerComposer(p.issues,t),answers.answerNotices(p.issues,t.n)]],tools:composer.tools};
   },
   // an event with no owner conversation (setup, project open) keeps its record as its own thread
   message:function(row){
    const r=row.event,request=r.request,url=request?.url;
    return {header:h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},r.title||'Request')),reading:[api.ids(r,[api.eventButtons(r),h('p',{class:'meta'},api.plain(r.state)),session(url)&&h('a',{'data-url':url,href:url},'Open native session')]),api.threadNotice(row.id),
     h('p',null,request?.reason||request?.next||'Deletion incomplete; retry Delete to remove the remaining owned files.')]};
   },
   operations:function(){return operations.operationBody(view.operation);},
   sessions:function(){return conversations.sessions();}
  });
  const api=execution.module('workspace.instance',{
   // the notice stays on the thread that asked; an archived thread leaves the list, so its notice and Undo go to the list footer
   messageAction:function(action,keys,thread){
    if(view.messageBusy||!keys.length)return;
    const id=Array.from(window.crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');view.messageBusy=id;view.messageThread={thread:thread||'conversation',action,keys};view.threadNotice={thread:view.messageThread.thread,text:'Checking '+action+' readback…'};
    ui.defer(()=>{if(view.messageBusy===id){view.messageBusy=null;view.threadNotice={thread:view.messageThread.thread,text:'Message action outcome unknown. Refresh to read back; no action was replayed.'};api.draw();}},30000);
    ui.send({a:'message-action',id,action,keys});api.draw();
   },
   thread:function(key){const r=snapshot.value.messages?.entries?.find(r=>r.key===key);return !r?null:r.kind==='question'?'task:'+r.board+'/'+r.task:conversations.ids().includes(r.id)?'conversation':'event:'+r.key;},
   // #1699 A2/A7: Mark read is explicit; Archive offers Undo; an archived event restores or deletes
   eventButtons:function(r){const busy=!!view.messageBusy;return r&&h('span',{class:'options'},!r.read&&!r.archived&&h('button',{'data-a':'message-read','data-key':r.key,disabled:busy},'Mark read'),r.archived?[h('button',{'data-a':'message-restore','data-key':r.key,disabled:busy},'Restore'),h('button',{'data-a':'message-delete','data-key':r.key,disabled:busy},'Delete permanently')]:h('button',{'data-a':'message-archive','data-key':r.key,disabled:busy},'Archive'));},
   // #1699 A6: a message receipt shows in the thread it acted on, never in the shared status line
   staleText:function(){const t=syncView.stale(snapshot.value);return t?h('span',{class:'meta stale'},t):null;},
   threadNotice:function(id){return view.threadNotice?.thread===id?h('p',{class:'meta thread-notice',role:'status'},view.threadNotice.text):null;},
   // #1699 A1: one task thread per board task with a question event, plus the task opened from the menu; an owner conversation's request events stay on it
   threads:function(){
    const S=snapshot.value,rows=new Map(),owned=new Set(conversations.ids()),local=conversations.sidebarState();
    for(const r of S.messages?.entries||[]){
     if(r.kind==='question'){const id='task:'+r.board+'/'+r.task,prior=rows.get(id);if(!prior||prior.event.archived&&!r.archived)rows.set(id,{id,task:r.task,board:r.board,root:r.root,title:'#'+r.task+' '+(r.title||''),project:r.project,revision:r.revision,event:r,archived:r.archived});}
     else if(r.kind==='request'&&!owned.has(r.id))rows.set('event:'+r.key,{id:'event:'+r.key,title:r.title||'Request',root:r.root,updated:r.updated??r.created,event:r,archived:r.archived,meta:(r.host||'Host unknown')+' · '+api.plain(r.state),marker:!r.read&&!r.archived?'Unread':''});
    }
    const a=view.answer,refs=new Map(conversations.opened().map(r=>[r.id,r]));
    if(a&&!refs.has('task:'+a.board+'/'+a.task))refs.set('task:'+a.board+'/'+a.task,{id:'task:'+a.board+'/'+a.task,root:a.root});
    for(const [id,root] of Object.entries(local.archived||{}))if(!refs.has(id))refs.set(id,{id,root});
    for(const {id,root} of refs.values()){if(!rows.has(id)){const cut=id.lastIndexOf('/'),task=Number(id.slice(cut+1)),board=id.slice(5,cut);rows.set(id,{id,task,board,root,title:'#'+task});}else if(root)rows.get(id).root=root;}
    for(const row of rows.values())if(row.task){
     const matches=S.projects.filter(p=>p.issues===row.board&&(!row.root||p.root===row.root)),p=matches.length===1?matches[0]:null,t=p?.tasks?.find(t=>t.n===row.task);
     if(t){row.title='#'+t.n+' '+t.title;row.revision=t.revision;row.updated=typeof t.updated==='number'?t.updated*1000:null;}
     if(p){row.root=p.root;row.project=p.name;}
     if(!row.event)row.archived=Object.hasOwn(local.archived||{},row.id);
     // #1773 A4: a row without a known state names only its project, never a bare unknown
     // #1819: an answer's expected state shows here in the same frame, marked unconfirmed until the board shows it
     const state=t?.state||row.event?.state,unconfirmed=['sending','waiting'].includes(t?.sync?.phase)?' ⟳':'';row.status=!state||state==='ask'&&!row.archived?'':api.plain(state)+unconfirmed;row.meta=(p?.name||row.project||'Board')+(row.status?' · '+row.status:'');
     row.marker=t?.state==='ask'&&!row.archived?'Needs reply':row.event&&!row.event.read&&!row.event.archived?'Unread':'';
    }
    return [...rows.values()];
   },
   // #1699 A2: the menu badge selects the first unread thread, an open question first; nothing is acknowledged
   unread:function(){
    const e=(snapshot.value?.messages?.entries||[]).filter(r=>!r.archived&&!r.read),r=e.find(r=>r.kind==='question'&&r.state==='ask')||e.find(r=>r.kind==='question')||e.find(r=>r.kind==='request');
    if(!r)return;
    if(r.kind==='question'){conversations.show('recent');conversations.select('task:'+r.board+'/'+r.task,false);}
    else if(!conversations.reveal(r.id,false)){conversations.show('recent');conversations.select('event:'+r.key,false);}
   },
   // A1: a record's technical identity, read only on demand
   ids:function(r,controls){return h('details',{class:'ids conversation-info','data-view-key':'ids:'+r.key},h('summary',null,...(r.kind==='question'?[icons.icon('info'),'Task info']:['Details'])),controls,h('p',{class:'meta'},r.kind==='question'?'Task #'+r.task+' · question revision '+r.revision:(r.host||'Unknown host')+' · request '+r.id+' · state '+r.state));},
   plain:function(state){return {launched:'Launched in the native session',launching:'Launching',pending:'Waiting to launch',failed:'Launch failed',unknown:'Delivery unknown'}[state]||(state?String(state).replace(/[-_]/g,' '):'State unknown');},
   // A3: one composer for a task answer: context, validation and Send; the card above keeps choices, receipt and recovery
   answerComposer:function(p,t){
    const c=answers.answerCompose(p.issues,t);if(!c)return {};
    const off=!c.live,why=c.live?c.problem||'':c.problem;
    return {body:h('section',{class:'answer-composer','aria-label':'Answer #'+t.n},pm.composer({text:c.draft,disabled:off,id:'answer-draft',field:'answer',label:c.send==='Requeue'?'Requeue reason':'Your answer',placeholder:c.send==='Requeue'?'Reason to requeue':c.choices?'Or type an answer':'What you did',
     // #1781: an empty draft shows no validation line, and Send is the arrow icon (input-scroll 04)
     reason:off||c.draft?why:'',
     // #1781: the bottom row matches the chat composer: command history left; Send, then the answer help icon, right (input-scroll 04)
     tools:h('div',{class:'conversation-tools composer-bottom'},
      h('details',{'data-view-key':'answer-history',class:'command-history-menu'},h('summary',{class:'compact-action',title:'History','aria-label':'History'},icons.icon('history')),h('div',{class:'conversation-history'},(view.S.ai?.history||[]).map((text,i)=>h('button',{'data-a':'answer-history','data-index':i,disabled:off},text)),h('p',{class:'meta'},'History fills the draft only; nothing is sent.'))),
      c.requeue!=null&&h('button',{'data-a':'answer-requeue',disabled:off||!!c.requeue},'Requeue'),h('button',{'data-a':'answer-send',class:'primary-action compact-action',title:c.send,'aria-label':c.send,disabled:off||!!c.problem},icons.icon('arrow-up')),
      h('details',{class:'composer-help','data-view-key':'answer-help'},h('summary',{class:'compact-action',title:'Answer help','aria-label':'Answer help'},icons.icon('info')),h('p',{class:'meta'},'Task answers take text only; add files or PDFs on the board. Numbers typed here are sent as text; use the option buttons to choose.'))),
     onInput:e=>{answers.answerInput(p.issues,t.n,e.currentTarget.value,[e.currentTarget.selectionStart,e.currentTarget.selectionEnd]);api.draw();}}))};
   },
   answerAct:function(type){const c=view.answer;if(c)answers.answerAct(c.board,c.task,type);},
   draw:function(){
    conversations.capture();
    const s=snapshot.value,active=document.activeElement,focus=state.capture().focus,scroll=view.restoreScroll??menu.querySelector('.content')?.scrollTop??0;
    document.documentElement.style.colorScheme=s?.theme||'light dark';menu.style.setProperty('--size',(s?.font||16)+'px');for(const entry of (s?.palette||'').split(';')){const [key,value]=entry.split(':');if(key&&value)menu.style.setProperty(key,value);}
    const actions=[icons.button('rotate-ccw','Refresh workspace status',{'data-a':'refresh'}),h('details',{class:'shell-overflow','data-view-key':'navigation-actions'},h('summary',{title:'Workspace navigation','aria-label':'Workspace navigation'},icons.icon('ellipsis')),h('nav',{class:'overflow-actions','aria-label':'Workspace'},[['conversations','Conversations'],['sessions','Work sessions']].map(([route,label])=>h('button',{'data-route':route},label))))];
    renderer.render(keyed([shell.bar({title:routes[view.page]?.title||'Unavailable',home:'Workspace home',back:view.page!=='conversations',actions,windows:[icons.button('always-on-top','Always on top',{'data-a':'workspace-pin','aria-pressed':String(!!view.workspacePin)}),icons.button(view.workspaceMaximized?'minimize-2':'maximize-2',view.workspaceMaximized?'Restore window':'Enlarge window',{'data-a':'workspace-enlarge','aria-pressed':String(!!view.workspaceMaximized)}),icons.button('x','Close workspace',{'data-a':'hide'})]}),
     h('main',{class:'content'+(view.page==='conversations'?' conversation-content':'')},h('div',{class:'content-body'},view.unavailable&&h('p',{role:'status'},view.unavailable+' The surviving destination is Conversations.'),s?api.route():h('p',null,'Loading workspace. Refresh if status does not arrive.'),[view.navigationError,view.localError,s?.ai?.error].filter(Boolean).map(error=>h('p',{role:'alert'},error)),h('p',{class:'meta',role:'status','aria-live':'polite','aria-atomic':'true','data-status':'workspace'},view.notice||''))),h('footer',{class:'status-footer'},h('span',null,'TaskQ '+(s?.version?'v'+s.version:'version unknown')),h('button',{class:'workspace-resize','data-a':'workspace-resize','aria-label':'Resize workspace',title:'Resize workspace; arrow keys adjust size'}))]),'/'+view.page);
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
    // #1699 A2: the removed Inbox, message and answer routes open their thread in Conversations
    if(page==='messages'){page='conversations';context={...context,thread:'unread'};}
    if(page==='answer'||page==='message')page='conversations';
    if(Object.hasOwn(routes,page))view.unavailable=null;
    else {view.unavailable='Requested destination is not available.';page='conversations';}
    renderer.push(state.capture());view.page=page;
    if(page==='operations')view.operation={operation:context?.operation,root:context?.root||null};
    // the menu entry names 'unread' when it counts events; otherwise the last thread stays
    if(page==='conversations'){
     const r=context?.key&&snapshot.value?.messages?.entries?.find(r=>r.key===context.key);
     if(context?.thread==='unread')api.unread();
     else if(context?.board&&context?.task){conversations.flush();conversations.remember('task:'+context.board+'/'+Number(context.task),context.root);conversations.show('recent');conversations.select('task:'+context.board+'/'+Number(context.task),false);}
     else if(r?.kind==='question'){conversations.show('recent');conversations.select('task:'+r.board+'/'+r.task,false);}
     else if(r&&!conversations.reveal(r.id,false)){conversations.show(r.archived?'archive':'recent');conversations.select('event:'+r.key,false);}
    }
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
     const text=r.reason||r.outcomes.map(o=>o.state==='accepted'?({read:'Message marked read',archive:'Archived',restore:'Restored',delete:'Message permanently deleted'}[o.action]||'Message updated'):'Unknown: '+o.reason).join('; '),done=!r.reason&&r.outcomes.every(o=>o.state==='accepted'),{thread,action,keys}=view.messageThread||{};
     view.undoMessage=done&&action==='archive'&&keys.length===1?keys[0]:null;
     if(done)for(const e of snapshot.value.messages?.entries||[])if(keys.includes(e.key)){if(action==='archive'||action==='restore')e.archived=action==='archive';if(action==='read')e.read=true;}
     if(done&&(action==='archive'||action==='delete')&&thread!=='conversation'&&(view.answer||view.message)&&thread===(view.answer?'task:'+view.answer.board+'/'+view.answer.task:'event:'+view.message)){view.answer=null;view.message=null;}
     // A7: Restore and Undo open the restored thread in Recent
     if(done&&action==='restore'&&keys.length===1){const e=snapshot.value.messages?.entries?.find(e=>e.key===keys[0]);conversations.show('recent');if(e?.kind==='question')conversations.select('task:'+e.board+'/'+e.task,false);else if(e&&!conversations.reveal(e.id,false))conversations.select('event:'+e.key,false);}
     view.threadNotice={thread:done&&['archive','delete'].includes(action)?'list':thread,text};
     api.draw();return;
    }
    if(s.diagnose){ui.execution.diagnose(s.diagnose.selector,Math.max(0,Math.min(60000,s.diagnose.until_ms-Date.now())),s.diagnose.limit);return;}
    if(['capmsg','focus_receipt','action_error','resources_receipt','aiError'].some(k=>s[k])){operations.receive(s);return;}
    if(s.answerReceipt){answers.answerReceipt(s.answerReceipt);return;}
    if(s.viewStateError!==undefined||s.workspaceLocalError!==undefined){if(s.viewStateError!==undefined)view.navigationError=s.viewStateError;if(s.workspaceLocalError!==undefined)view.localError=s.workspaceLocalError;api.draw();return;}
    if(s.open){const c=s.open;api.navigate(c.page,c);return;}
    // #1698 A1: only a status snapshot replaces S; the tray also forwards agentq replies and Completed history pages here
    if(Object.hasOwn(s,'agentqError')){operations.receive(s);return;}
    // #1819: the shared pending store changed; Conversations and the list show it in the same frame
    if(Array.isArray(s.sync)&&!s.projects){view.sync=s.sync;if(view.raw){snapshot.value=view.S=operationView.S=syncView.overlay(view.raw,view.sync);api.draw();}return;}
    if(!Array.isArray(s.projects))return;
    if(Array.isArray(s.sync))view.sync=s.sync;view.raw=s;s=syncView.overlay(s,view.sync);
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
     if(b.dataset.a==='workspace-pin'||b.dataset.a==='workspace-enlarge'){ui.send({a:'workspace-window',action:b.dataset.a==='workspace-pin'?'pin':'enlarge'});return;}
     if(b.dataset.a==='toast-dismiss'){view.toast=null;api.draw();}
     else if(b.dataset.route)api.navigate(b.dataset.route,{section:'conversations'});
     // #1699 A2/A7: selecting a thread acknowledges nothing; its draft, caret and scroll stay with it
     else if(b.dataset.a==='thread'){
      const row=api.threads().find(t=>t.id===b.dataset.thread);if(!row)return;conversations.flush();
      conversations.select(row.id,false);
      view.restoreScroll=0;api.draw();state.flush();
     }
     else if(['message-read','message-archive','message-restore','message-delete'].includes(b.dataset.a))api.messageAction(b.dataset.a.slice(8),[b.dataset.key],api.thread(b.dataset.key));
     else if(b.dataset.a==='new-request')api.navigate('conversations',{fresh:true,root:view.aiRoot});
     else if(b.dataset.a==='answer-send')api.answerAct('submit');
     else if(b.dataset.a==='answer-requeue')api.answerAct('requeue');
     else if(b.dataset.a==='answer-history'){const c=view.answer,text=view.S.ai?.history?.[Number(b.dataset.index)];if(c&&typeof text==='string'){b.closest('details')?.removeAttribute('open');answers.answerInput(c.board,c.task,text,[text.length,text.length]);api.draw();}}
     else if(b.dataset.a==='answer-steer'){const t=answers.project(b.dataset.board)?.tasks?.find(t=>t.n===+b.dataset.task);if(t)answers.answerSteer(b.dataset.board,t);}
     else if(b.dataset.a==='back')api.back();
     else if(b.dataset.a==='hide'){conversations.flush();state.flush();ui.send({a:'hide'});}
     else if(b.dataset.a==='refresh'){view.notice='Refreshing workspace status…';ui.send({a:'refresh'});api.draw();}
     else if(b.dataset.a==='answer-retry')answers.retry(b.dataset.key);
     else if(b.dataset.a==='sync-retry')ui.send({a:'sync-retry',root:b.dataset.root,id:b.dataset.id});
     else if(b.dataset.a==='answer-dismiss')answers.dismiss(b.dataset.key);
    });
    ui.listen(menu,'input',e=>{if(view.page==='operations')operations.operationInput(e);});
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
