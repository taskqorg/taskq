'use strict';
const ui=require('./ui-runtime.js'),icons=require('./ui-icons.js'),shell=require('./app-shell.js'),{h}=require('preact'),{signal}=require('@preact/signals'),{execution}=ui;
const syncView=require('./sync-view.js'),time=require('./relative-time.js'),notification=require('./notification-panel.js');
const {create}=execution.module('workspace',{
 create:function(window=globalThis){
  const document=window.document,menu=document.querySelector('.menu'),renderer=shell.create(menu),snapshot=signal(null);
  const view={safe:!!window.taskqSafeRecovery,page:'conversations',section:null,taskFilter:null,working:null,aiRoot:null,answer:null,disclosures:{},restoreScroll:0};
  // #1699 A1: Conversations is the one thread screen; a task thread is view.answer, an event thread view.message, else an owner conversation
  const routes={conversations:{title:'Conversations'},sessions:{title:'Work sessions',parent:'conversations'},operations:{title:'Operation review',parent:'conversations'}};
  // A2: the recorded Wouter history is the Back stack; each entry keeps its scroll, disclosures and focus.
  const state=require('./tray-view-state.js').create(view,{menu,document,routes,trail:renderer.entries(),root:'conversations',defer:ui.defer,cancel:ui.cancel,send:m=>ui.send({...m,a:'workspace-set'})});
  if(window.taskqNativeFrame)menu.classList.add('native-frame');
  let recoveryText='',composing=false,answerField=null,redraw=null,threadProjection=null,projectionSidebar=null;
  const notifications=notification.create({view,send:ui.send,draw:()=>api.draw(),messages:(action,rows)=>api.messageAction(action,rows.map(r=>r.key),'list'),reports:{rows:()=>reports.rows(),count:()=>reports.count(),markRead:rows=>reports.markRead(rows),feedback:()=>reports.feedback(),readFailure:()=>reports.readFailure()}});
  // #1662 A3: task answers use the workspace's one composer; a card change repaints it once per tick
  const pm=require('./pm-composer.js');
  const answers=require('./workspace-answers.js').create(view,{menu,document,send:m=>{ui.send(m);if(m.a==='answer-local')state.flush();},draw:()=>api.draw(),composer:true,steer:pm.composer,read:(p,t)=>conversations.task(p,t),historyText:(t,text,source,root)=>conversations.taskText(t,text,source,root),changed:()=>{if(view.page==='conversations'&&view.answer&&!redraw)redraw=ui.defer(()=>{redraw=null;api.draw();},0);}});
  const conversations=require('./conversations.js').create({window,document,menu,draw:()=>api.draw(),checkpoint:()=>api.checkpoint(),navigate:(...args)=>api.navigate(...args),onSelect:id=>{if(view.safe)return;if(view.result?.conversation!==id)view.result=null;const row=api.threads().find(t=>t.id===id);view.answer=row?.task?{root:row.root,board:row.board,task:row.task,revision:row.revision}:null;view.message=row?.event&&!row.task?row.event.key:null;}});
  const reports=require('./reports.js').create({draw:()=>api.draw(),onMove:step=>conversations.move(step),navigation:r=>conversations.navigation(r),reading:(key,r)=>conversations.reading(key,r),transcript:r=>conversations.closed(r),onOpen:r=>{if(r.next){const next=conversations.nextUnresolved(api.threads());if(next?.report)reports.open(next.root,next.report);else if(next)conversations.select(next.id,false);else{view.threadNotice={thread:'list',text:'No unresolved items in this view. Choose All to continue.'};api.draw();}return;}if(r.conversation&&conversations.ids().includes(r.conversation)){conversations.reveal(r.conversation,false,true);view.result={root:r.root,n:r.n,sha:r.sha,conversation:r.conversation};}else api.openTask(r.board||view.S.projects.find(p=>p.root===r.root)?.issues,r.n,r.root);}});
  const operationView=require('./tray-app.js').createState();
  const operations=require('./tray-app.js').create(operationView,{window,document,menu,workspace:true,render:()=>api.draw()});
  const {session,keyed}=execution.module('workspace.links',{session:function(url){return !!url&&/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(url);},
   // Disclosures, islands and cards keep their DOM node when a notice above them appears or goes; empty text holes become null.
   keyed:function(v){if(Array.isArray(v))return v.map(keyed);if(!v?.props)return v===''?null:v;const p=v.props;if(v.key==null)v.key=p['data-answer']||p['data-resources']||p['data-card']||p['data-view-key']||p.id;if(p.children!=null)p.children=keyed(p.children);return v;}});
  // A1: each route returns its Preact vnodes; called directly, since signal-aware components skip renders when only the plain view changes.
  const pages=execution.module('workspace.routes',{
   conversations:function(){
    conversations.task(null,null);
    const threads=api.threads(),id=conversations.sync(threads,view.answer?'task:'+view.answer.board+'/'+view.answer.task:view.message?'event:'+view.message:null),row=threads.find(t=>t.id===id),selected=row?.id;
    const marks=Object.fromEntries((snapshot.value.messages?.entries||[]).filter(r=>r.kind==='request'&&!r.archived&&!r.read).map(r=>[r.id,'Unread']));
    const c=!selected&&conversations.current(),project=c&&snapshot.value.projects.find(p=>p.root===(c.sent?.root||c.request?.root||c.root)),linked=c&&project?conversations.linked(c,project,view.sourceReports):[];
    const target=view.pmTask?.conversation===c?.id&&linked.find(t=>t.n===view.pmTask.task&&t.state==='ask');
    const activeTarget=target&&conversations.task(project,target),editor=activeTarget&&!activeTarget.body_lazy&&api.answerComposer(project,activeTarget).body;
    const news=linked.length?notifications.rows():[],cards=linked.map(t=>pages.linkedTask(project,t,c,news));cards.unshift(conversations.taskOverflow());
    const outcomes=view.ownerOutcomes;
    if(c&&(c.sent||c.request))cards.push(conversations.linkedTools());
    const ev=!selected&&(snapshot.value.messages?.entries||[]).find(r=>r.kind==='request'&&r.id===conversations.current()?.id);
    const pmResult=view.result&&view.result.conversation===c?.id&&reports.rows(true).find(r=>r.root===view.result.root&&r.n===view.result.n&&r.sha===view.result.sha);
    if(row?.report)reports.focus(row.root,row.report);else if(pmResult)reports.focus(pmResult.root,pmResult.n);
    return [conversations.body({rows:threads,historyTools:conversations.filterName()==='finished'&&h('details',{'data-view-key':'history-sources',class:'completed-scope'},h('summary',null,'History sources'),reports.sources().map(p=>h('p',{key:p.root,'data-history-source':p.root,role:p.reason?'status':undefined},p.name+' · '+p.loaded+' loaded'+(p.reason?' · History incomplete: '+p.reason:''))),h('button',{'data-a':'refresh'},'Refresh history')),newsTools:conversations.filterName()==='new'?notifications.tools():null,selected:selected||(pmResult?c.id:null),marks,cards,outcomes,editor:editor&&h('section',{class:'answer-composer'},h('button',{'data-a':'pm-return'},'Back to PM draft'),editor),resultReason:[...(reports.count()===null?['Result collection incomplete; loaded rows shown, total unknown. '+reports.reason()]:[]),...(conversations.filterName()==='activity'?snapshot.value.projects.filter(p=>!p.alive||p.complete===false).map(p=>p.name+': Task activity unknown; '+(p.error||'project snapshot stale or incomplete')):[])].join(' '),busy:!!view.messageBusy,archive:r=>{if(r.event)api.messageAction(r.archived?'restore':'archive',[r.event.key],r.id);else conversations.sidebar(r.archived?'restore':'archive',r.id,undefined,r.root||'');},notice:ev&&[!ev.read&&!ev.archived&&h('button',{'data-a':'message-read','data-key':ev.key,class:'compact-action notice-read',title:'Mark read','aria-label':'Mark read',disabled:!!view.messageBusy},icons.icon('check')),api.threadNotice('conversation')],detail:()=>pmResult?{readingKey:'result:'+pmResult.root+'/'+pmResult.n+'@'+pmResult.sha,header:h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},pmResult.title),h('button',{'data-a':'result-return'},'Back to conversation')),reading:reports.section(snapshot.value.projects||[])}:selected&&(row?.report?{readingKey:'result:'+row.root+'/'+row.report+'@'+reports.current()?.sha,header:h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},row.title),h('span',{class:'meta'},row.project)),reading:[reports.section(snapshot.value.projects||[]),notifications.history(row.news||[],null,row.id,notifications.taskMessages(row.board,row.task))]}:row?.task?pages.task(row):row?pages.message(row):h('p',{role:'status'},'This thread is unavailable or was deleted. Choose another thread; nothing was sent.')),
     // #1819: the window's one stale-data line sits in the status footer, which is already a live region
     footer:[api.staleText(),api.threadNotice('list'),view.undoMessage&&h('button',{'data-a':'message-restore','data-key':view.undoMessage,disabled:!!view.messageBusy},'Undo archive')]})];
   },
   linkedTask:function(p,t,c,news){
    if(t.report)return h('section',{key:'result:'+t.n,class:'journey-card linked-task'},h('h3',null,t.title),h('p',null,t.report.result||t.report.status),h('button',{'data-report':'open','data-root':t.report.root,'data-n':t.n},'Read result and decide'));
    if(t.body_lazy&&conversations.linkedCurrent()!==t.n)return h('article',{key:'linked:'+t.n,class:'journey-card linked-task','data-task':t.n},h('strong',null,'#'+t.n+' '+t.title),h('p',{class:'meta'},t.state),h('button',{'data-c':'linked-select','data-task':t.n},'Read task'));
    t=conversations.task(p,t);if(t.body_lazy)return h('article',{key:'linked:'+t.n,class:'journey-card linked-task','data-task':t.n},h('h3',null,'#'+t.n+' '+t.title),pages.taskWait(p,t));
    const active=view.pmTask?.conversation===c.id&&view.pmTask.task===t.n,local=answers.load(p.issues,t),pending=local?.op?.action==='start'&&['sending','unknown','accepted'].includes(local.receipt?.state);
    return h('article',{key:'linked:'+t.n,class:'journey-card linked-task','data-phase':t.state,'data-task':t.n},
     h('div',{class:'journey-heading','data-reading-key':'task:'+t.n},h('strong',null,'#'+t.n+' '+t.title),time.stamp(time.seconds(t.since),'Task state changed'),h('span',{class:'journey-state','data-state':t.state},{held:'Brief ready · not assigned',doing:t.executions?.some(x=>x.attempt===t.attempt&&x.state==='running')?'Worker is working':'Worker activity unknown',ask:'Worker asks you',review:t.role==='reviewer'?'Reviewer is checking':'Awaiting review / publication',closed:'Task closed · '+(t.close?.outcome||t.outcome||'outcome unknown')}[t.state]||t.state)),
     h('details',{open:t.state==='held','data-reading-key':'brief:'+t.n,'data-view-key':'linked-brief:'+t.n},h('summary',null,'Brief',time.stamp(null,'Brief','Brief timestamp unavailable in the board adapter')),h('div',{class:'conversation-text'},conversations.taskText(t,t.conversation.brief,'brief',p.root))),
     t.state==='held'&&h('button',{'data-a':'pm-assign','data-task':t.n,disabled:c.archived||pending||!p.alive||p.complete!==true},'Assign task'),
     t.state==='ask'&&h('div',{class:'thread-card'},pages.questionText(p,t),answers.answerSlot({...t,key:p.issues+'/'+t.n})),
     t.state==='ask'&&!active&&!c.archived&&h('button',{'data-a':'pm-answer','data-task':t.n},'Write an answer'),
     t.conversation.verdict&&h('section',{'data-reading-key':'result:'+t.n,'aria-label':t.state==='closed'?'Final result':t.state==='review'?'Candidate result':'Earlier result'},h('p',{class:'meta'},t.state==='closed'?'Final result':t.state==='review'?'Candidate result · awaiting publication':'Earlier result',time.stamp(null,'Result','Result timestamp unavailable in the board adapter')),h('div',{class:'conversation-text'},conversations.taskText(t,t.conversation.verdict,'result',p.root))),
     t.state==='closed'&&reports.rows(true).some(r=>r.root===p.root&&r.n===t.n)&&h('button',{'data-report':'open','data-root':p.root,'data-n':t.n},'Read result and decide'),
     t.result&&h('p',{class:'meta'},'Candidate: '+t.result+' · publication is separate'),
     notifications.history(news.filter(r=>r.root===p.root&&r.n===t.n),(t.history_count??t.history?.length)>0?()=>conversations.history(p,t,answers.answerHistory):null,'linked:'+p.root+'#'+t.n,notifications.taskMessages(p.issues,t.n)),
     answers.answerNotices(p.issues,t.n),pending&&local.receipt.state==='unknown'&&h('button',{'data-a':'answer-retry','data-key':p.root+'#'+local.op.id},'Check delivery'),
     h('button',{'data-a':'workspace','data-board':p.issues,'data-root':p.root,'data-task':t.n},'Task activity'));
   },
   questionText:function(p,t){const text=t.question?.text||t.card||'';return text.length>32768?conversations.taskText(t,text,'question',p.root):null;},
   taskWait:function(p,t){return [h('p',{role:'status'},t.body_reason),h('button',{'data-c':'task-body-retry','data-root':p.root,'data-task':t.n},'Retry card read')];},
   // #1699 A3: the task thread: question card with source choice ids, order and recommendation, then the shared answer composer
   task:function(row){
    const matches=view.S.projects.filter(p=>p.issues===row.board&&(!row.root||p.root===row.root)),p=matches.length===1?matches[0]:null,t=conversations.task(p,p?.tasks?.find(t=>t.n===row.task));
    const composer=!row.archived&&p&&t?.state==='ask'&&!t.body_lazy?api.answerComposer(p,t):{};
    // #1781: Task info above the title; the title row holds the board, Mark read and Archive icons (input-scroll 04)
    const ev=row.event,url=t?.url||row.board+'/'+row.task,busy=!!view.messageBusy;
    return {header:[h('details',{class:'ids conversation-info','data-view-key':'answer-ids:'+row.id},h('summary',null,icons.icon('info'),'Task info'),ev?.archived&&h('button',{'data-a':'message-delete','data-key':ev.key,disabled:busy},'Delete permanently'),h('p',{class:'meta'},'Task #'+row.task+' · attempt '+(t?.attempt??'unknown')+' · '+(t?.role||'Role unknown')+' · '+(t?.host||'Host unknown')+' · question revision '+(t?.revision??row.revision??'unknown'))),
     h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},(p?.name||row.project||'Board')+' · '+row.title),
      h('a',{class:'compact-action',href:url,'data-url':url,title:'Open #'+row.task+' on the board','aria-label':'Open #'+row.task+' on the board'},icons.icon(p?.board_kind==='gitlab'?'gitlab':p?.board_kind==='github'?'github':'external-link')),
      ev&&!row.archived&&!ev.read&&icons.button('check-check','Mark read',{'data-a':'message-read','data-key':ev.key,class:'compact-action',disabled:busy}),
      row.archived?h('button',{'data-a':'thread-restore','data-thread':row.id,disabled:busy},'Restore'):ev&&icons.button('archive','Archive',{'data-a':'message-archive','data-key':ev.key,class:'compact-action',disabled:busy}))],reading:[
     row.archived&&h('p',{role:'status'},'Archived in TaskQ · read-only. Restore before replying.'),
     api.threadNotice(row.id),conversations.taskOverflow(),
     t?.until&&h('p',{class:'meta'},require('./project-blocks.js').parked(t)),
     t&&h('div',{class:'task-destinations'},require('./project-blocks.js').destinations(t).map(d=>d.url?h('a',{href:d.url,'data-url':d.url,'data-destination':d.kind},d.label):h('p',{class:'meta','data-destination':d.kind},d.label+' unavailable: '+d.reason))),
     // #1819: the task's pending operation (Sending, Waiting GitHub, Done, Failed with Retry), as on the menu row
     syncView.row(t?.sync),
     !p||!t?h('p',{role:'status'},'This task is unavailable in the current snapshot. Refresh or open its board; no answer was sent.'):
     [(!p.alive||p.complete===false)&&h('p',{role:'status'},'Project status is unavailable, stale or partial. Refresh or inspect the board; answers are disabled.'),
      // #1773 A3: the task shows as one card in the chat: agent state, then the question; #1781: the thread title above
      // already names number and title (input-scroll 04), so the card keeps them only as its accessible name
      h('article',{class:'journey-card','data-phase':t.state,'aria-label':'#'+t.n+' · '+t.title},h('div',{class:'journey-heading'},time.stamp(time.seconds(t.since),t.state==='ask'?'Question opened':'Task state changed'),h('span',{class:'journey-state','data-state':t.state},icons.icon(t.state==='ask'?'circle-question-mark':'activity'),{ask:'Agent asks you',doing:t.executions?.some(x=>x.attempt===t.attempt&&x.state==='running')?'Worker is working':'Worker activity unknown',review:'In review',failed:'Failed',held:'Held'}[t.state]||api.plain(t.state))),t.body_lazy?pages.taskWait(p,t):row.archived?[h('p',{class:'conversation-text'},t.question?.text||t.card||(!(t.history_count??t.history?.length)?'No recorded question.':''))]:[t.state==='ask'?h('div',{class:'thread-card'},pages.questionText(p,t),answers.answerSlot({...t,key:p.issues+'/'+t.n})):t.state!=='doing'&&!(t.history_count??t.history?.length)&&(answers.answerWon(p.issues,t)||h('p',{role:'status'},t.wait||'There is no open question.'))]),t.state==='ask'&&!t.body_lazy&&composer.body,conversations.worker(row.id,t,p),!row.archived&&answers.answerSteerComposer(p.issues,t),answers.answerNotices(p.issues,t.n)],notifications.history(row.news||[],t&&(t.history_count??t.history?.length)>0?()=>conversations.history(p,t,answers.answerHistory):null,row.id,notifications.taskMessages(row.board,row.task))],tools:composer.tools};
   },
   // an event with no owner conversation (setup, project open) keeps its record as its own thread
   message:function(row){
    const r=row.event,request=r.request,url=request?.url;
    return {header:h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},r.title||'Request')),reading:[time.stamp(r.created,'Request created','Request creation timestamp unavailable'),api.ids(r,[api.eventButtons(r),h('p',{class:'meta'},api.plain(r.state)),session(url)&&h('a',{'data-url':url,href:url},'Open native session')]),api.threadNotice(row.id),
     h('p',null,request?.reason||request?.next||'Deletion incomplete; retry Delete to remove the remaining owned files.')]};
   },
   operations:function(){return operations.operationBody(view.operation);},
   sessions:function(){return pages.conversations();}
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
    if(threadProjection&&projectionSidebar===conversations.sidebarState())return threadProjection;projectionSidebar=conversations.sidebarState();
    view.ownerOutcomes={};
    const S=snapshot.value,taskIndex=new Map(S.projects.map(p=>[p,new Map((p.tasks||[]).map(t=>[t.n,t]))])),rows=new Map(),owned=new Set(conversations.ids()),local=conversations.sidebarState();
    for(const r of S.messages?.entries||[]){
     if(r.kind==='question'){const id='task:'+r.board+'/'+r.task,prior=rows.get(id);if(!prior||prior.event.archived&&!r.archived)rows.set(id,{id,task:r.task,board:r.board,root:r.root,title:'#'+r.task+' '+(r.title||''),project:r.project,revision:r.revision,event:r,archived:r.archived});}
     else if(r.kind==='request'&&!owned.has(r.id))rows.set('event:'+r.key,{id:'event:'+r.key,title:r.title||'Request',root:r.root,updated:r.created,timeContext:'Request created',event:r,archived:r.archived,meta:(r.host||'Host unknown')+' · '+api.plain(r.state),marker:!r.read&&!r.archived?'Unread':''});
    }
    for(const p of S.projects)for(const t of p.tasks||[])if(t.session||t.until||t.state==='ask'){const id='task:'+p.issues+'/'+t.n;if(!rows.has(id))rows.set(id,{id,root:p.root,board:p.issues,task:t.n,title:'#'+t.n+' '+t.title});}
    for(const event of notifications.rows()){const p=S.projects.find(p=>p.root===event.root);if(!p)continue;const id='task:'+p.issues+'/'+event.n,row=rows.get(id)||{id,root:p.root,board:p.issues,task:event.n,title:event.title,project:p.name};row.news=[...(row.news||[]),event];row.unread=row.news.some(r=>!r.read);row.updated??=event.at*1000;rows.set(id,row);}
    const a=view.answer,refs=new Map(conversations.opened().map(r=>[r.id,r]));
    if(a&&!refs.has('task:'+a.board+'/'+a.task))refs.set('task:'+a.board+'/'+a.task,{id:'task:'+a.board+'/'+a.task,root:a.root});
    for(const [id,root] of Object.entries(local.archived||{}))if(!refs.has(id))refs.set(id,{id,root});
    for(const {id,root} of refs.values()){if(!rows.has(id)){const cut=id.lastIndexOf('/'),task=Number(id.slice(cut+1)),board=id.slice(5,cut);rows.set(id,{id,task,board,root,title:'#'+task});}else if(root)rows.get(id).root=root;}
    for(const row of rows.values())if(row.task){
     const matches=S.projects.filter(p=>p.issues===row.board&&(!row.root||p.root===row.root)),p=matches.length===1?matches[0]:null,t=taskIndex.get(p)?.get(row.task);
     if(t?.conversation?.id&&owned.has(t.conversation.id)){const id=t.conversation.id;view.ownerOutcomes[id]={...view.ownerOutcomes[id],unread:!!(view.ownerOutcomes[id]?.unread||row.unread),needsReply:!!(view.ownerOutcomes[id]?.needsReply||t.state==='ask')};if(!refs.has(row.id)){rows.delete(row.id);continue;}row.linkedAlias=true;}
     if(t){row.activityThread=!!(t.session||t.until);row.title='#'+t.n+' '+t.title;row.revision=t.revision;row.updated=time.seconds(t.updated);row.updated_reason=t.updated_reason;}
     if(p){row.root=p.root;row.project=p.name;}
     row.archived=!!row.event?.archived||Object.hasOwn(local.archived||{},row.id);
     // #1773 A4: a row without a known state names only its project, never a bare unknown
     // #1819: an answer's expected state shows here in the same frame, marked unconfirmed until the board shows it
     const state=t?.state||row.event?.state,unconfirmed=t?.sync?.phase==='sending'?' · Sending':'';row.status=!state||state==='ask'&&!row.archived?'':api.plain(state)+unconfirmed;if(!row.status&&row.news?.length&&state!=='ask')row.status=notification.label(row.news[0].kind)+' event';row.meta=(p?.name||row.project||'Board')+(row.status?' · '+row.status:'');
     row.marker=t?.state==='ask'&&!row.archived?'Needs reply':row.event&&!row.event.read&&!row.event.archived?'Unread':'';
    }
    const collected=view.sourceReports=reports.threads();for(const r of collected)if(r.conversation&&owned.has(r.conversation)){const id=r.conversation;view.ownerOutcomes[id]={...view.ownerOutcomes[id],finished:true,unread:!!(view.ownerOutcomes[id]?.unread||r.unread),needsDecision:!!(view.ownerOutcomes[id]?.needsDecision||r.needsDecision)};}const results=collected.filter(r=>!r.conversation||!owned.has(r.conversation)||refs.has(r.id));
    for(const r of results){r.linkedAlias=!!r.conversation&&owned.has(r.conversation);r.archived=Object.hasOwn(local.archived||{},r.id);r.project=S.projects.find(p=>p.root===r.root)?.name;}
    for(const r of results){const row=rows.get(r.id);r.news=row?.news||[];r.unread=r.unread||row?.unread;r.at=r.news[0]?.at;r.kind=r.news[0]?.kind;}for(const row of rows.values()){row.at=row.news?.[0]?.at;row.kind=row.news?.[0]?.kind;}
    const attention=notifications.attention(local);
    for(const row of [...rows.values(),...results])row.unread=attention.fresh.has(row.task?attention.key(row.board,row.task):'owner:'+row.event?.id);
    for(const id of owned)view.ownerOutcomes[id]={...view.ownerOutcomes[id],unread:attention.fresh.has('owner:'+id)};
    const done=new Set(collected.map(r=>r.id));return threadProjection=[...rows.values()].filter(r=>!done.has(r.id)).concat(results);
   },
   openTask:function(board,task,root,external=false){threadProjection=null;const id='task:'+board+'/'+Number(task);conversations.flush();conversations.remember(id,root,true);if(external)conversations.enter(id,api.threads().find(r=>r.id===id)?.archived);else{conversations.show(api.threads().find(r=>r.id===id)?.archived?'archive':'recent');conversations.select(id,false);}},
   // #1699 A2: the menu badge selects the first unread thread, an open question first; nothing is acknowledged
   unread:function(){
    const e=(snapshot.value?.messages?.entries||[]).filter(r=>!r.archived&&!r.read),r=e.find(r=>r.kind==='question'&&r.state==='ask')||e.find(r=>r.kind==='question')||e.find(r=>r.kind==='request');
    if(!r){const report=reports.unread()[0];if(report)reports.open(report.root,report.n);return;}
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
     // #1816: explain an empty draft or a refused send beside the Send button.
     reason:c.files.some(f=>f.pending||f.error)?c.problem:c.reason||why,
     files:answers.fileControls(p.issues,t.n,off),...answers.fileEvents(p.issues,t.n,off),
     // #1781: the bottom row matches the chat composer: command history left; Send, then the answer help icon, right (input-scroll 04)
     tools:h('div',{class:'conversation-tools composer-bottom'},
      h('details',{'data-view-key':'answer-history',class:'command-history-menu'},h('summary',{class:'compact-action',title:'History','aria-label':'History'},icons.icon('history')),h('div',{class:'conversation-history'},(view.S.ai?.history||[]).map((text,i)=>h('button',{'data-a':'answer-history','data-index':i,disabled:off},text)),h('p',{class:'meta'},'History fills the draft only; nothing is sent.'))),
      c.requeue!=null&&h('button',{'data-a':'answer-requeue',disabled:off||!!c.requeue},'Requeue'),h('button',{'data-a':'answer-send',class:'primary-action compact-action',title:c.send,'aria-label':c.send,disabled:off||!!c.problem},icons.icon('arrow-up')),
      h('details',{class:'composer-help','data-view-key':'answer-help'},h('summary',{class:'compact-action',title:'Answer help','aria-label':'Answer help'},icons.icon('info')),h('p',{class:'meta'},'Paste, drop or pick images and files. Send uploads them to this task’s board. TaskQ sets no size or count cap; board and storage limits may refuse a file. Numbers typed here are text; remove files to choose an option.'))),
     onInput:e=>{answers.answerInput(p.issues,t.n,e.currentTarget.value,[e.currentTarget.selectionStart,e.currentTarget.selectionEnd]);api.draw();}}))};
   },
   answerContext:function(){return view.answer||(view.pmTask?.conversation===conversations.current()?.id?view.pmTask:null);},
   answerAct:function(type){const c=api.answerContext();if(c)answers.answerAct(c.board,c.task,type);},
   draw:function(){
    conversations.capture();view.disclosures={...view.disclosures,...state.groups()};
    const s=snapshot.value,active=document.activeElement,focus=state.capture().focus,scroll=view.restoreScroll??menu.querySelector('.content')?.scrollTop??0;
    document.documentElement.style.colorScheme=s?.theme||'light dark';menu.style.setProperty('--size',(s?.font||16)+'px');for(const entry of (s?.palette||'').split(';')){const [key,value]=entry.split(':');if(key&&value)menu.style.setProperty(key,value);}
    const actions=[icons.button('rotate-ccw','Refresh workspace status',{'data-a':'refresh'}),h('details',{class:'shell-overflow','data-view-key':'navigation-actions'},h('summary',{title:'Workspace navigation','aria-label':'Workspace navigation'},icons.icon('ellipsis')),h('nav',{class:'overflow-actions','aria-label':'Workspace'},h('button',{'data-a':'workspace-reload'},'Reload Conversations safely'),[['conversations','Conversations'],['sessions','Work sessions']].map(([route,label])=>h('button',{'data-route':route},label))))];
    renderer.render(keyed([shell.bar({title:routes[view.page]?.title||'Unavailable',home:'Workspace home',back:view.page!=='conversations'||renderer.entries().length>0,actions,windows:[icons.button('always-on-top','Always on top',{'data-a':'workspace-pin','aria-pressed':String(!!view.workspacePin)}),!window.taskqNativeFrame&&icons.button(view.workspaceMaximized?'minimize-2':'maximize-2',view.workspaceMaximized?'Restore window':'Enlarge window',{'data-a':'workspace-enlarge','aria-pressed':String(!!view.workspaceMaximized)}),!window.taskqNativeFrame&&icons.button('x','Close workspace',{'data-a':'hide'})]}),
     h('main',{class:'content'+(view.page==='conversations'?' conversation-content':'')},h('div',{class:'content-body'},view.unavailable&&h('p',{role:'status'},view.unavailable+' The surviving destination is Conversations.'),s?api.route():h('p',null,'Loading workspace. Refresh if status does not arrive.'),[view.navigationError,view.localError,s?.ai?.error].filter(Boolean).map(error=>h('p',{role:'alert'},error)),h('p',{class:'meta',role:'status','aria-live':'polite','aria-atomic':'true','data-status':'workspace'},view.notice||''))),h('footer',{class:'status-footer'},h('span',null,'TaskQ '+(s?.version?'v'+s.version:'version unknown')),h('button',{class:'workspace-resize','data-a':'workspace-resize','aria-label':'Resize workspace',title:'Resize workspace; arrow keys adjust size'}))]),'/'+view.page);
    answers.mountAnswers();if(view.page==='operations')operations.mountResources();if(view.page==='conversations'&&menu.querySelector('.report-card'))reports.mount(menu);state.restore(view.disclosures);if(view.page!=='conversations'&&menu.querySelector('.content').scrollTop!==scroll)menu.querySelector('.content').scrollTop=scroll;view.restoreScroll=null;
    if(active?.isConnected)active.focus({preventScroll:true});else if(focus)state.focus(focus);
    // a new answer field takes its saved caret once
    const field=menu.querySelector('[data-c-field=answer]');if(field&&field!==answerField){answerField=field;const target=api.answerContext(),t=target&&view.S?.projects.find(p=>p.issues===target.board&&(!target.root||p.root===target.root))?.tasks?.find(t=>t.n===target.task),c=t&&answers.answerCompose(target.board,t);if(Array.isArray(c?.caret))field.setSelectionRange(...c.caret);}
    conversations.afterDraw();state.flush();
    api.checkpoint();
   },
   checkpoint:function(){const value={conversation:conversations.recovery(),navigation:state.capture()},text=JSON.stringify(value);if(text!==recoveryText){recoveryText=text;ui.send({a:'workspace-checkpoint',value});}},
   // #1680 A3: a failing route keeps the shell (Home, Back, Close); its class and site reach tray stderr, not the page-wide fence
   route:function(){
    if(view.safe)return h('section',{'aria-label':'Conversations recovery'},h('p',{role:'status'},'Recovery view: the previous reader is unavailable. Drafts and pending receipts are retained. Refresh status or choose Conversations; nothing was sent.'),h('button',{'data-route':'conversations'},'Open Conversations'));
    try{const nodes=(Object.hasOwn(routes,view.page)?pages[view.page]:pages.conversations)();view.routeFault=null;return nodes;}
    catch(error){
     const d=require('./tray-web.js').site(error),key=view.page+' '+d.site;
     if(view.routeFault!==key){view.routeFault=key;ui.send({a:'route-error',route:view.page,...d});}
     return h('p',{role:'alert'},'This page could not be shown. Home, Back and Close still work; refresh status before retrying a command.');
    }
   },
   navigate:function(page,context){
    // #1662 A3: the removed New AI request route opens a new Conversations draft for its project
    view.safe=false;
    if(page==='request'){page='conversations';context={...context,fresh:true};}
    // #1699 A2: the removed Inbox, message and answer routes open their thread in Conversations
    if(page==='messages'){page='conversations';context={...context,thread:'unread'};}
    if(page==='notifications'){page='conversations';context={...context,filter:'new'};}
    if(page==='sessions'){page='conversations';context={...context,filter:'activity'};}
    if(page==='completed'){page='conversations';context={...context,filter:'finished'};}
    if(page==='answer'||page==='message'||page==='results'||page==='reports')page='conversations';
    if(Object.hasOwn(routes,page))view.unavailable=null;
    else {view.unavailable='Requested destination is not available.';page='conversations';}
    renderer.push({...state.capture(),conversation:conversations.context()});view.page=page;
    if(page==='operations'){view.operation={operation:context?.operation,root:context?.root||null};view.operationOrigin=context?.origin?.surface==='settings'?context.origin:null;}
    // the menu entry names 'unread' when it counts events; otherwise the last thread stays
    if(page==='conversations'){
     const r=context?.key&&snapshot.value?.messages?.entries?.find(r=>r.key===context.key);
     if(context?.thread==='centre'){if(snapshot.value?.conversations?.loading)view.centrePending=true;else if(!conversations.hasContext()){const a=notifications.attention();conversations.filter(a.needs.size?'needs':a.fresh.size?'new':'all');}}
     if(context?.thread==='recovery'){view.safe=true;view.answer=null;view.message=null;view.result=null;}
     if(context?.filter)conversations.filter(context.filter,context.criteria);
     if(context?.thread==='unread')api.unread();
     else if(context?.board&&context?.task){const matches=snapshot.value.projects.filter(p=>p.issues===context.board&&(!context.root||p.root===context.root)),root=matches.length===1?matches[0].root:context.root;if(context.sha){conversations.enter('task:'+context.board+'/'+Number(context.task));if(!reports.open(root,Number(context.task),context.sha)){view.answer=null;view.result=null;conversations.select(null,false);view.threadNotice={thread:'list',text:'The requested result SHA is unavailable. Choose the current result to review it; nothing was acknowledged.'};}}else api.openTask(context.board,context.task,root,true);}
     else if(r?.task&&r?.board)api.openTask(r.board,r.task,r.root);
     else if(r)conversations.enter(conversations.ids().includes(r.id)?r.id:'event:'+r.key);
    }
    view.restoreScroll=0;
    if(page==='conversations'&&context?.fresh)conversations.fresh({root:context.root||undefined});
    api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');if(!(page==='conversations'&&context?.fresh))menu.querySelector('h1').focus();state.flush();
   },
   back:function(){if(view.routeFault){view.routeFault=null;view.safe=true;view.answer=null;view.message=null;view.result=null;view.page='conversations';renderer.clear();api.draw();return;}if(view.page==='operations'&&view.operationOrigin){const origin=view.operationOrigin;view.operationOrigin=null;ui.send({a:'workspace-origin',origin});const prior=renderer.pop();if(prior){state.apply(prior);if(prior.conversation)conversations.restoreContext(prior.conversation);}api.draw();return;}const prior=renderer.pop()||{page:'conversations',open:{},scroll:0};state.apply(prior);if(prior.conversation)conversations.restoreContext(prior.conversation);api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');state.focus(prior.focus);},
   receive:function(s){
    threadProjection=null;
    if(Array.isArray(s.projects))s.index_spare=Math.max(0,16*1024*1024-new TextEncoder().encode(JSON.stringify(s)).length);
    if(s.notificationReceipt){notifications.receive(s.notificationReceipt);return;}
    if(s.completed){conversations.closedReceive(s.completed);return;}
    if(s.reportReceipt){reports.hydrate(s.reportReceipt.workspace_local);reports.receive(s.reportReceipt);return;}
    if(s.workspacePin!==undefined||s.workspaceMaximized!==undefined){if(s.workspacePin!==undefined)view.workspacePin=s.workspacePin;if(s.workspaceMaximized!==undefined)view.workspaceMaximized=s.workspaceMaximized;api.draw();return;}
    if(s.dialogueReceipt?.action==='task-body'&&s.dialogueReceipt.state==='accepted')answers.hydrate(s.dialogueReceipt.workspace_local);
    if(conversations.receive(s)) {api.draw();return;}
    if(s.messageReceipt){
     if(s.messageReceipt.id!==view.messageBusy)return;
     const r=s.messageReceipt;view.messageBusy=null;
     const text=r.reason||r.outcomes.map(o=>o.state==='accepted'?({read:'Message marked read',archive:'Archived',restore:'Restored',delete:'Message permanently deleted'}[o.action]||'Message updated'):'Unknown: '+o.reason).join('; '),done=!r.reason&&r.outcomes.every(o=>o.state==='accepted'),{thread,action,keys}=view.messageThread||{};
     view.undoMessage=done&&action==='archive'&&keys.length===1?keys[0]:null;
     if(done)for(const e of snapshot.value.messages?.entries||[])if(keys.includes(e.key)){if(action==='archive'||action==='restore')e.archived=action==='archive';if(action==='read')e.read=true;}
     if(done&&(action==='archive'||action==='delete'))conversations.archived(thread);
     if(done&&(action==='archive'||action==='delete')&&thread!=='conversation'&&(view.answer||view.message)&&thread===(view.answer?'task:'+view.answer.board+'/'+view.answer.task:'event:'+view.message)){view.answer=null;view.message=null;}
     // A7: Restore and Undo open the restored thread in Recent
     if(done&&action==='restore'&&keys.length===1){const e=snapshot.value.messages?.entries?.find(e=>e.key===keys[0]);conversations.show('recent');if(e?.kind==='question')conversations.select('task:'+e.board+'/'+e.task,false);else if(e&&!conversations.reveal(e.id,false))conversations.select('event:'+e.key,false);}
     view.threadNotice={thread:done&&['archive','delete'].includes(action)?'list':thread,text};
     api.draw();return;
    }
    if(s.diagnose){ui.execution.diagnose(s.diagnose.selector,Math.max(0,Math.min(60000,s.diagnose.until_ms-Date.now())),s.diagnose.limit);return;}
    if(['capmsg','focus_receipt','action_error','resources_receipt','aiError'].some(k=>s[k])){operations.receive(s);return;}
    if(s.answerReceipt){const c=api.answerContext(),local=c&&answers.load(c.board,{n:c.task});if(view.pmTask&&local?.op?.id===s.answerReceipt.id&&s.answerReceipt.state==='accepted')view.pmTask=null;answers.answerReceipt(s.answerReceipt);return;}
    if(s.replyFileReceipt){answers.answerFileReceipt(s.replyFileReceipt);return;}
    if(s.viewStateError!==undefined||s.workspaceLocalError!==undefined){if(s.viewStateError!==undefined)view.navigationError=s.viewStateError;if(s.workspaceLocalError!==undefined)view.localError=s.workspaceLocalError;api.draw();return;}
    if(s.workspaceRecoveryReady){ui.send({a:'workspace-recovered'});recoveryText='';api.checkpoint();return;}
    if(s.workspaceRecovery){conversations.restoreRecovery(s.workspaceRecovery.conversation);const v=require('./tray-view-state.js').normalize({schema:1,current:s.workspaceRecovery.navigation},snapshot.value||{},routes,'conversations');state.apply(v.current);api.draw();state.focus(v.current.focus);return;}
    if(s.open){const c=s.open;api.navigate(c.page,c);return;}
    // #1698 A1: only a status snapshot replaces S; the tray also forwards agentq replies and Completed history pages here
    if(Object.hasOwn(s,'agentqError')){operations.receive(s);return;}
    if(!Array.isArray(s.projects))return;
    const first=snapshot.value===null;reports.hydrate(s.workspace_local);reports.sync(s.projects,s.index_spare);snapshot.value=s;view.S=s;operationView.S=s;operations.focusReadback();operations.lifecycleReadback();
    answers.hydrate(s.workspace_local);state.hydrate({...s,navigation:s.workspace_navigation});
    if(first&&view.aiRoot===null)view.aiRoot=s.ai?.root||s.projects?.[0]?.root||null;
    // #1680 A4: request status lives on its conversation (handoff), never on other pages
    view.notice='Workspace status received.';
    if(view.centrePending&&!s.conversations?.loading){view.centrePending=false;if(!conversations.hasContext()){const a=notifications.attention();conversations.filter(a.needs.size?'needs':a.fresh.size?'new':'all');}}
    api.draw();if(first)state.focus(view.restoreFocus);
   },
   bounds:function(){return {conversations:conversations.bounds(),reports:reports.bounds(),answers:answers.bounds(),projectionRows:threadProjection?.length||0};},
   mount:function(){
    ui.connect();window.taskq={receive:api.receive,bounds:api.bounds};conversations.mount();api.draw();
    ui.listen(menu,'click',e=>{
     const link=e.target.closest('a');if(link){e.preventDefault();ui.send({a:'open',url:link.dataset.url||link.href});return;}
     const b=e.target.closest('button');if(!b||b.disabled)return;
     if(b.dataset.a==='thread-restore'){const row=api.threads().find(r=>r.id===b.dataset.thread);if(row?.event?.archived)api.messageAction('restore',[row.event.key],row.id);if(row&&Object.hasOwn(conversations.sidebarState().archived||{},row.id))conversations.sidebar('restore',row.id,undefined,row.root||'');return;}
     if(view.page==='operations'){
      if(b.dataset.route==='recovery'){api.navigate('operations',{operation:'recovery'});return;}
      if(operations.operationAction(b.dataset))return;
     }
     if(b.dataset.a==='pm-return'){view.pmTask=null;api.draw();return;}
     if(b.dataset.a==='pm-answer'||b.dataset.a==='pm-assign'){
      const c=conversations.current(),p=c&&view.S.projects.find(p=>p.root===(c.sent?.root||c.request?.root||c.root)),t=p?.tasks.find(t=>t.n===+b.dataset.task&&t.conversation?.id===c.id);if(!t||c.archived)return;
      if(b.dataset.a==='pm-answer'){view.pmTask={conversation:c.id,board:p.issues,root:p.root,task:t.n};api.draw();menu.querySelector('[data-c-field=answer]')?.focus();}
      else if(t.state==='held'&&(t.controls||[]).includes('start')){const op=require('./task-controls.js').op(t,'start','conversation.'+c.id);answers.save(p.issues,t,{...(answers.load(p.issues,t)||{draft:'',files:[]}),op,receipt:{state:'sending'}});ui.effect('workspace.assign',()=>answers.answerSend(p.issues,op),()=>{});api.draw();}return;
     }
     if(notifications.click(b))return;
     if(conversations.click(b))return;
     if(reports.click(b,snapshot.value?.projects||[]))return;
     if(b.dataset.a==='home'){if(view.page!=='conversations')api.navigate('conversations');return;}
     if(b.dataset.a==='workspace-pin'||b.dataset.a==='workspace-enlarge'){ui.send({a:'workspace-window',action:b.dataset.a==='workspace-pin'?'pin':'enlarge'});return;}
     if(b.dataset.a==='toast-dismiss'){view.toast=null;api.draw();}
     else if(b.dataset.route)api.navigate(b.dataset.route,{section:'conversations'});
     // #1699 A2/A7: selecting a thread acknowledges nothing; its draft, caret and scroll stay with it
     else if(b.dataset.a==='workspace'){api.navigate('conversations',{board:b.dataset.board,root:b.dataset.root,task:+b.dataset.task});}else if(b.dataset.a==='thread'){
      const row=api.threads().find(t=>t.id===b.dataset.thread);if(!row)return;conversations.flush();
      if(row.task)conversations.remember(row.id,row.root,true);conversations.select(row.id,false);
      view.restoreScroll=0;api.draw();state.flush();
     }
     else if(['message-read','message-archive','message-restore','message-delete'].includes(b.dataset.a))api.messageAction(b.dataset.a.slice(8),[b.dataset.key],api.thread(b.dataset.key));
     else if(b.dataset.a==='result-return'){view.result=null;api.draw();}
     else if(b.dataset.a==='new-request')api.navigate('conversations',{fresh:true,root:view.aiRoot});
     else if(b.dataset.a==='answer-send')api.answerAct('submit');
     else if(b.dataset.a==='answer-requeue')api.answerAct('requeue');
     else if(b.dataset.a==='answer-history'){const c=api.answerContext(),text=view.S.ai?.history?.[Number(b.dataset.index)];if(c&&typeof text==='string'){b.closest('details')?.removeAttribute('open');answers.answerInput(c.board,c.task,text,[text.length,text.length]);api.draw();}}
     else if(b.dataset.a==='answer-steer'){const t=answers.project(b.dataset.board)?.tasks?.find(t=>t.n===+b.dataset.task);if(t)answers.answerSteer(b.dataset.board,t);}
     else if(b.dataset.a==='back')api.back();
     else if(b.dataset.a==='hide'){conversations.workerClose();conversations.flush();state.flush();ui.send({a:'hide'});}
     else if(b.dataset.a==='workspace-reload'){conversations.flush();state.flush();if(conversations.recoverable())ui.send({a:'workspace-reload'});else{view.localError='Draft save or command receipt pending. Check delivery before reloading; edits kept in this window.';api.draw();}}
     else if(b.dataset.a==='refresh'){view.notice='Refreshing workspace status…';ui.send({a:'refresh'});api.draw();}
     else if(b.dataset.a==='answer-retry')answers.retry(b.dataset.key);
     else if(b.dataset.a==='sync-retry')ui.send({a:'sync-retry',root:b.dataset.root,id:b.dataset.id});
     else if(b.dataset.a==='answer-dismiss')answers.dismiss(b.dataset.key);
    });
    ui.listen(menu,'input',e=>{if(view.page==='operations')operations.operationInput(e);if(e.target.dataset.cField==='result-reason')reports.reasonInput(e.target.value,[e.target.selectionStart,e.target.selectionEnd]);if(e.target.dataset.cField==='result-date')reports.dateInput(e.target.value);});
    for(const event of ['select','keyup','click'])ui.listen(menu,event,e=>{if(e.target.dataset.cField==='result-reason')reports.reasonInput(null,[e.target.selectionStart,e.target.selectionEnd]);const c=api.answerContext();if(e.target.dataset.cField==='answer'&&c)answers.answerInput(c.board,c.task,null,[e.target.selectionStart,e.target.selectionEnd]);});
    ui.listen(menu,'compositionstart',()=>{composing=true;});ui.listen(menu,'compositionend',()=>{composing=false;});
    ui.listen(menu,'scroll',()=>state.schedule(),true);ui.listen(menu,'toggle',()=>state.schedule(),true);
    ui.listen(document,'keydown',e=>{
     if(e.target.dataset?.cField==='result-reason'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composing&&e.keyCode!==229){e.preventDefault();reports.reasonSend();return;}
     if(e.target.dataset?.cField==='answer'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composing&&e.keyCode!==229){e.preventDefault();api.answerAct('submit');return;}
     if(e.key==='Escape'){conversations.flush();const overflow=menu.querySelector('.shell-overflow[open]');if(overflow){overflow.open=false;overflow.querySelector('summary').focus();return;}conversations.workerClose();state.flush();ui.send({a:'hide'});}
     if(e.altKey&&e.key==='ArrowLeft'&&view.page!=='conversations'){e.preventDefault();api.back();}
    });
    // P-W3: the header drags the window; its controls never start a drag
    let dragFrom=null;
    ui.listen(menu,'pointerdown',e=>{if(e.button===0&&e.target.closest('.head')&&!e.target.closest('button,a,summary,input,select,textarea'))dragFrom={x:e.screenX,y:e.screenY};});
    ui.listen(window,'pointermove',e=>{if(!window.taskqNativeFrame&&dragFrom&&Math.hypot(e.screenX-dragFrom.x,e.screenY-dragFrom.y)>=4){dragFrom=null;ui.send({a:'workspace-window',action:'drag'});}});
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
