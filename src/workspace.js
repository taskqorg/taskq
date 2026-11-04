'use strict';
const copyLabel=require('./copy-label.js');
const ui=require('./ui-runtime.js'),icons=require('./ui-icons.js'),shell=require('./app-shell.js'),{h}=require('preact'),{signal}=require('@preact/signals'),{execution}=ui;
const errors=require('./errors.js'),syncView=require('./sync-view.js'),time=require('./relative-time.js'),notification=require('./notification-panel.js');
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
  const pinReads=require('./task-pins.js').create({snapshot:()=>snapshot.value,live:ref=>(snapshot.value?.projects||[]).some(p=>(p.tasks||[]).some(t=>t.state!=='closed'&&require('./task-views.js').identity(p)+'#'+t.n===ref)),send:ui.send,remove:ref=>ui.send({a:'task-pin',ref,pinned:false}),draw:()=>{threadProjection=null;api.draw();}});
  const notifications=notification.create({view,send:ui.send,draw:()=>api.draw(),messages:(action,rows)=>api.messageAction(action,rows.map(r=>r.key),'list'),reports:{rows:()=>reports.rows(),count:()=>reports.count(),markRead:rows=>reports.markRead(rows),feedback:()=>reports.feedback(),readFailure:()=>reports.readFailure()}});
  // #1662 A3: task answers use the workspace's one composer; a card change repaints it once per tick
  const pm=require('./pm-composer.js');
  const answers=require('./workspace-answers.js').create(view,{menu,document,send:m=>{if(m.a==='answer-local'){api.checkpoint();state.flush();}ui.send(m);},draw:()=>api.draw(),composer:true,steer:pm.composer,read:(p,t)=>conversations.task(p,t),historyText:(t,text,source,root)=>conversations.taskText(t,text,source,root),changed:()=>{if(view.page==='conversations'&&view.answer&&!redraw)redraw=ui.defer(()=>{redraw=null;api.draw();},0);}});
  const conversations=require('./conversations.js').create({window,document,menu,draw:()=>api.draw(),checkpoint:()=>api.checkpoint(),navigate:(...args)=>api.navigate(...args),onSelect:id=>{if(view.safe)return;if(view.result?.conversation!==id)view.result=null;const row=api.threads().find(t=>t.id===id);view.answer=row?.task?{root:row.root,board:row.board,task:row.task,revision:row.revision}:null;view.message=row?.event&&!row.task?row.event.key:null;}});
  const reports=require('./reports.js').create({draw:()=>{threadProjection=null;api.draw();},onMove:step=>conversations.move(step),navigation:r=>conversations.navigation(r),reading:(key,r)=>conversations.reading(key,r),transcript:r=>conversations.closed({...r,news:notifications.rows().filter(e=>e.root===r.root&&e.n===r.n)},answers.answerHistory),onOpen:r=>{if(r.next){const next=conversations.nextUnresolved(api.threads());if(next?.report)reports.open(next.root,next.report);else if(next)conversations.select(next.id,false);else{view.threadNotice={thread:'list',text:'No unresolved items in this view. Choose All to continue.'};api.draw();}return;}if(r.conversation&&conversations.ids().includes(r.conversation)){conversations.reveal(r.conversation,false,true);view.result={root:r.root,n:r.n,sha:r.sha,conversation:r.conversation};}else api.openTask(r.board||view.S.projects.find(p=>p.root===r.root)?.issues,r.n,r.root);}});
  const operationView=require('./tray-app.js').createState();
  const operations=require('./tray-app.js').create(operationView,{window,document,menu,workspace:true,render:()=>api.draw(),navigate:(page,context)=>api.navigate(page,context)});
  const session=url=>!!url&&/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(url);
  // Disclosures, islands and cards keep their DOM node when a notice above them appears or goes; empty text holes become null.
  function keyed(v){if(Array.isArray(v))return v.map(keyed);if(!v?.props)return v===''?null:v;const p=v.props;if(v.key==null)v.key=p['data-answer']||p['data-resources']||p['data-card']||p['data-view-key']||p.id;if(p.children!=null)p.children=keyed(p.children);return v;}
  // A1: each route returns its Preact vnodes; called directly, since signal-aware components skip renders when only the plain view changes.
  const pages={
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
    return [conversations.body({rows:threads,historyTools:conversations.filterName()==='finished'&&h('details',{'data-view-key':'history-sources',class:'completed-scope'},h('summary',null,'History sources'),reports.sources().map(p=>h('p',{key:p.root,'data-history-source':p.root},p.name+' · '+p.loaded+' loaded')),h('button',{'data-a':'refresh'},'Refresh history')),newsTools:conversations.filterName()==='new'?notifications.tools():null,selected:selected||(pmResult?c.id:null),marks,cards,outcomes,editor:editor&&h('section',{class:'answer-composer'},h('button',{'data-a':'pm-return'},'Back to PM draft'),editor),resultReason:reports.count()===null?'Collection incomplete; loaded rows shown, total unknown.':'',busy:!!view.messageBusy,archive:r=>{if(r.event)api.messageAction(r.archived?'restore':'archive',[r.event.key],r.id);else conversations.sidebar(r.archived?'restore':'archive',r.id,undefined,r.root||'');},notice:ev&&[!ev.read&&!ev.archived&&h('button',{'data-a':'message-read','data-key':ev.key,class:'compact-action notice-read',title:'Mark read','aria-label':'Mark read',disabled:!!view.messageBusy},icons.icon('check')),api.threadNotice('conversation')],detail:()=>pmResult?{readingKey:'result:'+pmResult.root+'/'+pmResult.n+'@'+pmResult.sha,header:h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},copyLabel.node(copyLabel.task(pmResult.project||project?.name||'Board',pmResult.n)),' ',pmResult.title.replace(/^#\d+\s*/,'')),h('button',{'data-a':'result-return'},'Back to conversation')),reading:reports.section(snapshot.value.projects||[])}:selected&&(row?.report?{readingKey:'result:'+row.root+'/'+row.report+'@'+reports.current()?.sha,header:pages.resultHeader(row),reading:reports.section(snapshot.value.projects||[])}:row?.unavailable?pages.unavailablePin(row):row?.task?pages.task(row):row?pages.message(row):h('p',{role:'status'},'This thread is unavailable or was deleted. Choose another thread; nothing was sent.')),
     // #1819: the window's one stale-data line sits in the status footer, which is already a live region
     footerText:[api.staleText(),view.threadNotice?.thread==='list'&&view.threadNotice.text],footerAlert:false,footerActions:view.undoMessage&&icons.button('undo-2','Undo archive',{'data-a':'message-restore','data-key':view.undoMessage,class:'compact-action',disabled:!!view.messageBusy})})];
   },
   linkedTask:function(p,t,c,news){
    if(t.report)return h('section',{key:'result:'+t.n,class:'journey-card linked-task'},h('h3',null,t.title),h('p',null,t.report.result||t.report.status),h('button',{'data-report':'open','data-root':t.report.root,'data-n':t.n},'Read result and decide'));
    if(t.body_lazy&&conversations.linkedCurrent()!==t.n)return h('article',{key:'linked:'+t.n,class:'journey-card linked-task','data-task':t.n},h('strong',null,copyLabel.node(copyLabel.task(p.name,t.n)),' '+t.title),h('p',{class:'meta'},t.state),h('button',{'data-c':'linked-select','data-task':t.n},'Read task'));
    t=conversations.task(p,t);if(t.body_lazy)return h('article',{key:'linked:'+t.n,class:'journey-card linked-task','data-task':t.n},h('h3',null,copyLabel.node(copyLabel.task(p.name,t.n)),' '+t.title),pages.taskWait(p,t));
    const active=view.pmTask?.conversation===c.id&&view.pmTask.task===t.n,local=answers.load(p.issues,t),pending=local?.op?.action==='start'&&['sending','unknown','accepted'].includes(local.receipt?.state);
    return h('article',{key:'linked:'+t.n,class:'journey-card linked-task','data-phase':t.state,'data-task':t.n},
     h('div',{class:'journey-heading','data-reading-key':'task:'+t.n},h('strong',null,copyLabel.node(copyLabel.task(p.name,t.n)),' '+t.title),time.stamp(time.seconds(t.since),'Task state changed'),h('span',{class:'journey-state','data-state':t.state},{held:'Brief ready · not assigned',doing:t.executions?.some(x=>x.attempt===t.attempt&&x.state==='running')?'Worker is working':'Worker activity unknown',ask:'Worker asks you',review:t.role==='reviewer'?'Reviewer is checking':'Awaiting review / publication',closed:'Task closed · '+(t.close?.outcome||t.outcome||'outcome unknown')}[t.state]||t.state)),
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
    const matches=view.S.projects.filter(p=>p.issues===row.board&&(!row.root||p.root===row.root)),p=matches.length===1?matches[0]:null,t=conversations.task(p,p?.tasks?.find(t=>t.n===row.task)),url=t?.url||row.board+'/'+row.task;
    const composer=!row.archived&&p&&t?.state==='ask'&&!t.body_lazy?api.answerComposer(p,t):{},history=t&&p?conversations.history(p,t,answers.answerHistory,true):{nodes:[],tools:null},block=require('./conversations.js').block;
    const source=t,question=t?.state==='ask'&&!t.body_lazy?block({role:'question',label:t.role==='reviewer'?'Reviewer question':'Worker question',text:t.question?.text||t.card,updated:time.seconds(t.since),attempt:t.attempt,session:source?.session},[row.archived?h('p',{class:'conversation-text'},t.question?.text||t.card):[pages.questionText(p,t),answers.answerSlot({...t,key:p.issues+'/'+t.n}),composer.body]],{'data-question':t.revision,'data-reading-key':'question:'+t.n+':'+t.revision,class:'thread-card current-question'}):null;
    const events=(row.news||[]).filter(r=>r.source==='event').map(r=>block({role:'event',label:notification.label(r.kind),text:r.title,updated:time.seconds(r.at),attempt:r.event},h('p',null,r.title),{'data-event':r.id,'data-reading-key':'result:'+row.task+':'+r.event}));
    view.taskDashboard??=require('./task-views.js').normalize(view.S.navigation?.dashboard);
    return {header:h('div',{class:'conversation-tools thread-head sticky-object-head'},h('h2',{class:'thread-title'},copyLabel.node(copyLabel.task(p?.name||row.project||'Board',row.task)),' ',row.title.replace(/^#\d+\s*/,'')),h('span',{class:'object-status'},t?.state_name||t?.state||'Status unavailable'),h('a',{class:'compact-action',href:url,'data-url':url,title:'Open #'+row.task+' on the board','aria-label':'Open #'+row.task+' on the board'},icons.icon(p?.board_kind==='gitlab'?'gitlab':p?.board_kind==='github'?'github':'external-link')),
     t&&p&&!row.archived&&h('span',{class:'row-actions',dangerouslySetInnerHTML:{__html:require('./task-controls.js').render(t,{key:p.issues+'/'+t.n})}}),
     p&&pages.pinButton(row),
     row.pinRef&&!t&&h('button',{'data-a':'pin-retry','data-ref':row.pinRef},'Retry pinned task'),
     h('button',{'data-a':'question-jump','data-task':row.task,'data-question':t?.state==='ask'?t.revision:undefined},icons.icon('message-circle-question'),'Questions'),
     icons.button(row.archived?'archive-restore':'archive',row.archived?'Restore':'Archive',{'data-c':'row-archive','data-id':row.id,onClick:()=>{if(row.event)api.messageAction(row.archived?'restore':'archive',[row.event.key],row.id);else conversations.sidebar(row.archived?'restore':'archive',row.id,undefined,p?.root||row.root||'');}})),
     reading:[row.archived&&h('p',{role:'status'},'Archived in TaskQ · read-only. Restore before replying.'),
      h('section',{class:'object-metadata','aria-label':'Task info'},h('h3',null,'Task info'),t&&h('dl',null,['Runtime','Host','Role','Session','Attempt','Revision','Created','Current stage elapsed'].flatMap((label,i)=>[h('dt',null,label),h('dd',null,[t.runtime,t.host,t.role,t.session,t.attempt,t.revision,time.seconds(t.created)?time.stamp(time.seconds(t.created),'Created'):'Unavailable',t.age][i]??'Unavailable')])),t?.until&&h('p',null,require('./project-blocks.js').parked(t)),t?.result&&h('p',{class:'meta'},'Candidate '+t.result),[t?.wait,t?.publication?.reason,t?.failed,t?.later].filter(Boolean).map(text=>h('p',{role:'status'},text)),t&&require('./task-health.js').details(t),t&&(t.blocked||[]).map(b=>h('p',null,h('a',{href:b.url,'data-url':b.url},b.ref),' · '+b.reason)),t&&h('div',{class:'task-destinations'},require('./project-blocks.js').destinations(t).map(d=>d.url?h('a',{href:d.url,'data-url':d.url},d.label):h('p',{class:'meta'},d.label+' unavailable: '+d.reason))),t?.brief&&h('details',{'data-view-key':'task-brief:'+row.id},h('summary',null,'Task brief'),conversations.taskText(t,t.brief,'brief',p.root)),t?.steps?.length>0&&h('ol',{class:'task-steps'},t.steps.map(step=>h('li',null,step.state,' · ',step.age||'Duration unavailable')))),
      api.threadNotice(row.id),conversations.taskOverflow(),conversations.targetNotice(),syncView.row(t?.sync),history.tools,
      !p?h('p',{role:'status'},'Project unavailable; no source substituted. No input sent.'):!t?conversations.closed({root:p.root,board:p.issues,n:row.task,sha:'exact',news:row.news},answers.answerHistory):t.body_lazy?pages.taskWait(p,t):conversations.worker(row.id,t,p,[...history.nodes,...events,question].filter(Boolean)),
      !row.archived&&p&&t&&answers.answerSteerComposer(p.issues,t),p&&answers.answerNotices(p.issues,row.task)]};
   },
   unavailablePin:function(row){return {header:h('div',{class:'conversation-tools thread-head sticky-object-head'},h('h2',{class:'thread-title'},row.title),icons.button('pin-off','Unpin task',{'data-a':'global-task-pin','data-ref':row.pinRef,'data-pinned':'false'})),reading:[h('p',{role:'status'},'Registered board unavailable; no source substituted. '+row.pinRef),h('button',{'data-a':'refresh'},'Retry pinned task')]};},
   pinButton:function(row){const p=view.S.projects.find(p=>p.root===row.root&&p.issues===row.board),ref=p&&require('./task-views.js').identity(p)+'#'+row.task,pinned=!!view.taskDashboard?.pins?.some(pin=>pin.ref===ref);return p&&icons.button(pinned?'pin-off':'pin',pinned?'Unpin task':'Pin task',{'data-a':'global-task-pin','data-root':p.root,'data-task':row.task,'data-ref':ref,'data-pinned':String(!pinned),'aria-pressed':String(pinned)});},
   resultHeader:function(row){return h('div',{class:'conversation-tools thread-head sticky-object-head'},h('h2',{class:'thread-title'},copyLabel.node(copyLabel.task(row.project||snapshot.value.projects.find(p=>p.root===row.root)?.name||'Board',row.task||row.report)),' ',row.title.replace(/^#\d+\s*/,'')),h('span',{class:'meta'},(row.project||'Board')+' · Finished'),h('a',{href:row.board+'/'+row.task,'data-url':row.board+'/'+row.task,'aria-label':'Open task on the board'},icons.icon('external-link')),h('button',{'data-a':'question-jump','data-task':row.task},icons.icon('message-circle-question'),'Questions'),pages.pinButton(row));},
   // an event with no owner conversation (setup, project open) keeps its record as its own thread
   message:function(row){
    const r=row.event,request=r.request,url=request?.url;
    return {header:h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title'},r.title||'Request')),reading:[time.stamp(r.created,'Request created','Request creation timestamp unavailable'),api.ids(r,[api.eventButtons(r),h('p',{class:'meta'},api.plain(r.state)),session(url)&&h('a',{'data-url':url,href:url},'Open native session')]),api.threadNotice(row.id),
     h('p',null,request?.reason||request?.next||'Deletion incomplete; retry Delete to remove the remaining owned files.')]};
   },
   operations:function(){return operations.operationBody(view.operation);},
   sessions:function(){return pages.conversations();}
  };
  const api={
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
   staleText:function(){return syncView.stale(snapshot.value);},
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
    for(const p of S.projects)for(const t of p.tasks||[])if(!t.dup){const id='task:'+p.issues+'/'+t.n;if(!rows.has(id))rows.set(id,{id,root:p.root,board:p.issues,task:t.n,title:'#'+t.n+' '+t.title});}
    for(const event of notifications.rows()){const p=S.projects.find(p=>p.root===event.root);if(!p)continue;const id='task:'+p.issues+'/'+event.n,row=rows.get(id)||{id,root:p.root,board:p.issues,task:event.n,title:event.title,project:p.name};row.news=[...(row.news||[]),event];row.unread=row.news.some(r=>!r.read);row.updated??=event.at*1000;rows.set(id,row);}
    const a=view.answer,refs=new Map(conversations.opened().map(r=>[r.id,r]));
    pinReads.sync(view.taskDashboard?.pins||[]);
    for(const pin of view.taskDashboard?.pins||[]){const cut=pin.ref.lastIndexOf('#'),n=Number(pin.ref.slice(cut+1)),p=S.projects.find(p=>require('./task-views.js').identity(p)===pin.ref.slice(0,cut)),read=pinReads.read(pin);if(!p||!Number.isSafeInteger(n)||n<=0){const id='unavailable-pin:'+pin.ref;rows.set(id,{id,pinRef:pin.ref,unavailable:true,title:pin.title||pin.ref,status:'Registered board unavailable',meta:pin.ref});continue;}const id='task:'+p.issues+'/'+n;refs.set(id,{id,root:p.root});if(!rows.has(id))rows.set(id,{id,root:p.root,board:p.issues,task:n,title:pin.title||'#'+n,status:'Task source unavailable'});Object.assign(rows.get(id),{pinRef:pin.ref,...(read?{state:read.value?.state,taskRow:{...read.value,n,title:read.value?.title||pin.title,state:read.value?.state||'unknown',state_name:read.loading?'Reading board…':read.reason||read.value?.close||'Status unavailable'}}:{})});}
    if(a&&!refs.has('task:'+a.board+'/'+a.task))refs.set('task:'+a.board+'/'+a.task,{id:'task:'+a.board+'/'+a.task,root:a.root});
    for(const [id,root] of Object.entries(local.archived||{}))if(!refs.has(id))refs.set(id,{id,root});
    for(const {id,root} of refs.values()){if(!rows.has(id)){const cut=id.lastIndexOf('/'),task=Number(id.slice(cut+1)),board=id.slice(5,cut);rows.set(id,{id,task,board,root,title:'#'+task});}else if(root)rows.get(id).root=root;}
    for(const row of rows.values())if(row.task){
     const matches=S.projects.filter(p=>p.issues===row.board&&(!row.root||p.root===row.root)),p=matches.length===1?matches[0]:null,t=taskIndex.get(p)?.get(row.task);
     if(t?.conversation?.id&&owned.has(t.conversation.id)){const id=t.conversation.id;view.ownerOutcomes[id]={...view.ownerOutcomes[id],unread:!!(view.ownerOutcomes[id]?.unread||row.unread),needsReply:!!(view.ownerOutcomes[id]?.needsReply||t.state==='ask')};if(!refs.has(row.id)){rows.delete(row.id);continue;}row.linkedAlias=true;}
     if(t){row.taskRow={...t,root:p.root,key:p.issues+'/'+t.n};row.state=t.state;row.heartbeat=t.heartbeat;row.activityThread=!!(t.session||t.worker||t.until);row.title='#'+t.n+' '+t.title;row.revision=t.revision;row.updated=time.seconds(t.updated);row.updated_reason=t.updated_reason;}
     if(p){row.root=p.root;row.project=p.name;}
     row.archived=!!row.event?.archived||Object.hasOwn(local.archived||{},row.id);
     // #1773 A4: a row without a known state names only its project, never a bare unknown
     // #1819: an answer's expected state shows here in the same frame, marked unconfirmed until the board shows it
     const state=t?.state||row.event?.state,unconfirmed=t?.sync?.phase==='sending'?' · Sending':'';row.status=!state||state==='ask'&&!row.archived?'':api.plain(state)+unconfirmed;if(!row.status&&row.news?.length&&state!=='ask')row.status=notification.label(row.news[0].kind)+' event';row.meta=(p?.name||row.project||'Board')+(row.status?' · '+row.status:'');
     row.marker=t?.state==='ask'&&!row.archived?'Needs reply':row.event&&!row.event.read&&!row.event.archived?'Unread':'';
    }
    const collected=view.sourceReports=reports.threads();for(const r of collected)if(r.conversation&&owned.has(r.conversation)){const id=r.conversation;view.ownerOutcomes[id]={...view.ownerOutcomes[id],finished:true,unread:!!(view.ownerOutcomes[id]?.unread||r.unread),needsDecision:!!(view.ownerOutcomes[id]?.needsDecision||r.needsDecision)};}const results=collected.filter(r=>!r.conversation||!owned.has(r.conversation)||refs.has(r.id));
    for(const r of results){r.linkedAlias=!!r.conversation&&owned.has(r.conversation);r.archived=Object.hasOwn(local.archived||{},r.id);r.project=S.projects.find(p=>p.root===r.root)?.name;}
    for(const r of results){const row=rows.get(r.id);r.pinRef=row?.pinRef;r.news=row?.news||[];r.unread=r.unread||row?.unread;r.at=r.news[0]?.at;r.kind=r.news[0]?.kind;}for(const row of rows.values()){row.at=row.news?.[0]?.at;row.kind=row.news?.[0]?.kind;}
    const attention=notifications.attention(local);
    for(const row of [...rows.values(),...results])row.unread=attention.fresh.has(row.task?attention.key(row.board,row.task):'owner:'+row.event?.id);
    for(const id of owned)view.ownerOutcomes[id]={...view.ownerOutcomes[id],unread:attention.fresh.has('owner:'+id)};
    const done=new Set(collected.map(r=>r.id));return threadProjection=[...rows.values()].filter(r=>!done.has(r.id)).concat(results);
   },
   openTask:function(board,task,root,external=false){threadProjection=null;const id='task:'+board+'/'+Number(task);conversations.flush();conversations.remember(id,root,true);const row=api.threads().find(r=>r.id===id);if(row?.pinRef)pinReads.opened(row.pinRef);if(external)conversations.enter(id,row?.archived);else{conversations.show(row?.archived?'archive':'recent');conversations.select(id,false);}},
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
   ...execution.module('workspace.instance',{
   draw:function(){
    conversations.capture();view.disclosures={...view.disclosures,...state.groups()};
    const s=snapshot.value,active=document.activeElement,focus=state.capture().focus,scroll=view.restoreScroll??menu.querySelector('.content')?.scrollTop??0;
    document.documentElement.style.colorScheme=s?.theme||'light dark';menu.style.setProperty('--size',(s?.font||16)+'px');for(const entry of (s?.palette||'').split(';')){const [key,value]=entry.split(':');if(key&&value)menu.style.setProperty(key,value);}
    const actions=[icons.button('rotate-ccw','Refresh workspace status',{'data-a':'refresh'}),h('details',{class:'shell-overflow','data-view-key':'navigation-actions'},h('summary',{title:'Workspace navigation','aria-label':'Workspace navigation'},icons.icon('ellipsis')),h('nav',{class:'overflow-actions','aria-label':'Workspace'},h('button',{'data-a':'workspace-reload'},'Reload Conversations safely'),[['conversations','Conversations']].map(([route,label])=>h('button',{'data-route':route},label))))];
    renderer.render(keyed([shell.bar({errors:errors.entry(s,[view.navigationError,view.localError,s?.ai?.error,view.unavailable,operationView.aiError,operationView.workspaceError,operationView.agentqError]),title:routes[view.page]?.title||'Unavailable',home:'Workspace home',back:view.page!=='conversations'||renderer.entries().length>0,actions,windows:[icons.button('always-on-top','Always on top',{'data-a':'workspace-pin','aria-pressed':String(!!view.workspacePin)}),!window.taskqNativeFrame&&icons.button(view.workspaceMaximized?'minimize-2':'maximize-2',view.workspaceMaximized?'Restore window':'Enlarge window',{'data-a':'workspace-enlarge','aria-pressed':String(!!view.workspaceMaximized)}),!window.taskqNativeFrame&&icons.button('x','Close workspace',{'data-a':'hide'})]}),
     h('main',{class:'content'+(view.page==='conversations'?' conversation-content':'')},h('div',{class:'content-body'},view.page!=='conversations'&&view.unavailable&&h('p',{role:'status'},view.unavailable+' The surviving destination is Conversations.'),s?api.route():h('p',null,'Loading workspace. Refresh if status does not arrive.'),h('p',{class:'meta',role:'status','aria-live':'polite','aria-atomic':'true','data-status':'workspace'},view.notice||''))),h('footer',{class:'status-footer'},h('span',null,'TaskQ ',copyLabel.node(s?.version?'v'+s.version:'version unknown')),h('button',{class:'workspace-resize','data-a':'workspace-resize','aria-label':'Resize workspace',title:'Resize workspace; arrow keys adjust size'}))]),'/'+view.page);
    answers.mountAnswers();if(view.page==='operations')operations.mountResources();if(view.page==='conversations'&&menu.querySelector('.report-card'))reports.mount(menu);state.restore(view.disclosures);if(view.page!=='conversations'&&menu.querySelector('.content').scrollTop!==scroll)menu.querySelector('.content').scrollTop=scroll;view.restoreScroll=null;
    // Background redraws leave an unchanged native editor (and its IME owner) alone.
    if(document.hasFocus()&&document.activeElement!==active){if(active?.isConnected)active.focus({preventScroll:true});else if(focus)state.focus(focus);}
    // a new answer field takes its saved caret once
    const field=menu.querySelector('[data-c-field=answer],[data-c-field=steer]');if(field&&field!==answerField){answerField=field;const target=api.answerContext(),t=target&&view.S?.projects.find(p=>p.issues===target.board&&(!target.root||p.root===target.root))?.tasks?.find(t=>t.n===target.task),c=t&&(field.dataset.cField==='steer'?answers.load(target.board,t):answers.answerCompose(target.board,t));if(Array.isArray(c?.caret))field.setSelectionRange(...c.caret);}
    conversations.afterDraw();state.flush();
    api.checkpoint();
   },
   checkpoint:function(){const value={conversation:conversations.recovery(),answers:answers.recovery(),localError:view.localError||null,navigation:state.capture()},text=JSON.stringify(value);if(text!==recoveryText){recoveryText=text;ui.send({a:'workspace-checkpoint',value});}},
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
    if(page==='conversations'&&context?.target)conversations.target(conversations.context().selected,context.target,false);
    else if(page==='conversations'&&typeof context?.event==='string'&&context.board&&context.task){
     const matches=notifications.rows().filter(r=>r.source==='event'&&r.n===Number(context.task)&&(!context.root||r.root===context.root)&&view.S.projects.some(p=>p.root===r.root&&p.issues===context.board)&&(r.id===context.event||r.at+' #'+r.n+' '+r.kind+' '+r.event===context.event));
     conversations.target(conversations.context().selected,{kind:'event',id:matches.length===1?matches[0].id:context.event},false);
    }
    view.restoreScroll=0;
    if(page==='conversations'&&context?.fresh)conversations.fresh({root:context.root||undefined});
    const restore=page==='conversations'&&!context?.fresh&&conversations.resume();
    api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');if(!restore&&!(page==='conversations'&&context?.fresh))menu.querySelector('h1').focus();state.flush();
   },
   back:function(){if(view.routeFault){view.routeFault=null;view.safe=true;view.answer=null;view.message=null;view.result=null;view.page='conversations';renderer.clear();api.draw();return;}if(view.page==='operations'&&view.operationOrigin){const origin=view.operationOrigin;view.operationOrigin=null;ui.send({a:'workspace-origin',origin});const prior=renderer.pop();if(prior){state.apply(prior);if(prior.conversation)conversations.restoreContext(prior.conversation);}api.draw();return;}const prior=renderer.pop()||{page:'conversations',open:{},scroll:0};state.apply(prior);if(prior.conversation)conversations.restoreContext(prior.conversation);api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');state.focus(prior.focus);},
   receive:function(s){
    threadProjection=null;
    if(Array.isArray(s.projects)){s=errors.retain(s,snapshot.value);s.index_spare=Math.max(0,16*1024*1024-new TextEncoder().encode(JSON.stringify(s)).length);}
    if(s.dashboard||s.navigation?.dashboard){view.taskDashboard=require('./task-views.js').normalize(s.dashboard||s.navigation.dashboard);conversations.setTaskPins(view.taskDashboard.pins);}
    if(s.dashboard){if(snapshot.value)snapshot.value.navigation={...snapshot.value.navigation,dashboard:view.taskDashboard};api.draw();return;}
    if(s.taskPinError){view.notice=s.taskPinError;api.draw();return;}
    if(s.notificationReceipt){notifications.receive(s.notificationReceipt);return;}
    if(s.completed){if(!pinReads.receive(s.completed))conversations.closedReceive(s.completed);return;}
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
    if(['capmsg','focus_receipt','action_error','resources_receipt'].some(k=>s[k])||['aiError','workspaceError'].some(k=>Object.hasOwn(s,k))){operations.receive(s);return;}
    if(s.answerReceipt){const c=api.answerContext(),local=c&&answers.load(c.board,{n:c.task});if(view.pmTask&&local?.op?.id===s.answerReceipt.id&&s.answerReceipt.state==='accepted')view.pmTask=null;answers.answerReceipt(s.answerReceipt);return;}
    if(s.replyFileReceipt){answers.answerFileReceipt(s.replyFileReceipt);return;}
    if(s.viewStateError!==undefined||s.workspaceLocalError!==undefined){if(s.viewStateError!==undefined)view.navigationError=s.viewStateError;if(s.workspaceLocalError!==undefined)view.localError=s.workspaceLocalError;api.draw();return;}
    if(s.workspaceRecoveryReady){ui.send({a:'workspace-recovered'});recoveryText='';api.checkpoint();return;}
    if(s.workspaceRecovery){answers.restoreRecovery(s.workspaceRecovery.answers);answerField=null;view.localError=s.workspaceRecovery.localError;conversations.restoreRecovery(s.workspaceRecovery.conversation);const v=require('./tray-view-state.js').normalize({schema:1,current:s.workspaceRecovery.navigation},snapshot.value||{},routes,'conversations');state.apply(v.current);const restore=v.current.page==='conversations'&&conversations.resume();api.draw();if(!restore)state.focus(v.current.focus);return;}
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
    copyLabel.mount(menu);ui.connect();window.taskq={receive:api.receive,bounds:api.bounds};conversations.mount();api.draw();
    ui.listen(menu,'click',e=>{
     const link=e.target.closest('a');if(link){e.preventDefault();ui.send({a:'open',url:link.dataset.url||link.href});return;}
     const b=e.target.closest('button,.task-target[role=button]');if(!b||b.disabled||b.getAttribute('aria-disabled')==='true')return;
     if(b.dataset.a==='thread-restore'){const row=api.threads().find(r=>r.id===b.dataset.thread);if(row?.event?.archived)api.messageAction('restore',[row.event.key],row.id);if(row&&Object.hasOwn(conversations.sidebarState().archived||{},row.id))conversations.sidebar('restore',row.id,undefined,row.root||'');return;}
     if(view.page==='operations'){
      if(b.dataset.route==='recovery'){api.navigate('operations',{operation:'recovery'});return;}
      if(operations.operationAction(b.dataset))return;
     }
     if(b.dataset.a==='control'){const target=api.answerContext(),p=target&&answers.project(target.board),t=p?.tasks?.find(t=>t.n===target.task);if(t&&p.alive&&p.complete!==false&&!conversations.sidebarState().archived?.['task:'+p.issues+'/'+t.n]&&(t.controls||[]).includes(b.dataset.control))ui.effect('workspace.task-control',()=>answers.answerSend(p.issues,require('./task-controls.js').op(t,b.dataset.control,'conversation')),()=>{});api.draw();return;}
     if(b.dataset.a==='global-task-pin'){ui.send(b.dataset.pinned==='true'?{a:'task-pin',root:b.dataset.root,task:Number(b.dataset.task),pinned:true}:{a:'task-pin',ref:b.dataset.ref,...(b.dataset.root?{root:b.dataset.root,task:Number(b.dataset.task)}:{}),pinned:false});return;}
     if(b.dataset.a==='pin-retry'){const pin=view.taskDashboard?.pins.find(p=>p.ref===b.dataset.ref);if(pin){pinReads.read(pin,true);api.draw();}return;}
     if(b.dataset.a==='unavailable-pin'){conversations.select(b.dataset.id,false);api.draw();return;}
     if(b.dataset.a==='question-jump'){const q=b.dataset.question?Number(b.dataset.question):Number(menu.querySelector('[data-question]')?.dataset.question);if(q&&view.answer)conversations.target('task:'+view.answer.board+'/'+view.answer.task,{kind:'question',revision:q});else{view.threadNotice={thread:conversations.context().selected,text:'No question is loaded for this source. Load recorded history to inspect earlier questions.'};api.draw();}return;}
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
     else if(b.dataset.a==='workspace'){if(b.dataset.page==='operations')api.navigate('operations',{operation:b.dataset.operation,root:b.dataset.root||null});else if(b.closest('.conversation-rows')&&b.dataset.task){if(conversations.context().selected!=='task:'+b.dataset.board+'/'+Number(b.dataset.task))renderer.push({...state.capture(),conversation:conversations.context()});api.openTask(b.dataset.board,Number(b.dataset.task),b.dataset.root,false);if(b.dataset.openQuestion)conversations.target('task:'+b.dataset.board+'/'+b.dataset.task,{kind:'question',revision:Number(b.dataset.openQuestion)},false);api.draw();state.flush();}else api.navigate('conversations',{board:b.dataset.board,root:b.dataset.root,task:+b.dataset.task,...(b.dataset.openQuestion?{target:{kind:'question',revision:Number(b.dataset.openQuestion)}}:{})});}else if(b.dataset.a==='thread'){
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
     else if(b.dataset.a==='refresh'){if(view.page==='conversations')conversations.capture(false,true);view.notice='Refreshing workspace status…';ui.send({a:'refresh'});api.draw();}
     else if(b.dataset.a==='answer-retry')answers.retry(b.dataset.key);
     else if(b.dataset.a==='sync-retry')ui.send({a:'sync-retry',root:b.dataset.root,id:b.dataset.id});
     else if(b.dataset.a==='answer-dismiss')answers.dismiss(b.dataset.key);
    });
    ui.listen(menu,'input',e=>{if(view.page==='operations')operations.operationInput(e);if(e.target.dataset.cField==='result-reason')reports.reasonInput(e.target.value,[e.target.selectionStart,e.target.selectionEnd]);if(e.target.dataset.cField==='result-date')reports.dateInput(e.target.value);});
    for(const event of ['select','keyup','click'])ui.listen(menu,event,e=>{if(e.target.dataset.cField==='result-reason')reports.reasonInput(null,[e.target.selectionStart,e.target.selectionEnd]);const c=api.answerContext();if(e.target.dataset.cField==='steer'&&c)answers.steerCaret(c.board,c.task,[e.target.selectionStart,e.target.selectionEnd]);if(e.target.dataset.cField==='answer'&&c)answers.answerInput(c.board,c.task,null,[e.target.selectionStart,e.target.selectionEnd]);});
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
   })
  };
  return api;
 }
});
module.exports={create};
