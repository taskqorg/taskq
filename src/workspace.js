'use strict';
const ui=require('./ui-runtime.js'),shell=require('./app-shell.js'),{signal}=require('@preact/signals'),{execution}=ui;
const {create}=execution.module('workspace',{
 create:function(window=globalThis){
  const document=window.document,menu=document.querySelector('.menu'),renderer=shell.create(menu),snapshot=signal(null),esc=shell.escape;
  const view={page:'messages',taskFilter:null,working:null,aiRoot:null,answer:null,disclosures:{},restoreScroll:0},trail=[];
  const routes={messages:{title:'Messages'},request:{title:'New AI request',parent:'messages'},answer:{title:'Task answer',parent:'messages'},sessions:{title:'Work sessions',parent:'messages'},agents:{title:'Agent settings',parent:'messages'}};
  const state=require('./tray-view-state.js').create(view,{menu,document,routes,trail,root:'messages',defer:ui.defer,cancel:ui.cancel,send:m=>ui.send({...m,a:'workspace-set'})});
  const local=signal({text:'',caret:[0,0],op:null,receipt:null});let hydrated=false,timer=null,composing=false;
  const answers=require('./workspace-answers.js').create(view,{menu,document,send:m=>{ui.send(m);if(m.a==='answer-local')state.flush();},draw:()=>api.draw()});
  const api=execution.module('workspace.instance',{
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
   request:function(){
    const s=snapshot.value,ps=view.S.projects,r=s.ai?.request,bad=api.requestProblem(),l=local.value;
    return '<label for="request-project">Project</label><select id="request-project" data-field="project"'+(l.op&&['sending','unknown'].includes(l.receipt?.state)?' disabled':'')+'><option value="">Choose a project</option>'+(!ps.some(p=>p.root===view.aiRoot)&&view.aiRoot?'<option value="'+esc(view.aiRoot)+'" selected disabled>Unavailable project</option>':'')+ps.map(p=>'<option value="'+esc(p.root)+'"'+(p.root===view.aiRoot?' selected':'')+'>'+esc(p.name)+'</option>').join('')+'</select>'+
     '<p class="meta">Runtime: '+esc(s.ai?.runtime?.name||'unknown')+'. The native session owns all replies. Codex sessions are unavailable here.</p>'+
     '<form data-form="request"><label for="request-text">Request to AI</label><textarea id="request-text" data-field="request" rows="4"'+(l.op&&['sending','unknown'].includes(l.receipt?.state)?' readonly':'')+'>'+esc(l.text)+'</textarea><div class="options"><button type="submit" data-a="request-send"'+(bad?' disabled':'')+' aria-describedby="request-reason">Send</button></div><p id="request-reason">'+esc(bad)+'</p></form>'+
     (l.receipt?'<p role="status" aria-live="polite">'+esc(l.receipt.state+(l.receipt.reason?': '+l.receipt.reason:''))+'</p>':'')+
     (l.receipt?.state==='unknown'?'<button data-a="request-retry">Check delivery</button>':'')+
     '<details data-view-key="request-history"><summary>Recent requests</summary>'+(s.ai?.history||[]).map((t,i)=>'<button class="project" data-a="request-pick" data-index="'+i+'">'+esc(t)+'</button>').join('')+'</details>'+
     (r?'<section aria-label="Latest native request"><p>Request · '+esc(r.kind||'unknown')+'</p><p>'+esc(r.reason||r.next||'Request state unknown; inspect the native runtime before retrying.')+'</p>'+(r.url&&/^https:\/\/claude\.ai\/code\/session_[A-Za-z0-9]+$/.test(r.url)?'<a data-url="'+esc(r.url)+'" href="'+esc(r.url)+'">Open session</a>':'')+'</section>':'');
   },
   body:function(){
    if(view.page==='request')return api.request();
    if(view.page==='answer'){
     const c=view.answer,p=view.S.projects.find(p=>p.issues===c?.board&&(!c.root||p.root===c.root)),t=p?.tasks?.find(t=>t.n===c.task);
     if(!p||!t)return '<p role="status">This task is unavailable in the current snapshot. Refresh or open its board; no answer was sent.</p>'+(c?.board?'<a href="'+esc(c.board+'/'+c.task)+'" data-url="'+esc(c.board+'/'+c.task)+'">Open task on board</a>':'');
     const row={...t,key:p.issues+'/'+t.n};
     return '<p class="meta">'+esc(p.name)+' · <a href="'+esc(p.issues)+'" data-url="'+esc(p.issues)+'">Board</a> · attempt '+esc(t.attempt??'unknown')+' · revision '+esc(t.revision??'unknown')+'</p>'+(!p.alive||p.complete===false?'<p role="status">Project status is unavailable, stale or partial. Refresh or inspect the board; answers are disabled.</p>':'')+
      (c.revision&&c.revision!==t.revision?'<p role="status">The question changed since this launcher was opened. Read the current question before answering.</p>':'')+
      (t.state==='ask'?answers.answerSlot(row):'<p role="status">Task state: '+esc(t.state)+'. There is no open question.</p>')+answers.answerNotices(p.issues);
    }
    if(view.page==='messages'){
     const ps=view.S.projects,questions=ps.flatMap(p=>(p.tasks||[]).filter(t=>t.state==='ask').map(t=>'<button class="project" data-a="open-answer" data-root="'+esc(p.root)+'" data-board="'+esc(p.issues)+'" data-task="'+t.n+'" data-revision="'+t.revision+'">'+esc(p.name)+' · #'+t.n+' '+esc(t.title)+'</button>')).join('');
     return '<div class="options"><button data-route="request">New AI request</button><button data-route="sessions">Work sessions</button><button data-route="agents">Agent settings</button></div>'+
      (ps.some(p=>!p.alive||p.complete===false)?'<p role="status">Questions may be missing: project status is stale, unavailable or partial. Refresh or inspect the board.</p>':'')+
      (questions||'<p>No questions in the available snapshot.</p>')+answers.answerNotices();
    }
    return view.page==='sessions'?'<p>Work sessions are not available here. Use the tray’s Agents view and the recorded native session links.</p>':'<p>Agent settings are not available here. Use Settings → Agents in the tray. Opening this screen changes no runtime or permissions.</p>';
   },
   draw:function(){
    const s=snapshot.value,active=document.activeElement,focus=state.capture().focus,scroll=view.restoreScroll??menu.querySelector('.content')?.scrollTop??0;
    document.documentElement.style.colorScheme=s?.theme||'light dark';menu.style.setProperty('--size',(s?.font||16)+'px');
    const actions='<button data-a="refresh" title="Refresh workspace status" aria-label="Refresh workspace status">Refresh</button><details class="shell-overflow" data-view-key="navigation-actions"><summary title="Workspace navigation" aria-label="Workspace navigation">⋯</summary><nav class="overflow-actions" aria-label="Workspace"><button data-route="messages">Messages</button><button data-route="request">New AI request</button><button data-route="sessions">Work sessions</button><button data-route="agents">Agent settings</button></nav></details>';
    renderer.draw(shell.bar({title:routes[view.page]?.title||'Unavailable',back:view.page!=='messages',actions,windows:'<button data-a="hide" title="Close workspace" aria-label="Close workspace">×</button>'})+'<main class="content"><div class="content-body">'+(view.unavailable?'<p role="status">'+esc(view.unavailable)+' The surviving destination is Messages.</p>':'')+(s?api.body():'<p>Loading workspace. Refresh if status does not arrive.</p>')+[view.navigationError,view.localError,s?.ai?.error].filter(Boolean).map(error=>'<p role="alert">'+esc(error)+'</p>').join('')+'<p class="meta" role="status" aria-live="polite" aria-atomic="true" data-status="workspace">'+esc(view.notice||'')+'</p></div></main>'+answers.answerToast(),'/'+view.page);
    answers.mountAnswers();state.restore(view.disclosures);menu.querySelector('.content').scrollTop=scroll;view.restoreScroll=null;
    if(active?.isConnected)active.focus({preventScroll:true});else if(focus)state.focus(focus);
    state.flush();
   },
   navigate:function(page,context){
    if(Object.hasOwn(routes,page))view.unavailable=null;
    else {view.unavailable='Requested destination is not available.';page='messages';}
    trail.push(state.capture());if(trail.length>32)trail.shift();view.page=page;
    if(page==='answer')view.answer=context;
    if(page==='request'&&context?.root){
     if(local.value.op&&['sending','unknown'].includes(local.value.receipt?.state)){view.aiRoot=local.value.op.root;view.notice='A request is awaiting readback in its original project. Check delivery before changing project.';}
     else view.aiRoot=context.root;
    }
    view.disclosures={};view.restoreScroll=0;api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');menu.querySelector('h1').focus();state.flush();
    if(page==='request'){const input=menu.querySelector('[data-field=request]');input?.setSelectionRange(...local.value.caret);}
   },
   back:function(){const prior=trail.pop()||{page:'messages',open:{},scroll:0};state.apply(prior);api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');state.focus(prior.focus);},
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
    if(s.diagnose){ui.execution.diagnose(s.diagnose.selector,Math.max(0,Math.min(60000,s.diagnose.until_ms-Date.now())),s.diagnose.limit);return;}
    if(s.answerReceipt){answers.answerReceipt(s.answerReceipt);return;}
    if(s.aiReceipt){
     const r=s.aiReceipt,l=local.value;if(r.id!==l.op?.id)return;
     ui.cancel(timer);local.value={...l,receipt:r,...(r.state==='accepted'?{text:'',caret:[0,0]}:{})};
     if(r.state==='accepted')local.value.op=null;
     api.persist();api.draw();return;
    }
    if(s.viewStateError!==undefined||s.workspaceLocalError!==undefined){if(s.viewStateError!==undefined)view.navigationError=s.viewStateError;if(s.workspaceLocalError!==undefined)view.localError=s.workspaceLocalError;api.draw();return;}
    if(s.open){const c=s.open;api.navigate(c.page,c);return;}
    const first=snapshot.value===null;snapshot.value=s;view.S=s;
    if(!hydrated){hydrated=true;const l=s.ai?.local;local.value=l&&typeof l.text==='string'?{...l,caret:l.caret||[0,0]}:{text:s.ai?.draft||'',caret:[0,0],op:null,receipt:null};if(local.value.op&&local.value.receipt?.state==='sending')local.value={...local.value,receipt:{state:'unknown',reason:'Workspace reopened; inspect delivery with the same request ID.'}};}
    answers.hydrate(s.workspace_local);state.hydrate({...s,navigation:s.workspace_navigation});
    if(first&&view.aiRoot===null)view.aiRoot=s.ai?.root||s.projects?.[0]?.root||null;
    if(local.value.op&&['sending','unknown'].includes(local.value.receipt?.state))view.aiRoot=local.value.op.root;
    const request=s.ai?.request;view.notice=request?'Native request: '+(request.kind||'unknown')+(request.reason?'. '+request.reason:''):'Workspace status received.';
    answers.answerReadback();api.draw();if(first){const input=menu.querySelector('[data-field=request]');input?.setSelectionRange(...local.value.caret);state.focus(view.restoreFocus);}
   },
   mount:function(){
    ui.connect();window.taskq={receive:api.receive};api.draw();
    ui.listen(menu,'click',e=>{
     const link=e.target.closest('a');if(link){e.preventDefault();ui.send({a:'open',url:link.dataset.url||link.href});return;}
     const b=e.target.closest('button');if(!b||b.disabled)return;
     if(b.dataset.a==='toast-dismiss'){view.toast=null;api.draw();}
     else if(b.dataset.route)api.navigate(b.dataset.route);
     else if(b.dataset.a==='open-answer')api.navigate('answer',{root:b.dataset.root,board:b.dataset.board,task:Number(b.dataset.task),revision:Number(b.dataset.revision)});
     else if(b.dataset.a==='back')api.back();
     else if(b.dataset.a==='hide'){api.persist();state.flush();ui.send({a:'hide'});}
     else if(b.dataset.a==='refresh'){view.notice='Refreshing workspace status…';ui.send({a:'refresh'});api.draw();}
     else if(b.dataset.a==='request-retry'){const l=local.value;if(l.op&&l.receipt?.state==='unknown'){local.value={...l,receipt:{state:'sending'}};api.persist();api.dispatch(l.op,true);api.draw();}}
     else if(b.dataset.a==='request-pick'&&!local.value.op){local.value={...local.value,text:snapshot.value.ai.history[Number(b.dataset.index)],caret:[0,0]};api.persist();api.draw();menu.querySelector('[data-field=request]')?.focus();}
     else if(b.dataset.a==='answer-retry')answers.retry(b.dataset.key);
     else if(b.dataset.a==='answer-dismiss')answers.dismiss(b.dataset.key);
    });
    ui.listen(menu,'submit',e=>{if(e.target.dataset.form==='request'){e.preventDefault();api.submit();}});
    ui.listen(menu,'input',e=>{if(e.target.dataset.field==='request'){local.value={...local.value,text:e.target.value,caret:[e.target.selectionStart,e.target.selectionEnd]};api.persist();api.draw();}});
    ui.listen(menu,'change',e=>{if(e.target.dataset.field==='project'){view.aiRoot=e.target.value;api.persist();api.draw();}});
    for(const event of ['select','keyup','click'])ui.listen(menu,event,e=>{if(e.target.dataset.field==='request'){local.value={...local.value,caret:[e.target.selectionStart,e.target.selectionEnd]};api.persist();}});
    ui.listen(menu,'compositionstart',()=>{composing=true;});ui.listen(menu,'compositionend',()=>{composing=false;});
    ui.listen(menu,'scroll',()=>state.schedule(),true);ui.listen(menu,'toggle',()=>state.schedule(),true);
    ui.listen(document,'keydown',e=>{
     if(e.target.dataset?.field==='request'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composing&&e.keyCode!==229){e.preventDefault();api.submit();return;}
     if(e.key==='Escape'){const overflow=menu.querySelector('.shell-overflow[open]');if(overflow){overflow.open=false;overflow.querySelector('summary').focus();return;}api.persist();state.flush();ui.send({a:'hide'});}
     if(e.altKey&&e.key==='ArrowLeft'&&view.page!=='messages'){e.preventDefault();api.back();}
    });
    ui.send({a:'ready'});
   }
  });
  return api;
 }
});
module.exports={create};
